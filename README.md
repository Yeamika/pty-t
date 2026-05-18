# pty-t

Cross-platform Rust PTY sharing demo with a server, terminal client, and WebSocket control plane.

## Run

```bash
cargo run -p pty_t_server --bin pyttd
```

Create a PTY from the server prompt:

```text
create main bash
```

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

Client ids are optional and will be generated automatically if omitted.

## Library

`pty_t_server` can be embedded directly. `pyttd` is only a thin binary that exposes the library over WebSocket and starts the interactive admin CLI.

```rust
use pty_t_server::{PtyServer, session::CommandSpec};

# async fn example() -> anyhow::Result<()> {
let server = PtyServer::default_shell(80, 24);
server.create_pty(
    "main",
    CommandSpec { program: "bash".into(), args: vec![] },
    None,
    None,
)?;
server.start_websocket("127.0.0.1:8080")?;
# Ok(())
# }
```

## Layout

- `crates/protocol`: shared WebSocket protocol types
- `crates/shared`: shared facade crate
- `crates/server`: PTY server and admin CLI
- `crates/client`: terminal client TUI
