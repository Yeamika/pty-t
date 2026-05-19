use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use pty_t_protocol::{AdminText, ServerText, SessionDetail, SessionSummary};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub async fn list(url: &str) -> Result<()> {
    match request(url, AdminText::List).await? {
        ServerText::Sessions { sessions } => print_sessions(&sessions),
        ServerText::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("server returned an unexpected response to list")),
    }
}

pub async fn detail(url: &str, pty: String) -> Result<()> {
    match request(url, AdminText::Detail { pty }).await? {
        ServerText::Session { session } => {
            print_detail(&session);
            Ok(())
        }
        ServerText::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("server returned an unexpected response to detail")),
    }
}

pub struct CreateOptions {
    pub pty: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

pub async fn create(url: &str, options: CreateOptions) -> Result<()> {
    let CreateOptions {
        pty,
        program,
        args,
        cwd,
        env,
        cols,
        rows,
    } = options;

    let request = AdminText::Create {
        pty,
        program,
        args,
        cwd,
        env,
        cols,
        rows,
    };

    match self::request(url, request).await? {
        ServerText::Info { message } => {
            println!("{message}");
            Ok(())
        }
        ServerText::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!("server returned an unexpected response to create")),
    }
}

pub async fn history_limit(url: &str, pty: String, bytes: usize) -> Result<()> {
    match request(url, AdminText::HistoryLimit { pty, bytes }).await? {
        ServerText::Info { message } => {
            println!("{message}");
            Ok(())
        }
        ServerText::Error { message } => Err(anyhow!(message)),
        _ => Err(anyhow!(
            "server returned an unexpected response to history-limit"
        )),
    }
}

pub fn parse_env(values: Vec<String>) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for value in values {
        let Some((key, val)) = value.split_once('=') else {
            return Err(anyhow!("environment values must use KEY=VALUE: {value}"));
        };
        if key.is_empty() {
            return Err(anyhow!("environment key must not be empty: {value}"));
        }
        env.insert(key.to_string(), val.to_string());
    }
    Ok(env)
}

async fn request(url: &str, request: AdminText) -> Result<ServerText> {
    let (ws, _) = connect_async(url)
        .await
        .with_context(|| format!("connect {url}"))?;
    let (mut write, mut read) = ws.split();
    write
        .send(Message::Text(serde_json::to_string(&request)?.into()))
        .await?;

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Ping(data) => write.send(Message::Pong(data)).await?,
            Message::Close(_) => break,
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    Err(anyhow!("server closed before sending a response"))
}

fn print_sessions(sessions: &[SessionSummary]) -> Result<()> {
    if sessions.is_empty() {
        println!("no sessions");
        return Ok(());
    }

    println!(
        "{:<20} {:>8} {:>10} {:>9} {:>15} {:>12}  COMMAND",
        "PTY", "PID", "SIZE", "CLIENTS", "HISTORY", "CREATED"
    );
    for session in sessions {
        println!(
            "{:<20} {:>8} {:>10} {:>9} {:>15} {:>12}  {}",
            session.pty,
            opt_u32(session.process_id),
            format!("{}x{}", session.cols, session.rows),
            session.clients.len(),
            format!(
                "{}/{}",
                session.output_history_bytes, session.output_history_limit
            ),
            time_text(session.created_at),
            command_text(&session.command),
        );
    }
    Ok(())
}

fn print_detail(session: &SessionDetail) {
    println!("pty: {}", session.pty);
    println!("pid: {}", opt_u32(session.process_id));
    println!("command: {}", command_text(&session.command));
    println!("cwd: {}", session.cwd.as_deref().unwrap_or("-"));
    println!("created: {}", time_text(session.created_at));
    println!("size: {}x{}", session.cols, session.rows);
    println!(
        "history: {}/{} bytes",
        session.output_history_bytes, session.output_history_limit
    );
    println!(
        "controller: {}",
        session.controller.as_deref().unwrap_or("-")
    );
    println!("exit_code: {}", opt_u32(session.exit_code));

    if session.client_details.is_empty() {
        println!("clients: none");
    } else {
        println!("clients:");
        for client in &session.client_details {
            println!("  {}@{}", client.id, client.peer_addr);
        }
    }

    if session.env.is_empty() {
        println!("env: none");
    } else {
        println!("env:");
        for (key, value) in &session.env {
            println!("  {key}={value}");
        }
    }
}

fn command_text(argv: &[String]) -> String {
    if argv.is_empty() {
        return "-".to_string();
    }
    argv.join(" ")
}

fn opt_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn time_text(created_at: u64) -> String {
    if created_at == 0 {
        return "unknown".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(created_at);
    if now >= created_at {
        format!("{}s ago", (now - created_at) / 1000)
    } else {
        "now".to_string()
    }
}
