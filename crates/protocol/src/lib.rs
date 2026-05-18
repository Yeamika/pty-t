use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientText {
    Hello {
        id: String,
        pty: String,
        cols: u16,
        rows: u16,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    RequestControl,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdminText {
    Create {
        pty: String,
        program: String,
        #[serde(default)]
        args: Vec<String>,
        cols: Option<u16>,
        rows: Option<u16>,
    },
    List,
    Control {
        pty: String,
        id: String,
    },
    ResizePty {
        pty: String,
        cols: u16,
        rows: u16,
    },
    Send {
        pty: String,
        data: String,
    },
    Kill {
        pty: String,
    },
    Listen {
        addr: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub pty: String,
    pub command: Vec<String>,
    pub controller: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub clients: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerText {
    Meta {
        id: String,
        pty: String,
        role: String,
        cols: u16,
        rows: u16,
    },
    Error {
        message: String,
    },
    Info {
        message: String,
    },
    Sessions {
        sessions: Vec<SessionSummary>,
    },
}

pub fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.max(1), rows.max(1))
}
