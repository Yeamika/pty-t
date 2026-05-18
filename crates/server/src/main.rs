mod cli;
mod connection;

use anyhow::Result;
use clap::Parser;
use pty_t_server::session::{CommandSpec, TermSize};
use pty_t_server::{default_shell, PtyManager};

use crate::cli::{cli_loop, print_help};
use crate::connection::start_listener;

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

    #[arg(long)]
    remote_create: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let program = args.shell.unwrap_or_else(default_shell);
    let manager = PtyManager::new(
        CommandSpec::new(program),
        TermSize {
            cols: args.cols,
            rows: args.rows,
        },
    );
    manager.set_remote_create_enabled(args.remote_create);

    let state = manager.state();
    let _ = start_listener(args.listen, state.clone())?;
    print_help();

    tokio::spawn(async move {
        if let Err(err) = cli_loop(state).await {
            eprintln!("cli error: {err:#}");
        }
    });

    std::future::pending::<()>().await;
    Ok(())
}
