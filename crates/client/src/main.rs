mod admin;
mod input;
mod keys;
mod render;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::event::KeyEvent;
use crossterm::terminal;
use crossterm::{cursor, execute};
use futures_util::{SinkExt, StreamExt};
use input::spawn_input_thread;
use keys::process_key;
use pty_t_protocol::{clamp_size, ClientText, ServerText};
use render::{draw_message, render};
use std::io::stdout;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{self, Instant};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub(crate) type ClientWsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:8080", global = true)]
    url: String,

    #[arg(long, global = true)]
    id: Option<String>,

    #[arg(long, default_value = "main", global = true)]
    pty: String,

    #[arg(long, global = true)]
    cols: Option<u16>,

    #[arg(long, global = true)]
    rows: Option<u16>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    List,
    Detail {
        pty: String,
    },
    Create {
        pty: String,
        program: String,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    HistoryLimit {
        pty: String,
        bytes: usize,
    },
}

pub(crate) enum LocalEvent {
    Key(KeyEvent),
    Resize { cols: u16, rows: u16 },
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusMode {
    Input,
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatusView {
    Normal,
    Link,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandSelection {
    Mode,
    Identity,
}

pub(crate) struct ViewState {
    pub(crate) id: String,
    pub(crate) pty: String,
    pub(crate) role: String,
    pub(crate) pty_cols: u16,
    pub(crate) pty_rows: u16,
    pub(crate) local_cols: u16,
    pub(crate) local_rows: u16,
    pub(crate) focus: FocusMode,
    pub(crate) status_view: StatusView,
    pub(crate) command_selection: CommandSelection,
    pub(crate) ctrl_c_count: u8,
}

struct PendingPing {
    seq: u64,
    sent_at: Instant,
}

pub(crate) struct Metrics {
    pub(crate) tx_bytes: u64,
    pub(crate) rx_bytes: u64,
    last_output: Instant,
    rtt: Option<Duration>,
    next_ping_seq: u64,
    pending_ping: Option<PendingPing>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            tx_bytes: 0,
            rx_bytes: 0,
            last_output: Instant::now(),
            rtt: None,
            next_ping_seq: 1,
            pending_ping: None,
        }
    }

    pub(crate) fn record_tx(&mut self, len: usize) {
        self.tx_bytes += len as u64;
    }

    fn record_rx(&mut self, len: usize, output: bool) {
        self.rx_bytes += len as u64;
        if output {
            self.last_output = Instant::now();
        }
    }

    fn ping_due(&self) -> bool {
        match self.pending_ping {
            None => true,
            Some(PendingPing { sent_at, .. }) => sent_at.elapsed() >= Duration::from_secs(5),
        }
    }

    fn note_ping_sent(&mut self) -> u64 {
        let seq = self.next_ping_seq;
        self.next_ping_seq = self.next_ping_seq.wrapping_add(1);
        self.pending_ping = Some(PendingPing {
            seq,
            sent_at: Instant::now(),
        });
        seq
    }

    fn note_pong(&mut self, data: &[u8]) -> bool {
        if data.len() != 8 {
            return false;
        }

        let mut seq_bytes = [0u8; 8];
        seq_bytes.copy_from_slice(data);
        let seq = u64::from_be_bytes(seq_bytes);

        let Some(PendingPing {
            seq: pending_seq,
            sent_at,
        }) = self.pending_ping.take()
        else {
            return false;
        };

        if seq != pending_seq {
            self.pending_ping = Some(PendingPing {
                seq: pending_seq,
                sent_at,
            });
            return false;
        }

        self.rtt = Some(sent_at.elapsed());
        true
    }

    pub(crate) fn latency_text(&self) -> String {
        match self.rtt {
            Some(rtt) => render::format_duration(rtt),
            None => "?".to_string(),
        }
    }

    pub(crate) fn idle_text(&self) -> String {
        render::format_duration(self.last_output.elapsed())
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

struct TerminalOptions {
    url: String,
    id: Option<String>,
    pty: String,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Clone, Copy)]
struct TerminalSize {
    local_cols: u16,
    local_rows: u16,
    desired_cols: u16,
    desired_rows: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::List) => admin::list(&args.url).await,
        Some(Command::Detail { pty }) => admin::detail(&args.url, pty).await,
        Some(Command::Create {
            pty,
            program,
            cwd,
            env,
            args: command_args,
        }) => {
            let size = resolve_terminal_size(args.cols, args.rows)?;
            admin::create(
                &args.url,
                admin::CreateOptions {
                    pty: pty.clone(),
                    program,
                    args: command_args,
                    cwd,
                    env: admin::parse_env(env)?,
                    cols: Some(size.desired_cols),
                    rows: Some(size.desired_rows),
                },
            )
            .await?;
            run_terminal_with_size(
                TerminalOptions {
                    url: args.url,
                    id: args.id,
                    pty,
                    cols: args.cols,
                    rows: args.rows,
                },
                size,
            )
            .await
        }
        Some(Command::HistoryLimit { pty, bytes }) => {
            admin::history_limit(&args.url, pty, bytes).await
        }
        None => {
            run_terminal(TerminalOptions {
                url: args.url,
                id: args.id,
                pty: args.pty,
                cols: args.cols,
                rows: args.rows,
            })
            .await
        }
    }
}

