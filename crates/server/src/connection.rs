use crate::session::CommandSpec;
use crate::state::ServerState;
use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use pty_t_demo::protocol::{AdminText, ClientText, ServerText};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

pub fn start_listener(addr: String, state: Arc<ServerState>) -> Result<String> {
    let std_listener =
        std::net::TcpListener::bind(&addr).with_context(|| format!("bind {addr}"))?;
    std_listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(std_listener)?;
    let actual_addr = listener.local_addr()?.to_string();
    state.add_listener(actual_addr.clone());
    println!("websocket listening on ws://{actual_addr}");

    tokio::spawn(async move {
        accept_loop(listener, state).await;
    });

    Ok(actual_addr)
}

async fn accept_loop(listener: TcpListener, state: Arc<ServerState>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, state).await {
                        eprintln!("connection error: {err:#}");
                    }
                });
            }
            Err(err) => {
                eprintln!("listener accept error: {err:#}");
                break;
            }
        }
    }
}

pub async fn handle_connection(stream: TcpStream, state: Arc<ServerState>) -> Result<()> {
    let ws = accept_async(stream).await?;
    let (mut ws_write, mut ws_read) = ws.split();

    let first = ws_read
        .next()
        .await
        .ok_or_else(|| anyhow!("client disconnected before hello"))??;
    let first_text = first
        .into_text()
        .context("first websocket frame must be hello text")?;
    let Ok(ClientText::Hello {
        id,
        pty,
        cols,
        rows,
    }) = serde_json::from_str::<ClientText>(&first_text)
    else {
        let admin = serde_json::from_str::<AdminText>(&first_text)?;
        let response = handle_admin_command(state.clone(), admin).await;
        send_admin_response(&mut ws_write, response).await?;

        while let Some(msg) = ws_read.next().await {
            match msg? {
                Message::Text(text) => {
                    let response = match serde_json::from_str::<AdminText>(&text) {
                        Ok(admin) => handle_admin_command(state.clone(), admin).await,
                        Err(err) => Err(anyhow!("bad admin message: {err}")),
                    };
                    send_admin_response(&mut ws_write, response).await?;
                }
                Message::Ping(data) => ws_write.send(Message::Pong(data)).await?,
                Message::Close(_) => break,
                Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }

        return Ok(());
    };

    let session = state.get_or_create_session(&pty, cols, rows)?;
    let token = state.next_token();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    let requested_id = id;
    let id = session.register_client(requested_id.clone(), token, tx.clone(), cols, rows)?;
    if id == requested_id {
        println!("client {id} joined pty {pty}");
    } else {
        println!("client {requested_id} joined pty {pty} as {id}");
    }

    let mut result = Ok(());
    while let Some(msg) = ws_read.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(err) => {
                eprintln!("client {id} disconnected without close handshake: {err}");
                break;
            }
        };

        result = match msg {
            Message::Binary(data) => session.write_from_client(&id, token, &data),
            Message::Text(text) => match serde_json::from_str::<ClientText>(&text) {
                Ok(ClientText::Resize { cols, rows }) => {
                    session.set_client_size(&id, token, cols, rows)
                }
                Ok(ClientText::RequestControl) => session.set_controller(&id),
                Ok(ClientText::Hello { .. }) => Ok(()),
                Err(err) => {
                    let msg = ServerText::Error {
                        message: format!("bad client message: {err}"),
                    };
                    if let Ok(text) = serde_json::to_string(&msg) {
                        let _ = tx.send(Message::Text(text.into()));
                    }
                    Ok(())
                }
            },
            Message::Ping(data) => {
                let _ = tx.send(Message::Pong(data));
                Ok(())
            }
            Message::Close(_) => break,
            Message::Pong(_) | Message::Frame(_) => Ok(()),
        };

        if result.is_err() {
            break;
        }
    }

    session.unregister_client(&id, token);
    writer_task.abort();
    println!("client {id} left pty {pty}");
    result
}

async fn send_admin_response(
    ws_write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    response: Result<ServerText>,
) -> Result<()> {
    let msg = match response {
        Ok(msg) => msg,
        Err(err) => ServerText::Error {
            message: err.to_string(),
        },
    };
    ws_write
        .send(Message::Text(serde_json::to_string(&msg)?.into()))
        .await?;
    Ok(())
}

async fn handle_admin_command(state: Arc<ServerState>, msg: AdminText) -> Result<ServerText> {
    match msg {
        AdminText::Create {
            pty,
            program,
            args,
            cols,
            rows,
        } => {
            if program.is_empty() {
                return Err(anyhow!("program must not be empty"));
            }
            let command = CommandSpec { program, args };
            let argv = command.argv().join(" ");
            state.create_session(pty.clone(), command, cols, rows)?;
            Ok(ServerText::Info {
                message: format!("created pty {pty}: {argv}"),
            })
        }
        AdminText::List => Ok(ServerText::Sessions {
            sessions: state.summaries(),
        }),
        AdminText::Control { pty, id } => {
            let session = state
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.set_controller(&id)?;
            Ok(ServerText::Info {
                message: format!("controller for {pty} is now {id}"),
            })
        }
        AdminText::ResizePty { pty, cols, rows } => {
            let session = state
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.resize(cols, rows)?;
            Ok(ServerText::Info {
                message: format!("resized {pty} to {cols}x{rows}"),
            })
        }
        AdminText::Send { pty, data } => {
            let session = state
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.write_from_server(data.as_bytes())?;
            Ok(ServerText::Info {
                message: format!("sent {} bytes to {pty}", data.len()),
            })
        }
        AdminText::Kill { pty } => {
            let session = state
                .remove_session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.kill()?;
            Ok(ServerText::Info {
                message: format!("killed {pty}"),
            })
        }
        AdminText::Listen { addr } => {
            let actual = start_listener(addr.clone(), state)?;
            Ok(ServerText::Info {
                message: format!("listening on ws://{actual}"),
            })
        }
    }
}
