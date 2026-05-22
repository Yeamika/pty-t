use crate::session::Session;
use crate::state::ServerState;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::{CommandSpec, SessionDetail, SessionSummary, TermSize};

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
        Self::new(CommandSpec::new(default_shell()), TermSize { cols, rows })
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
        self.create_pty(name, CommandSpec::new(default_shell()), None, None)
    }

    pub fn session(&self, name: &str) -> Option<Arc<Session>> {
        self.state.session(name)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        self.state.summaries()
    }

    pub fn detail(&self, pty: &str) -> Result<SessionDetail> {
        self.state.detail(pty)
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

    pub fn snapshot_pty_plain(&self, pty: &str) -> Result<String> {
        Ok(self.state.require_session(pty)?.snapshot_plain())
    }

    pub fn subscribe_output(&self, pty: &str) -> Result<mpsc::UnboundedReceiver<Vec<u8>>> {
        Ok(self.state.require_session(pty)?.subscribe_output())
    }

    pub fn process_id(&self, pty: &str) -> Result<Option<u32>> {
        Ok(self.state.require_session(pty)?.process_id())
    }

    pub fn try_exit_code(&self, pty: &str) -> Result<Option<u32>> {
        self.state.require_session(pty)?.try_exit_code()
    }

    pub fn wait_exit_code(&self, pty: &str) -> Result<u32> {
        self.state.require_session(pty)?.wait_exit_code()
    }

    pub async fn wait_exit_code_timeout(
        &self,
        pty: &str,
        timeout: Duration,
    ) -> Result<Option<u32>> {
        let session = self.state.require_session(pty)?;
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let Some(code) = session.try_exit_code()? {
                return Ok(Some(code));
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(None);
            }

            let sleep = (deadline - now).min(Duration::from_millis(20));
            tokio::time::sleep(sleep).await;
        }
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
