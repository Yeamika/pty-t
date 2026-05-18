# pty-t

Cross-platform Rust PTY sharing demo with a server, terminal client, and WebSocket control plane.

## Run

```bash
cargo run -p pty_t_server --bin ptytd
```

Create a PTY from the server prompt:

```text
create main bash
```

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

Client ids are optional and will be generated automatically if omitted.

Client management commands:

```bash
ptyt --url ws://127.0.0.1:8080 list
ptyt --url ws://127.0.0.1:8080 detail main
ptyt --url ws://127.0.0.1:8080 create main bash
```

Remote create is disabled by default. Enable it from the `ptytd` prompt with `remote-create on` or start `ptytd --remote-create`.

## Library

`pty_t_server` can be embedded directly. The core library only manages PTY sessions and exposes control functions. `ptytd` is the thin server binary that exposes those controls over WebSocket and starts the interactive admin CLI.

```rust
use pty_t_server::{PtyManager, session::CommandSpec};

# async fn example() -> anyhow::Result<()> {
let manager = PtyManager::default_shell(80, 24);
manager.create_pty(
    "main",
    CommandSpec::new("bash")
        .args(["-lc", "echo hello from core"])
        .cwd("/tmp")
        .env("EXAMPLE", "1"),
    None,
    None,
)?;
let exit = manager.wait_exit_code("main")?;
# Ok(())
# }
```

`shell_manager` wraps the core library for remote execution workflows. It can open a `ptyt`-compatible WebSocket endpoint, but that endpoint only accepts terminal clients and does not expose admin create/control APIs.

```rust
use shell_manager::ShellManager;
use std::time::Duration;

# async fn example() -> anyhow::Result<()> {
let manager = ShellManager::default_shell(80, 24);
manager.create_bash("main")?;
manager.lock_pty("main")?; // controller becomes user 0

let output = manager
    .attach_execute("main", b"echo hello\n", Duration::from_millis(500))
    .await?;
let snapshot = manager.snapshot("main")?;

let _addr = manager.start_websocket("127.0.0.1:8080")?;
# Ok(())
# }
```

Run the demo:

```bash
cargo run -p shell_manager --bin shell-manager-demo
```

## Layout

- `crates/protocol`: shared WebSocket protocol types
- `crates/shared`: shared facade crate
- `crates/server`: PTY server and admin CLI
- `crates/client`: terminal client TUI
- `crates/shell_manager`: library/demo for controlled remote shell execution
