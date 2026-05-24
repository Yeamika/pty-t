# pty-t

[简体中文](README.zh-CN.md) | English

`pty-t` is a shared PTY tool with multiple viewers. You can share it with your teammates, or with Claude and GPT.

`tmux` is for local terminal multiplexing; `pty-t` is for sharing one PTY session over WebSocket.

## Usage

Start the server:

```bash
cargo run -p pty_t_server --bin ptytd
```

Create a session from the server prompt:

```text
create main bash
```

Connect a client:

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

Useful client commands:

```bash
ptyt --url ws://127.0.0.1:8080 list
ptyt --url ws://127.0.0.1:8080 detail main
ptyt --url ws://127.0.0.1:8080 create main bash
ptyt --url ws://127.0.0.1:8080 history-limit main 1048576
```

Useful server commands:

```text
list
create main bash
control main <client-id>
resize main 120 40
send main echo hello
history-limit main 1048576
kill main
```

Remote PTY creation is disabled by default. Enable it with:

```text
remote-create on
```

or:

```bash
ptytd --remote-create
```

`pty_t_core` is the embeddable library part. It handles PTY sessions, controller state, output history, process state, resizing, input, and subscriptions without the WebSocket server.

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
