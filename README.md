# pty-t

Cross-platform Rust PTY sharing demo with a server, terminal client, and WebSocket control plane.

## Run

```bash
cargo run -p pty_t_server --bin s
```

```bash
cargo run -p pty_t_client --bin c -- --url ws://127.0.0.1:8080 --id 001 --pty main
```

## Layout

- `crates/protocol`: shared WebSocket protocol types
- `crates/shared`: shared facade crate
- `crates/server`: PTY server and admin CLI
- `crates/client`: terminal client TUI
