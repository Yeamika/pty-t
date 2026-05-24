# pty-t

[English](README.md) | 简体中文

`pty-t` 是一个多 viewer 的共享 PTY 工具，可以共享给你的小伙伴，或者你的 Claude、GPT。

`tmux` 面向本地终端复用；`pty-t` 面向通过 WebSocket 共享同一个 PTY session。

## 怎么用

启动服务端：

```bash
cargo run -p pty_t_server --bin ptytd
```

在服务端提示符里创建 session：

```text
create main bash
```

连接客户端：

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

常用客户端命令：

```bash
ptyt --url ws://127.0.0.1:8080 list
ptyt --url ws://127.0.0.1:8080 detail main
ptyt --url ws://127.0.0.1:8080 create main bash
ptyt --url ws://127.0.0.1:8080 history-limit main 1048576
```

常用服务端命令：

```text
list
create main bash
control main <client-id>
resize main 120 40
send main echo hello
history-limit main 1048576
kill main
```

远程创建 PTY 默认关闭。打开方式：

```text
remote-create on
```

或者：

```bash
ptytd --remote-create
```

`pty_t_core` 是可嵌入的库部分。它负责 PTY session、controller 状态、输出历史、进程状态、resize、输入和订阅，不依赖 WebSocket 服务端。

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

## 目录

- `crates/protocol`：WebSocket 协议类型
- `crates/core`：不含网络层的 PTY/session 核心
- `crates/server`：WebSocket 服务端和 admin CLI
- `crates/client`：终端客户端 TUI
