use crate::input::key_to_bytes;
use crate::{ClientWsSink, CommandSelection, FocusMode, Metrics, ViewState};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures_util::SinkExt;
use pty_t_protocol::ClientText;
use std::io::Stdout;
use tokio_tungstenite::tungstenite::Message;

pub(crate) async fn process_key(
    key: KeyEvent,
    parser: &vt100::Parser,
    view: &mut ViewState,
    metrics: &mut Metrics,
    out: &mut Stdout,
    ws_write: &mut ClientWsSink,
    ctrl_c_streak: &mut u8,
) -> Result<bool> {
    match view.focus {
        FocusMode::Input => {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            {
                *ctrl_c_streak = ctrl_c_streak.saturating_add(1);
                view.ctrl_c_count = *ctrl_c_streak;
                let bytes = vec![0x03];
                metrics.record_tx(bytes.len());
                ws_write.send(Message::Binary(bytes.into())).await?;

                if *ctrl_c_streak >= 3 {
                    *ctrl_c_streak = 0;
                    view.ctrl_c_count = 0;
                    view.focus = FocusMode::Command;
                    view.command_selection = CommandSelection::Mode;
                }

                crate::render::render(out, parser, view, metrics)?;
                return Ok(false);
            }

            let had_ctrl_c_hint = *ctrl_c_streak > 0;
            *ctrl_c_streak = 0;
            view.ctrl_c_count = 0;

            if matches!(key.code, KeyCode::Tab) {
                view.status_view = match view.status_view {
                    crate::StatusView::Normal => crate::StatusView::Link,
                    crate::StatusView::Link => crate::StatusView::Normal,
                };
                let bytes = b"\t".to_vec();
                metrics.record_tx(bytes.len());
                ws_write.send(Message::Binary(bytes.into())).await?;
                crate::render::render(out, parser, view, metrics)?;
                return Ok(false);
            }

            if let Some(bytes) = key_to_bytes(key) {
                metrics.record_tx(bytes.len());
                ws_write.send(Message::Binary(bytes.into())).await?;
            }

            if had_ctrl_c_hint {
                crate::render::render(out, parser, view, metrics)?;
            }
            Ok(false)
        }
        FocusMode::Command => {
            *ctrl_c_streak = 0;
            view.ctrl_c_count = 0;

            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            {
                return Ok(true);
            }

            match key.code {
                KeyCode::Esc => {
                    view.focus = FocusMode::Input;
                    view.command_selection = CommandSelection::Mode;
                }
                KeyCode::Enter => match view.command_selection {
                    CommandSelection::Mode => {
                        view.focus = FocusMode::Input;
                    }
                    CommandSelection::Identity => {
                        let msg = serde_json::to_string(&ClientText::RequestControl)?;
                        metrics.record_tx(msg.len());
                        ws_write.send(Message::Text(msg.into())).await?;
                        view.focus = FocusMode::Input;
                    }
                },
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    view.command_selection = match view.command_selection {
                        CommandSelection::Mode => CommandSelection::Identity,
                        CommandSelection::Identity => CommandSelection::Mode,
                    };
                }
                _ => {}
            }

            crate::render::render(out, parser, view, metrics)?;
            Ok(false)
        }
    }
}
