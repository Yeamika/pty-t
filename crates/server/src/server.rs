use crate::cli::cli_loop;
use crate::connection::start_listener;
use crate::session::{CommandSpec, Session, TermSize};
use crate::state::ServerState;
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct PtyServer {
    state: Arc<ServerState>,
}

impl PtyServer {
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

    pub fn start_websocket(&self, addr: impl Into<String>) -> Result<String> {
        start_listener(addr.into(), self.state.clone())
    }

    pub async fn run_cli(&self) -> Result<()> {
        cli_loop(self.state.clone()).await
    }
}

pub fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string())
    }
}
