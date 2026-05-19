# pty-t

[English](README.md) | 简体中文

`pty-t` 是一个用 Rust 编写的共享 PTY 工具和库。它的核心作用是把一个本地 PTY 会话暴露成可共享的终端会话，让多个客户端通过 WebSocket 连接到同一个 PTY：所有客户端都能看到输出，其中一个客户端作为 controller 负责输入，其他客户端作为 viewer 旁观。

这个项目适合用于：

- 远程共享一个 shell/命令行会话
- 多人查看同一个终端输出
- 在自己的服务里嵌入 PTY 会话管理能力
- 基于 WebSocket 构建终端协作或远程调试工具

## 核心设计

- `pty_t_core` 只管理 PTY/session/controller/输出历史/进程状态，不接触网络层。
- `ptytd` 是服务端二进制，负责启动 WebSocket 监听和本地 admin CLI。
- `ptyt` 是终端客户端，负责连接服务端、渲染终端输出和发送输入。
- `pty_t_protocol` 定义 client/server/admin 使用的 WebSocket JSON 协议。

每个 PTY session 默认保留最多 `1 MiB` 输出历史，新的客户端连接后可以先拿到当前终端快照，再继续接收实时输出。history 上限可以通过 admin 命令调整。

## 快速开始

启动服务端：

```bash
cargo run -p pty_t_server --bin ptytd
```

在 `ptytd` 提示符里创建一个 PTY：

```text
create main bash
```

连接到这个 PTY：

```bash
cargo run -p pty_t_client --bin ptyt -- --url ws://127.0.0.1:8080 --pty main
```

客户端 id 是可选的；不指定时会自动生成。

## 管理命令

通过客户端查看和管理 session：

```bash
ptyt --url ws://127.0.0.1:8080 list
ptyt --url ws://127.0.0.1:8080 detail main
ptyt --url ws://127.0.0.1:8080 create main bash
ptyt --url ws://127.0.0.1:8080 history-limit main 1048576
```

服务端本地 CLI 也支持类似命令：

```text
list
create main bash
control main <client-id>
resize main 120 40
send main echo hello
history-limit main 1048576
kill main
```

远程创建 PTY 默认关闭。可以在 `ptytd` 提示符里执行：

```text
remote-create on
```

也可以在启动时启用：

```bash
ptytd --remote-create
```

## 作为库使用

如果只需要 PTY/session 管理，不需要 WebSocket 服务，可以直接嵌入 `pty_t_core`：

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

## 目录结构

- `crates/protocol`: WebSocket 协议类型
- `crates/core`: 不含网络层的 PTY/session 核心
- `crates/server`: WebSocket 服务端和 admin CLI
- `crates/client`: 终端客户端 TUI

## 注意事项

- 默认监听地址是 `127.0.0.1:8080`。
- admin/control plane 目前没有鉴权。
- 不建议在没有鉴权、TLS 或网络访问控制的情况下直接暴露到公网。
