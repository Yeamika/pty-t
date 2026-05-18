use anyhow::Result;
use clap::Parser;
use pty_t_server::cli::{cli_loop, print_help};
use pty_t_server::connection::start_listener;
use pty_t_server::session::{CommandSpec, TermSize};
use pty_t_server::state::ServerState;
use std::sync::Arc;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,

    #[arg(long)]
    shell: Option<String>,

    #[arg(long, default_value_t = 80)]
    cols: u16,

    #[arg(long, default_value_t = 24)]
    rows: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let program = args.shell.unwrap_or_else(default_shell);
    let state = Arc::new(ServerState::new(
        CommandSpec {
            program,
            args: Vec::new(),
        },
        TermSize {
            cols: args.cols,
            rows: args.rows,
        },
    ));

    let _ = start_listener(args.listen, state.clone())?;
    print_help();

    let cli_state = state.clone();
    tokio::spawn(async move {
        if let Err(err) = cli_loop(cli_state).await {
            eprintln!("cli error: {err:#}");
        }
    });

    std::future::pending::<()>().await;
    Ok(())
}

fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string())
    }
}
