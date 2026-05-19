# pty-t

English | [简体中文](README.zh-CN.md)

`pty-t` is a Rust PTY sharing tool and library. Its core purpose is to expose a local PTY as a shared terminal session, so multiple clients can connect to the same PTY over WebSocket. Every client can see the output, while one client acts as the controller and sends input; the rest are viewers.

Use `pty-t` to:

- Share a shell or command-line session remotely
- Let multiple people watch the same terminal output
- Embed PTY session management into another Rust service
- Build terminal collaboration or remote debugging tools on top of WebSocket

## Core Design

- `pty_t_core` manages PTY sessions, controllers, output history, and process state. It does not contain any network code.
- `ptytd` is the server binary. It starts the WebSocket listener and local admin CLI.
- `ptyt` is the terminal client. It connects to the server, renders terminal output, and sends input.
- `pty_t_protocol` defines the WebSocket JSON protocol used by clients, servers, and admin commands.

Each PTY session keeps up to `1 MiB` of output history by default. New clients receive the current terminal snapshot first, then continue receiving live output. The history limit can be changed through admin commands.

## Quick Start

Start the server:

```bash
cargo run -p pty_t_server --bin ptytd
```

Create a PTY from the `ptytd` prompt:

```text
create main bash
```

Connect to the PTY:

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

Client ids are optional. If omitted, one is generated automatically.

## Admin Commands

Inspect and manage sessions from the client:

```bash
ptyt --url ws://127.0.0.1:8080 list
ptyt --url ws://127.0.0.1:8080 detail main
ptyt --url ws://127.0.0.1:8080 create main bash
ptyt --url ws://127.0.0.1:8080 history-limit main 1048576
```

The local `ptytd` CLI supports similar commands:

```text
list
create main bash
control main <client-id>
resize main 120 40
send main echo hello
history-limit main 1048576
kill main
```

Remote PTY creation is disabled by default. Enable it from the `ptytd` prompt:

```text
remote-create on
```

Or enable it at startup:

```bash
ptytd --remote-create
```

## Library Usage

If you only need PTY/session management without WebSocket serving, embed `pty_t_core` directly:

```rust
use pty_t_core::{session::CommandSpec, PtyManager};

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

- `crates/protocol`: WebSocket protocol types
- `crates/core`: PTY/session core without networking
- `crates/server`: WebSocket server and admin CLI
- `crates/client`: terminal client TUI

## Security Notes

- The default listen address is `127.0.0.1:8080`.
- The admin/control plane currently has no authentication.
- Do not expose it directly to the public internet without adding authentication, TLS, or network-level access control.