async fn run_terminal(options: TerminalOptions) -> Result<()> {
    let size = resolve_terminal_size(options.cols, options.rows)?;
    run_terminal_with_size(options, size).await
}

async fn run_terminal_with_size(options: TerminalOptions, size: TerminalSize) -> Result<()> {
    let TerminalSize {
        local_cols,
        local_rows,
        desired_cols,
        desired_rows,
    } = size;

    let _guard = TerminalGuard::enter()?;
    let mut out = stdout();

    let (ws, _) = connect_async(&options.url)
        .await
        .with_context(|| format!("connect {}", options.url))?;
    let (mut ws_write, mut ws_read) = ws.split();
    let id = options.id.unwrap_or_else(random_client_id);

    let hello = ClientText::Hello {
        id: id.clone(),
        pty: options.pty.clone(),
        cols: desired_cols,
        rows: desired_rows,
    };
    let mut metrics = Metrics::new();
    let hello_json = serde_json::to_string(&hello)?;
    metrics.record_tx(hello_json.len());
    ws_write.send(Message::Text(hello_json.into())).await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<LocalEvent>();
    spawn_input_thread(tx);

    let mut parser = vt100::Parser::new(desired_rows, desired_cols, 2000);
    let mut ping_tick = time::interval_at(
        Instant::now() + Duration::from_secs(3),
        Duration::from_secs(3),
    );
    ping_tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut ctrl_c_streak = 0u8;
    let mut view = ViewState {
        id,
        pty: options.pty,
        role: "Viewer".to_string(),
        pty_cols: desired_cols,
        pty_rows: desired_rows,
        local_cols,
        local_rows,
        focus: FocusMode::Input,
        status_view: StatusView::Normal,
        command_selection: CommandSelection::Mode,
        ctrl_c_count: 0,
    };

    render(&mut out, &parser, &view, &metrics)?;

    loop {
        tokio::select! {
            local = rx.recv() => {
                let Some(local) = local else { break; };
                match local {
                    LocalEvent::Key(key) => {
                        if process_key(
                            key,
                            &parser,
                            &mut view,
                            &mut metrics,
                            &mut out,
                            &mut ws_write,
                            &mut ctrl_c_streak,
                        )
                        .await?
                        {
                            break;
                        }
                    }
                    LocalEvent::Resize { cols, rows } => {
                        view.local_cols = cols;
                        view.local_rows = rows;
                        let pty_rows = rows.saturating_sub(1).max(1);
                        let resize = ClientText::Resize { cols, rows: pty_rows };
                        let resize_json = serde_json::to_string(&resize)?;
                        metrics.record_tx(resize_json.len());
                        ws_write.send(Message::Text(resize_json.into())).await?;
                        render(&mut out, &parser, &view, &metrics)?;
                    }
                    LocalEvent::Quit => break,
                }
            }
            _ = ping_tick.tick() => {
                if metrics.ping_due() {
                    let seq = metrics.note_ping_sent();
                    metrics.record_tx(8);
                    ws_write
                        .send(Message::Ping(seq.to_be_bytes().to_vec().into()))
                        .await?;
                }
            }
            msg = ws_read.next() => {
                let Some(msg) = msg else { break; };
                match msg? {
                    Message::Binary(data) => {
                        metrics.record_rx(data.len(), true);
                        parser.process(&data);
                        render(&mut out, &parser, &view, &metrics)?;
                    }
                    Message::Text(text) => {
                        metrics.record_rx(text.len(), false);
                        match serde_json::from_str::<ServerText>(&text) {
                            Ok(ServerText::Meta { id, pty, role, cols, rows }) => {
                                view.id = id;
                                view.pty = pty;
                                view.role = role;
                                view.pty_cols = cols;
                                view.pty_rows = rows;
                                parser.screen_mut().set_size(rows, cols);
                                render(&mut out, &parser, &view, &metrics)?;
                            }
                            Ok(ServerText::Error { message }) | Ok(ServerText::Info { message }) => {
                                draw_message(&mut out, &message, &view)?;
                            }
                            Ok(ServerText::Sessions { .. }) | Ok(ServerText::Session { .. }) => {}
                            Err(_) => {}
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(data) => {
                        metrics.record_rx(data.len(), false);
                        metrics.record_tx(data.len());
                        ws_write.send(Message::Pong(data)).await?;
                    }
                    Message::Pong(data) => {
                        metrics.record_rx(data.len(), false);
                        if metrics.note_pong(&data) {
                            render(&mut out, &parser, &view, &metrics)?;
                        }
                    }
                    Message::Frame(_) => {}
                }
            }
        }
    }

    Ok(())
}

fn resolve_terminal_size(cols: Option<u16>, rows: Option<u16>) -> Result<TerminalSize> {
    let (local_cols, local_rows) = terminal::size()?;
    let desired_cols = cols.unwrap_or(local_cols);
    let desired_rows = rows.unwrap_or_else(|| local_rows.saturating_sub(1).max(1));
    let (desired_cols, desired_rows) = clamp_size(desired_cols, desired_rows);
    Ok(TerminalSize {
        local_cols,
        local_rows,
        desired_cols,
        desired_rows,
    })
}

fn random_client_id() -> String {
    format!("client-{:016x}", rand::random::<u64>())
}
