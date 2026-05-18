use crate::ShellManager;
use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use pty_t_demo::protocol::{ClientText, ServerText};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

pub fn start_client_listener(addr: String, manager: ShellManager) -> Result<String> {
    let std_listener =
        std::net::TcpListener::bind(&addr).with_context(|| format!("bind {addr}"))?;
    std_listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(std_listener)?;
    let actual_addr = listener.local_addr()?.to_string();

    tokio::spawn(async move {
        accept_loop(listener, manager).await;
    });

    Ok(actual_addr)
}

async fn accept_loop(listener: TcpListener, manager: ShellManager) {
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let manager = manager.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_client(stream, peer_addr, manager).await {
                        eprintln!("shell-manager websocket error: {err:#}");
                    }
                });
            }
            Err(err) => {
                eprintln!("shell-manager accept error: {err:#}");
                break;
            }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    peer_addr: SocketAddr,
    manager: ShellManager,
) -> Result<()> {
    let ws = accept_async(stream).await?;
    let (mut ws_write, mut ws_read) = ws.split();
    let first = ws_read
        .next()
        .await
        .ok_or_else(|| anyhow!("client disconnected before hello"))??;
    let first_text = first
        .into_text()
        .context("first frame must be hello text")?;
    let Ok(ClientText::Hello {
        id,
        pty,
        cols,
        rows,
    }) = serde_json::from_str::<ClientText>(&first_text)
    else {
        send_error(&mut ws_write, "admin websocket messages are not supported").await?;
        return Ok(());
    };

    let Some(session) = manager.core().session(&pty) else {
        send_error(&mut ws_write, &format!("pty {pty} does not exist")).await?;
        return Ok(());
    };

    let token = rand_token();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    let id = session.register_client(id, token, tx.clone(), peer_addr, cols, rows)?;
    let mut result = Ok(());
    while let Some(msg) = ws_read.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(err) => {
                eprintln!("shell-manager client {id} disconnected: {err}");
                break;
            }
        };

        result = handle_client_message(&manager, &session, &tx, &pty, &id, token, msg).await;
        if result.is_err() {
            break;
        }
    }

    session.unregister_client(&id, token);
    writer_task.abort();
    result
}

async fn handle_client_message(
    manager: &ShellManager,
    session: &std::sync::Arc<pty_t_server::session::Session>,
    tx: &mpsc::UnboundedSender<Message>,
    pty: &str,
    id: &str,
    token: u64,
    msg: Message,
) -> Result<()> {
    match msg {
        Message::Binary(data) => session.write_from_client(id, token, &data),
        Message::Text(text) => match serde_json::from_str::<ClientText>(&text) {
            Ok(ClientText::Resize { cols, rows }) => session.set_client_size(id, token, cols, rows),
            Ok(ClientText::RequestControl) => {
                if manager.is_locked(pty) && id != "0" {
                    send_error_tx(tx, "pty is locked to user 0");
                    Ok(())
                } else {
                    session.set_controller(id)
                }
            }
            Ok(ClientText::Hello { .. }) => Ok(()),
            Err(err) => {
                send_error_tx(tx, &format!("bad client message: {err}"));
                Ok(())
            }
        },
        Message::Ping(data) => {
            let _ = tx.send(Message::Pong(data));
            Ok(())
        }
        Message::Close(_) | Message::Pong(_) | Message::Frame(_) => Ok(()),
    }
}

async fn send_error(
    ws_write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    message: &str,
) -> Result<()> {
    ws_write
        .send(Message::Text(error_text(message)?.into()))
        .await?;
    Ok(())
}

fn send_error_tx(tx: &mpsc::UnboundedSender<Message>, message: &str) {
    if let Ok(text) = error_text(message) {
        let _ = tx.send(Message::Text(text.into()));
    }
}

fn error_text(message: &str) -> Result<String> {
    Ok(serde_json::to_string(&ServerText::Error {
        message: message.to_string(),
    })?)
}

fn rand_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}
