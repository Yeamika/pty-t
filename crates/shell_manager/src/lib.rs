mod websocket;

use anyhow::Result;
use pty_t_demo::protocol::SessionSummary;
use pty_t_server::session::{CommandSpec, Session, TermSize};
use pty_t_server::{default_shell, PtyManager};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{self, Instant};

#[derive(Clone)]
pub struct ShellManager {
    core: PtyManager,
    locked: Arc<Mutex<HashSet<String>>>,
}

impl ShellManager {
    pub fn new(default_command: CommandSpec, default_size: TermSize) -> Self {
        Self {
            core: PtyManager::new(default_command, default_size),
            locked: Arc::new(Mutex::new(HashSet::new())),
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

    pub fn core(&self) -> PtyManager {
        self.core.clone()
    }

    pub fn create_pty(
        &self,
        name: impl Into<String>,
        command: CommandSpec,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Arc<Session>> {
        self.core.create_pty(name, command, cols, rows)
    }

    pub fn create_bash(&self, name: impl Into<String>) -> Result<Arc<Session>> {
        self.core.create_bash(name)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        self.core.list()
    }

    pub fn lock_pty(&self, pty: &str) -> Result<()> {
        self.core.force_controller(pty, "0")?;
        self.locked.lock().unwrap().insert(pty.to_string());
        Ok(())
    }

    pub fn unlock_pty(&self, pty: &str) {
        self.locked.lock().unwrap().remove(pty);
    }

    pub fn is_locked(&self, pty: &str) -> bool {
        self.locked.lock().unwrap().contains(pty)
    }

    pub fn start_websocket(&self, addr: impl Into<String>) -> Result<String> {
        websocket::start_client_listener(addr.into(), self.clone())
    }

    pub async fn attach_execute(
        &self,
        pty: &str,
        input: impl AsRef<[u8]>,
        collect_for: Duration,
    ) -> Result<Vec<u8>> {
        let mut rx = self.core.subscribe_output(pty)?;
        self.core.send_to_pty(pty, input.as_ref())?;

        let deadline = Instant::now() + collect_for;
        let mut output = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }

            match time::timeout(deadline - now, rx.recv()).await {
                Ok(Some(chunk)) => output.extend(chunk),
                Ok(None) | Err(_) => break,
            }
        }
        Ok(output)
    }

    pub fn snapshot(&self, pty: &str) -> Result<Vec<u8>> {
        self.core.snapshot_pty(pty)
    }
}
