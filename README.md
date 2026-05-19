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
ptyt --url ws://127.0.0.1:8080 history-limit main 1048576
```

Remote create is disabled by default. Enable it from the `ptytd` prompt with `remote-create on` or start `ptytd --remote-create`.

PTY output history is retained per session for late subscribers. The default limit is 1 MiB per PTY. Set it from the server prompt with `history-limit <pty> <bytes>` or remotely with `ptyt history-limit <pty> <bytes>`.

## Library

`pty_t_core` can be embedded directly. The core library only manages PTY sessions and exposes control functions. `ptytd` is the thin server binary that exposes those controls over WebSocket and starts the interactive admin CLI.

```rust
use pty_t_core::{PtyManager, session::CommandSpec};

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

## Layout

- `crates/protocol`: shared WebSocket protocol types
- `crates/core`: PTY/session core without network code
- `crates/server`: PTY WebSocket server and admin CLI
- `crates/client`: terminal client TUI
