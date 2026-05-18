use crate::types::{path_text, OutputState};
use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use pty_t_demo::protocol::{clamp_size, ClientSummary, SessionDetail, SessionSummary};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

pub use crate::client_id::allocate_client_id;
pub use crate::types::{ClientInfo, CommandSpec, TermSize};

pub struct Session {
    name: String,
    command: CommandSpec,
    cwd: Option<std::path::PathBuf>,
    created_at: u64,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    clients: Mutex<HashMap<String, ClientInfo>>,
    controller: Mutex<Option<String>>,
    parser: Mutex<vt100::Parser>,
    output: Mutex<OutputState>,
    size: Mutex<TermSize>,
}

impl Session {
    pub fn new(name: String, command: CommandSpec, cols: u16, rows: u16) -> Result<Arc<Self>> {
        let (cols, rows) = clamp_size(cols, rows);
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&command.program);
        for arg in &command.args {
            cmd.arg(arg.as_str());
        }
        if let Some(cwd) = &command.cwd {
            cmd.cwd(cwd);
        }
        for (key, value) in &command.env {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let session = Arc::new(Self {
            name,
            cwd: command.effective_cwd(),
            command,
            created_at: now_millis(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            clients: Mutex::new(HashMap::new()),
            controller: Mutex::new(None),
            parser: Mutex::new(vt100::Parser::new(rows, cols, 2000)),
            output: Mutex::new(OutputState {
                history: Vec::new(),
                subscribers: Vec::new(),
            }),
            size: Mutex::new(TermSize { cols, rows }),
        });

        let weak = Arc::downgrade(&session);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let Some(session) = weak.upgrade() else {
                    break;
                };
                session.on_pty_output(&buf[..n]);
            }
        });

        Ok(session)
    }

    pub fn on_pty_output(&self, data: &[u8]) {
        self.parser.lock().unwrap().process(data);
        let data_vec = data.to_vec();

        let mut output = self.output.lock().unwrap();
        output.history.extend(&data_vec);
        output
            .subscribers
            .retain(|tx| tx.send(data_vec.clone()).is_ok());
        drop(output);

        let clients = self
            .clients
            .lock()
            .unwrap()
            .values()
            .map(|client| client.tx.clone())
            .collect::<Vec<_>>();

        for tx in clients {
            let _ = tx.send(Message::Binary(data.to_vec().into()));
        }
    }

    pub fn register_client(
        &self,
        id: String,
        token: u64,
        tx: mpsc::UnboundedSender<Message>,
        peer_addr: SocketAddr,
        cols: u16,
        rows: u16,
    ) -> Result<String> {
        let (cols, rows) = clamp_size(cols, rows);
        let id = {
            let mut clients = self.clients.lock().unwrap();
            let id = allocate_client_id(&clients, &id);
            clients.insert(
                id.clone(),
                ClientInfo::new(token, tx.clone(), TermSize { cols, rows }, peer_addr),
            );
            id
        };

        if self.controller_id().is_none() {
            *self.controller.lock().unwrap() = Some(id.clone());
        }

        if self.controller_id().as_deref() == Some(id.as_str()) {
            self.resize(cols, rows)?;
        } else {
            self.broadcast_meta();
        }

        let snapshot = self.parser.lock().unwrap().screen().state_formatted();
        let _ = tx.send(Message::Binary(snapshot.into()));
        Ok(id)
    }

    pub fn unregister_client(&self, id: &str, token: u64) {
        let removed = {
            let mut clients = self.clients.lock().unwrap();
            if clients.get(id).map(|client| client.token()) == Some(token) {
                clients.remove(id);
                true
            } else {
                false
            }
        };

        if !removed {
            return;
        }

        let was_controller = self.controller_id().as_deref() == Some(id);
        if was_controller {
            let next = self.clients.lock().unwrap().keys().next().cloned();
            *self.controller.lock().unwrap() = next.clone();

            if let Some(next_id) = next {
                if let Some(size) = self.client_size(&next_id) {
                    if self.resize(size.cols, size.rows).is_ok() {
                        return;
                    }
                }
            }
        }

        self.broadcast_meta();
    }

    pub fn client_size(&self, id: &str) -> Option<TermSize> {
        self.clients
            .lock()
            .unwrap()
            .get(id)
            .map(|client| client.size())
    }

    pub fn controller_id(&self) -> Option<String> {
        self.controller.lock().unwrap().clone()
    }

    pub fn force_controller(&self, id: impl Into<String>) {
        *self.controller.lock().unwrap() = Some(id.into());
        self.broadcast_meta();
    }

    pub fn set_controller(&self, id: &str) -> Result<()> {
        let size = self
            .client_size(id)
            .ok_or_else(|| anyhow!("client {id} is not connected to pty {}", self.name))?;

        *self.controller.lock().unwrap() = Some(id.to_string());
        self.resize(size.cols, size.rows)
    }

    pub fn set_client_size(&self, id: &str, token: u64, cols: u16, rows: u16) -> Result<()> {
        let (cols, rows) = clamp_size(cols, rows);
        let valid = {
            let mut clients = self.clients.lock().unwrap();
            let Some(client) = clients.get_mut(id) else {
                return Ok(());
            };
            if client.token() != token {
                return Ok(());
            }
            client.set_size(TermSize { cols, rows });
            true
        };

        if valid && self.controller_id().as_deref() == Some(id) {
            self.resize(cols, rows)?;
        }
        Ok(())
    }

    pub fn write_from_client(&self, id: &str, token: u64, data: &[u8]) -> Result<()> {
        let token_is_current = self
            .clients
            .lock()
            .unwrap()
            .get(id)
            .map(|client| client.token())
            == Some(token);

        if !token_is_current || self.controller_id().as_deref() != Some(id) {
            return Ok(());
        }

        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn write_from_server(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn subscribe_output(&self) -> mpsc::UnboundedReceiver<Vec<u8>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut output = self.output.lock().unwrap();
        if !output.history.is_empty() {
            let _ = tx.send(output.history.clone());
        }
        output.subscribers.push(tx);
        rx
    }

    pub fn snapshot_formatted(&self) -> Vec<u8> {
        self.parser.lock().unwrap().screen().state_formatted()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let (cols, rows) = clamp_size(cols, rows);
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.master.lock().unwrap().resize(size)?;
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
        *self.size.lock().unwrap() = TermSize { cols, rows };
        self.broadcast_meta();
        Ok(())
    }

    pub fn broadcast_meta(&self) {
        let size = *self.size.lock().unwrap();
        let controller = self.controller_id();
        let clients = self
            .clients
            .lock()
            .unwrap()
            .iter()
            .map(|(id, client)| (id.clone(), client.tx.clone()))
            .collect::<Vec<_>>();

        for (id, tx) in clients {
            let role = if controller.as_deref() == Some(id.as_str()) {
                "Controler"
            } else {
                "Viewer"
            };
            let msg = pty_t_demo::protocol::ServerText::Meta {
                id: id.clone(),
                pty: self.name.clone(),
                role: role.to_string(),
                cols: size.cols,
                rows: size.rows,
            };

            if let Ok(text) = serde_json::to_string(&msg) {
                let _ = tx.send(Message::Text(text.into()));
            }
        }
    }

    pub fn kill(&self) -> Result<()> {
        self.child.lock().unwrap().kill()?;
        Ok(())
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.lock().unwrap().process_id()
    }

    pub fn try_exit_code(&self) -> Result<Option<u32>> {
        Ok(self
            .child
            .lock()
            .unwrap()
            .try_wait()?
            .map(|status| status.exit_code()))
    }

    pub fn wait_exit_code(&self) -> Result<u32> {
        Ok(self.child.lock().unwrap().wait()?.exit_code())
    }

    pub fn summary(&self) -> SessionSummary {
        let size = *self.size.lock().unwrap();
        let controller = self.controller_id();
        let clients = self.clients.lock().unwrap();
        let mut ids = clients.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        let mut client_details = clients
            .iter()
            .map(|(id, client)| ClientSummary {
                id: id.clone(),
                peer_addr: client.peer_addr().to_string(),
            })
            .collect::<Vec<_>>();
        client_details.sort_by(|a, b| a.id.cmp(&b.id));
        SessionSummary {
            pty: self.name.clone(),
            command: self.command.argv(),
            controller,
            cols: size.cols,
            rows: size.rows,
            process_id: self.process_id(),
            created_at: self.created_at,
            clients: ids,
            client_details,
        }
    }

    pub fn detail(&self) -> SessionDetail {
        let summary = self.summary();
        SessionDetail {
            pty: summary.pty,
            command: summary.command,
            cwd: path_text(self.cwd.as_deref()),
            env: self.command.env.clone(),
            process_id: summary.process_id,
            created_at: self.created_at,
            controller: summary.controller,
            cols: summary.cols,
            rows: summary.rows,
            clients: summary.clients,
            client_details: summary.client_details,
            exit_code: self.try_exit_code().ok().flatten(),
        }
    }

    pub fn summary_line(&self) -> String {
        let summary = self.summary();
        format!(
            "pty={} cmd={} size={}x{} controller={} clients=[{}]",
            summary.pty,
            summary.command.join(" "),
            summary.cols,
            summary.rows,
            summary.controller.unwrap_or_else(|| "-".to_string()),
            summary
                .client_details
                .iter()
                .map(|client| format!("{}@{}", client.id, client.peer_addr))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
