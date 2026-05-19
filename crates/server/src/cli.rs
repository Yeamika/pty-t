use anyhow::{anyhow, Result};
use pty_t_core::session::CommandSpec;
use pty_t_protocol::SessionSummary;
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::connection::{start_listener, ServerRuntime};

pub async fn cli_loop(runtime: Arc<ServerRuntime>) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("s> ");
        std::io::stdout().flush()?;

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields = match shell_words::split(line) {
            Ok(fields) => fields,
            Err(err) => {
                println!("parse error: {err}");
                continue;
            }
        };

        let mut parts = fields.iter();
        let cmd = parts.next().map(String::as_str).unwrap_or_default();
        match cmd {
            "help" | "h" => print_help(),
            "list" | "ls" => {
                let sessions = runtime.summaries();
                if sessions.is_empty() {
                    println!("no sessions");
                } else {
                    for session in sessions {
                        println!("{}", summary_line(&session));
                    }
                }
                let listeners = runtime.listeners();
                println!("listeners=[{}]", listeners.join(","));
                println!("remote_create={}", runtime.remote_create_enabled());
            }
            "remote-create" | "remote_create" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: remote-create <on|off>"))?;
                let enabled = match value.as_str() {
                    "on" | "true" | "1" | "enable" | "enabled" => true,
                    "off" | "false" | "0" | "disable" | "disabled" => false,
                    _ => return Err(anyhow!("usage: remote-create <on|off>")),
                };
                runtime.set_remote_create_enabled(enabled);
                println!("remote_create={enabled}");
            }
            "create" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: create <pty> <program> [args...]"))?;
                let program = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: create <pty> <program> [args...]"))?;
                let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                let command = CommandSpec::new(program.to_string()).args(args);
                let argv = command.argv().join(" ");
                runtime
                    .core()
                    .create_session(pty.to_string(), command, None, None)?;
                println!("created pty {pty}: {argv}");
            }
            "listen" => {
                let addr = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: listen <ip:port>"))?;
                let actual = start_listener(addr.to_string(), runtime.clone())?;
                println!("listening on ws://{actual}");
            }
            "control" | "controller" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: control <pty> <client-id>"))?;
                let id = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: control <pty> <client-id>"))?;
                let session = runtime
                    .core()
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.set_controller(id)?;
                runtime.broadcast_meta(pty);
                println!("controller for {pty} is now {id}");
            }
            "resize" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: resize <pty> <cols> <rows>"))?;
                let cols = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: resize <pty> <cols> <rows>"))?
                    .parse::<u16>()?;
                let rows = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: resize <pty> <cols> <rows>"))?
                    .parse::<u16>()?;
                let session = runtime
                    .core()
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.resize(cols, rows)?;
                runtime.broadcast_meta(pty);
                println!("resized {pty} to {cols}x{rows}");
            }
            "send" | "input" => {
                let mut split = line.splitn(3, char::is_whitespace);
                let _ = split.next();
                let pty = split
                    .next()
                    .ok_or_else(|| anyhow!("usage: send <pty> <text>"))?;
                let text = split.next().unwrap_or_default();
                let session = runtime
                    .core()
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(b'\n');
                session.write_from_server(&bytes)?;
            }
            "kill" => {
                let pty = parts.next().ok_or_else(|| anyhow!("usage: kill <pty>"))?;
                let session = runtime
                    .core()
                    .remove_session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.kill()?;
                runtime.close_pty_clients(pty);
                println!("killed {pty}");
            }
            "history-limit" | "history_limit" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: history-limit <pty> <bytes>"))?;
                let bytes = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: history-limit <pty> <bytes>"))?
                    .parse::<usize>()?;
                let session = runtime
                    .core()
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.set_output_history_limit(bytes);
                println!("history limit for {pty} is now {bytes} bytes");
            }
            "quit" | "exit" => std::process::exit(0),
            _ => println!("unknown command: {cmd}; try help"),
        }
    }

    Ok(())
}

pub fn print_help() {
    println!("commands: help | list | create <pty> <program> [args...] | remote-create <on|off> | listen <ip:port> | control <pty> <client-id> | resize <pty> <cols> <rows> | send <pty> <text> | kill <pty> | history-limit <pty> <bytes> | quit");
}

fn summary_line(session: &SessionSummary) -> String {
    let clients = if session.client_details.is_empty() {
        session.clients.join(",")
    } else {
        session
            .client_details
            .iter()
            .map(|client| format!("{}@{}", client.id, client.peer_addr))
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        "pty={} cmd={} size={}x{} history={}/{} controller={} clients=[{}]",
        session.pty,
        session.command.join(" "),
        session.cols,
        session.rows,
        session.output_history_bytes,
        session.output_history_limit,
        session.controller.as_deref().unwrap_or("-"),
        clients,
    )
}
