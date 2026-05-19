use anyhow::Result;
use clap::Parser;
use pty_t_core::session::{CommandSpec, TermSize};
use pty_t_core::{default_shell, PtyManager};
use pty_t_server::cli::{cli_loop, print_help};
use pty_t_server::connection::{start_listener, ServerRuntime};
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

    let runtime = Arc::new(ServerRuntime::new(manager.state()));
    runtime.set_remote_create_enabled(args.remote_create);

    let _ = start_listener(args.listen, runtime.clone())?;
    print_help();

    tokio::spawn(async move {
        if let Err(err) = cli_loop(runtime).await {
            eprintln!("cli error: {err:#}");
        }
    });

    std::future::pending::<()>().await;
    Ok(())
}
