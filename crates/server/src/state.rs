use crate::session::{CommandSpec, Session, TermSize};
use anyhow::{anyhow, Result};
use pty_t_demo::protocol::SessionSummary;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct ServerState {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next_token: AtomicU64,
    default_command: CommandSpec,
    default_size: TermSize,
    listeners: Mutex<Vec<String>>,
}

impl ServerState {
    pub fn new(default_command: CommandSpec, default_size: TermSize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            default_command,
            default_size,
            listeners: Mutex::new(Vec::new()),
        }
    }

    pub fn next_token(&self) -> u64 {
        self.next_token.fetch_add(1, Ordering::Relaxed)
    }

    pub fn add_listener(&self, addr: String) {
        self.listeners.lock().unwrap().push(addr);
    }

    pub fn listeners(&self) -> Vec<String> {
        self.listeners.lock().unwrap().clone()
    }

    pub fn get_or_create_session(&self, name: &str, cols: u16, rows: u16) -> Result<Arc<Session>> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(name).cloned() {
            return Ok(session);
        }

        let cols = if cols == 0 {
            self.default_size.cols
        } else {
            cols
        };
        let rows = if rows == 0 {
            self.default_size.rows
        } else {
            rows
        };
        let session = Session::new(name.to_string(), self.default_command.clone(), cols, rows)?;
        sessions.insert(name.to_string(), session.clone());
        Ok(session)
    }

    pub fn create_session(
        &self,
        name: String,
        command: CommandSpec,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Arc<Session>> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(&name) {
            return Err(anyhow!("pty {name} already exists"));
        }

        let cols = cols.unwrap_or(self.default_size.cols);
        let rows = rows.unwrap_or(self.default_size.rows);
        let session = Session::new(name.clone(), command, cols, rows)?;
        sessions.insert(name, session.clone());
        Ok(session)
    }

    pub fn session(&self, name: &str) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().get(name).cloned()
    }

    pub fn require_session(&self, name: &str) -> Result<Arc<Session>> {
        self.session(name)
            .ok_or_else(|| anyhow!("pty {name} does not exist"))
    }

    pub fn all_sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    pub fn summaries(&self) -> Vec<SessionSummary> {
        let mut summaries = self
            .all_sessions()
            .into_iter()
            .map(|session| session.summary())
            .collect::<Vec<_>>();
        summaries.sort_by(|a, b| a.pty.cmp(&b.pty));
        summaries
    }

    pub fn remove_session(&self, name: &str) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().remove(name)
    }

    pub fn require_removed_session(&self, name: &str) -> Result<Arc<Session>> {
        self.remove_session(name)
            .ok_or_else(|| anyhow!("pty {name} does not exist"))
    }
}
