use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use pty_t_core::session::CommandSpec;
use pty_t_core::state::ServerState;
use pty_t_protocol::{
    AdminText, ClientSummary, ClientText, ServerText, SessionDetail, SessionSummary,
};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

pub struct ServerRuntime {
    core: Arc<ServerState>,
    remote_create_enabled: AtomicBool,
    listeners: Mutex<Vec<String>>,
    clients: Mutex<HashMap<String, HashMap<String, ConnectedClient>>>,
    exit_watchers: Mutex<HashSet<String>>,
}

struct ConnectedClient {
    token: u64,
    tx: mpsc::UnboundedSender<Message>,
    peer_addr: SocketAddr,
}

impl ServerRuntime {
    pub fn new(core: Arc<ServerState>) -> Self {
        Self {
            core,
            remote_create_enabled: AtomicBool::new(false),
            listeners: Mutex::new(Vec::new()),
            clients: Mutex::new(HashMap::new()),
            exit_watchers: Mutex::new(HashSet::new()),
        }
    }

    pub fn core(&self) -> Arc<ServerState> {
        self.core.clone()
    }

    pub fn remote_create_enabled(&self) -> bool {
        self.remote_create_enabled.load(Ordering::Relaxed)
    }

    pub fn set_remote_create_enabled(&self, enabled: bool) {
        self.remote_create_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn add_listener(&self, addr: String) {
        self.listeners.lock().unwrap().push(addr);
    }

    pub fn listeners(&self) -> Vec<String> {
        self.listeners.lock().unwrap().clone()
    }

    fn register_client(
        &self,
        pty: &str,
        id: String,
        token: u64,
        tx: mpsc::UnboundedSender<Message>,
        peer_addr: SocketAddr,
    ) {
        let mut clients = self.clients.lock().unwrap();
        clients.entry(pty.to_string()).or_default().insert(
            id,
            ConnectedClient {
                token,
                tx,
                peer_addr,
            },
        );
    }

    fn remove_client(&self, pty: &str, id: &str, token: u64) {
        let mut clients = self.clients.lock().unwrap();
        let Some(session_clients) = clients.get_mut(pty) else {
            return;
        };
        if session_clients.get(id).map(|client| client.token) == Some(token) {
            session_clients.remove(id);
        }
        if session_clients.is_empty() {
            clients.remove(pty);
        }
    }

    pub fn close_pty_clients(&self, pty: &str) {
        let clients = self.clients.lock().unwrap().remove(pty);
        if let Some(clients) = clients {
            for client in clients.into_values() {
                let _ = client.tx.send(Message::Close(None));
            }
        }
    }

    fn client_details(&self, pty: &str) -> Vec<ClientSummary> {
        let clients = self.clients.lock().unwrap();
        let Some(session_clients) = clients.get(pty) else {
            return Vec::new();
        };

        let mut client_details = session_clients
            .iter()
            .map(|(id, client)| ClientSummary {
                id: id.clone(),
                peer_addr: client.peer_addr.to_string(),
            })
            .collect::<Vec<_>>();
        client_details.sort_by(|a, b| a.id.cmp(&b.id));
        client_details
    }

    pub fn summaries(&self) -> Vec<SessionSummary> {
        self.core
            .summaries()
            .into_iter()
            .map(|summary| self.attach_client_details(summary))
            .collect()
    }

    pub fn detail(&self, pty: &str) -> Result<SessionDetail> {
        let detail = self.core.detail(pty)?;
        Ok(self.attach_client_details_to_detail(detail))
    }

    fn attach_client_details(&self, summary: pty_t_core::SessionSummary) -> SessionSummary {
        let client_details = self.client_details(&summary.pty);
        SessionSummary {
            pty: summary.pty,
            command: summary.command,
            controller: summary.controller,
            cols: summary.cols,
            rows: summary.rows,
            process_id: summary.process_id,
            created_at: summary.created_at,
            exit_code: summary.exit_code,
            output_history_bytes: summary.output_history_bytes,
            output_history_limit: summary.output_history_limit,
            clients: summary.clients,
            client_details,
        }
    }

    fn attach_client_details_to_detail(&self, detail: pty_t_core::SessionDetail) -> SessionDetail {
        let client_details = self.client_details(&detail.pty);
        SessionDetail {
            pty: detail.pty,
            command: detail.command,
            cwd: detail.cwd,
            env: detail.env,
            process_id: detail.process_id,
            created_at: detail.created_at,
            controller: detail.controller,
            cols: detail.cols,
            rows: detail.rows,
            exit_code: detail.exit_code,
            output_history_bytes: detail.output_history_bytes,
            output_history_limit: detail.output_history_limit,
            clients: detail.clients,
            client_details,
        }
    }

    pub fn broadcast_meta(&self, pty: &str) {
        let Some(session) = self.core.session(pty) else {
            return;
        };
        let summary = session.summary();
        let controller = summary.controller;
        let clients = {
            let clients = self.clients.lock().unwrap();
            clients
                .get(pty)
                .map(|clients| {
                    clients
                        .iter()
                        .map(|(id, client)| (id.clone(), client.tx.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        for (id, tx) in clients {
            let role = if controller.as_deref() == Some(id.as_str()) {
                "Controller"
            } else {
                "Viewer"
            };
            let msg = ServerText::Meta {
                id: id.clone(),
                pty: summary.pty.clone(),
                role: role.to_string(),
                cols: summary.cols,
                rows: summary.rows,
                exit_code: summary.exit_code,
            };

            if let Ok(text) = serde_json::to_string(&msg) {
                let _ = tx.send(Message::Text(text.into()));
            }
        }
    }

    pub fn start_exit_watcher(self: &Arc<Self>, pty: &str) {
        {
            let mut exit_watchers = self.exit_watchers.lock().unwrap();
            if !exit_watchers.insert(pty.to_string()) {
                return;
            }
        }

        let pty = pty.to_string();
        let runtime = self.clone();
        tokio::spawn(async move {
            loop {
                let Some(session) = runtime.core.session(&pty) else {
                    break;
                };
                match session.try_exit_code() {
                    Ok(Some(_)) => {
                        runtime.broadcast_meta(&pty);
                        break;
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
                    Err(err) => {
                        eprintln!("exit watcher error for {pty}: {err:#}");
                        break;
                    }
                }
            }

            runtime.exit_watchers.lock().unwrap().remove(&pty);
        });
    }
}

pub fn start_listener(addr: String, runtime: Arc<ServerRuntime>) -> Result<String> {
    let std_listener =
        std::net::TcpListener::bind(&addr).with_context(|| format!("bind {addr}"))?;
    std_listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(std_listener)?;
    let actual_addr = listener.local_addr()?.to_string();
    runtime.add_listener(actual_addr.clone());
    println!("websocket listening on ws://{actual_addr}");

    tokio::spawn(async move {
        accept_loop(listener, runtime).await;
    });

    Ok(actual_addr)
}

async fn accept_loop(listener: TcpListener, runtime: Arc<ServerRuntime>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, peer_addr, runtime).await {
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

pub async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    runtime: Arc<ServerRuntime>,
) -> Result<()> {
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
        let response = handle_admin_command(runtime.clone(), admin).await;
        send_admin_response(&mut ws_write, response).await?;

        while let Some(msg) = ws_read.next().await {
            match msg? {
                Message::Text(text) => {
                    let response = match serde_json::from_str::<AdminText>(&text) {
                        Ok(admin) => handle_admin_command(runtime.clone(), admin).await,
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

    let Some(session) = runtime.core().session(&pty) else {
        send_admin_response(
            &mut ws_write,
            Ok(ServerText::Error {
                message: format!("pty {pty} does not exist; create it on the server first"),
            }),
        )
        .await?;
        return Ok(());
    };
    let token = runtime.core().next_token();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let output_rx = session.subscribe_live_output();

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    let requested_id = id;
    let id = session.register_client(requested_id.clone(), token, cols, rows)?;
    runtime.register_client(&pty, id.clone(), token, tx.clone(), peer_addr);
    runtime.start_exit_watcher(&pty);

    let snapshot = session.snapshot_formatted();
    let _ = tx.send(Message::Binary(snapshot.into()));
    runtime.broadcast_meta(&pty);

    let output_tx = tx.clone();
    let output_task = tokio::spawn(async move {
        let mut output_rx = output_rx;
        while let Some(data) = output_rx.recv().await {
            if output_tx.send(Message::Binary(data.into())).is_err() {
                break;
            }
        }
    });

    if id == requested_id {
        println!("client {id} from {peer_addr} joined pty {pty}");
    } else {
        println!("client {requested_id} from {peer_addr} joined pty {pty} as {id}");
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
                    let result = session.set_client_size(&id, token, cols, rows);
                    if result.is_ok() {
                        runtime.broadcast_meta(&pty);
                    }
                    result
                }
                Ok(ClientText::RequestControl) => {
                    let result = session.set_controller(&id);
                    if result.is_ok() {
                        runtime.broadcast_meta(&pty);
                    }
                    result
                }
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
    runtime.remove_client(&pty, &id, token);
    runtime.broadcast_meta(&pty);
    writer_task.abort();
    output_task.abort();
    let _ = writer_task.await;
    let _ = output_task.await;
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

async fn handle_admin_command(runtime: Arc<ServerRuntime>, msg: AdminText) -> Result<ServerText> {
    match msg {
        AdminText::Create {
            pty,
            program,
            args,
            cwd,
            env,
            cols,
            rows,
        } => {
            if !runtime.remote_create_enabled() {
                return Err(anyhow!(
                    "remote create is disabled; run `remote-create on` in ptytd to enable it"
                ));
            }
            if program.is_empty() {
                return Err(anyhow!("program must not be empty"));
            }
            let mut command = CommandSpec::new(program).args(args).envs(env);
            if let Some(cwd) = cwd {
                command = command.cwd(cwd);
            }
            let argv = command.argv().join(" ");
            runtime
                .core()
                .create_session(pty.clone(), command, cols, rows)?;
            Ok(ServerText::Info {
                message: format!("created pty {pty}: {argv}"),
            })
        }
        AdminText::List => Ok(ServerText::Sessions {
            sessions: runtime.summaries(),
        }),
        AdminText::Detail { pty } => Ok(ServerText::Session {
            session: runtime.detail(&pty)?,
        }),
        AdminText::Control { pty, id } => {
            let session = runtime
                .core()
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.set_controller(&id)?;
            runtime.broadcast_meta(&pty);
            Ok(ServerText::Info {
                message: format!("controller for {pty} is now {id}"),
            })
        }
        AdminText::ResizePty { pty, cols, rows } => {
            let session = runtime
                .core()
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.resize(cols, rows)?;
            runtime.broadcast_meta(&pty);
            Ok(ServerText::Info {
                message: format!("resized {pty} to {cols}x{rows}"),
            })
        }
        AdminText::Send { pty, data } => {
            let session = runtime
                .core()
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.write_from_server(data.as_bytes())?;
            Ok(ServerText::Info {
                message: format!("sent {} bytes to {pty}", data.len()),
            })
        }
        AdminText::Kill { pty } => {
            let session = runtime
                .core()
                .remove_session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.kill()?;
            runtime.close_pty_clients(&pty);
            Ok(ServerText::Info {
                message: format!("killed {pty}"),
            })
        }
        AdminText::HistoryLimit { pty, bytes } => {
            let session = runtime
                .core()
                .session(&pty)
                .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
            session.set_output_history_limit(bytes);
            Ok(ServerText::Info {
                message: format!("history limit for {pty} is now {bytes} bytes"),
            })
        }
        AdminText::Listen { addr } => {
            let actual = start_listener(addr.clone(), runtime)?;
            Ok(ServerText::Info {
                message: format!("listening on ws://{actual}"),
            })
        }
    }
}
