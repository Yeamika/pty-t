use crate::state::ServerState;
use anyhow::{anyhow, Result};
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn cli_loop(state: Arc<ServerState>) -> Result<()> {
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
                let sessions = state.all_sessions();
                if sessions.is_empty() {
                    println!("no sessions");
                } else {
                    for session in sessions {
                        println!("{}", session.summary_line());
                    }
                }
                let listeners = state.listeners();
                println!("listeners=[{}]", listeners.join(","));
            }
            "create" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: create <pty> <program> [args...]"))?;
                let program = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: create <pty> <program> [args...]"))?;
                let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                let command = crate::session::CommandSpec {
                    program: program.to_string(),
                    args,
                };
                let argv = command.argv().join(" ");
                state.create_session(pty.to_string(), command, None, None)?;
                println!("created pty {pty}: {argv}");
            }
            "listen" => {
                let addr = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: listen <ip:port>"))?;
                let actual = crate::connection::start_listener(addr.to_string(), state.clone())?;
                println!("listening on ws://{actual}");
            }
            "control" | "controller" => {
                let pty = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: control <pty> <client-id>"))?;
                let id = parts
                    .next()
                    .ok_or_else(|| anyhow!("usage: control <pty> <client-id>"))?;
                let session = state
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.set_controller(id)?;
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
                let session = state
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.resize(cols, rows)?;
                println!("resized {pty} to {cols}x{rows}");
            }
            "send" | "input" => {
                let mut split = line.splitn(3, char::is_whitespace);
                let _ = split.next();
                let pty = split
                    .next()
                    .ok_or_else(|| anyhow!("usage: send <pty> <text>"))?;
                let text = split.next().unwrap_or_default();
                let session = state
                    .session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(b'\n');
                session.write_from_server(&bytes)?;
            }
            "kill" => {
                let pty = parts.next().ok_or_else(|| anyhow!("usage: kill <pty>"))?;
                let session = state
                    .remove_session(pty)
                    .ok_or_else(|| anyhow!("pty {pty} does not exist"))?;
                session.kill()?;
                println!("killed {pty}");
            }
            "quit" | "exit" => std::process::exit(0),
            _ => println!("unknown command: {cmd}; try help"),
        }
    }

    Ok(())
}

pub fn print_help() {
    println!("commands: help | list | create <pty> <program> [args...] | listen <ip:port> | control <pty> <client-id> | resize <pty> <cols> <rows> | send <pty> <text> | kill <pty> | quit");
}
