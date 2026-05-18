use anyhow::Result;
use clap::Parser;
use pty_t_server::session::CommandSpec;
use shell_manager::ShellManager;
use std::time::Duration;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,

    #[arg(long, default_value = "main")]
    pty: String,

    #[arg(long)]
    program: Option<String>,

    #[arg(long)]
    lock: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let manager = ShellManager::default_shell(80, 24);
    let command = CommandSpec {
        program: args.program.unwrap_or_else(default_program),
        args: Vec::new(),
    };

    manager.create_pty(args.pty.clone(), command, None, None)?;
    if args.lock {
        manager.lock_pty(&args.pty)?;
    }

    let output = manager
        .attach_execute(
            &args.pty,
            b"echo shell-manager-ready\n",
            Duration::from_millis(300),
        )
        .await?;
    print!("{}", String::from_utf8_lossy(&output));

    let actual = manager.start_websocket(args.listen)?;
    println!("ptyt-compatible websocket listening on ws://{actual}");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn default_program() -> String {
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        "bash".to_string()
    }
}
