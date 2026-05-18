use crate::session::{CommandSpec, Session, TermSize};
use crate::state::ServerState;
use anyhow::Result;
use pty_t_demo::protocol::SessionSummary;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct PtyManager {
    state: Arc<ServerState>,
}

impl PtyManager {
    pub fn new(default_command: CommandSpec, default_size: TermSize) -> Self {
        Self {
            state: Arc::new(ServerState::new(default_command, default_size)),
        }
    }

    pub fn default_shell(cols: u16, rows: u16) -> Self {
        Self::new(
            CommandSpec {
                program: default_shell(),
                args: Vec::new(),
            },
            TermSize { cols, rows },
        )
    }

    pub fn state(&self) -> Arc<ServerState> {
        self.state.clone()
    }

    pub fn create_pty(
        &self,
        name: impl Into<String>,
        command: CommandSpec,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Arc<Session>> {
        self.state.create_session(name.into(), command, cols, rows)
    }

    pub fn create_bash(&self, name: impl Into<String>) -> Result<Arc<Session>> {
        self.create_pty(
            name,
            CommandSpec {
                program: default_shell(),
                args: Vec::new(),
            },
            None,
            None,
        )
    }

    pub fn session(&self, name: &str) -> Option<Arc<Session>> {
        self.state.session(name)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        self.state.summaries()
    }

    pub fn set_controller(&self, pty: &str, id: &str) -> Result<()> {
        self.state.require_session(pty)?.set_controller(id)
    }

    pub fn force_controller(&self, pty: &str, id: impl Into<String>) -> Result<()> {
        self.state.require_session(pty)?.force_controller(id);
        Ok(())
    }

    pub fn resize_pty(&self, pty: &str, cols: u16, rows: u16) -> Result<()> {
        self.state.require_session(pty)?.resize(cols, rows)
    }

    pub fn send_to_pty(&self, pty: &str, data: &[u8]) -> Result<()> {
        self.state.require_session(pty)?.write_from_server(data)
    }

    pub fn snapshot_pty(&self, pty: &str) -> Result<Vec<u8>> {
        Ok(self.state.require_session(pty)?.snapshot_formatted())
    }

    pub fn subscribe_output(&self, pty: &str) -> Result<mpsc::UnboundedReceiver<Vec<u8>>> {
        Ok(self.state.require_session(pty)?.subscribe_output())
    }

    pub fn kill_pty(&self, pty: &str) -> Result<()> {
        self.state.require_removed_session(pty)?.kill()
    }
}

pub type PtyServer = PtyManager;

pub fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string())
    }
}
