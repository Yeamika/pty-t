use super::{CommandSelection, FocusMode, Metrics, StatusView, ViewState};
use anyhow::Result;
use crossterm::cursor;
use crossterm::queue;
use crossterm::style::{Attribute, SetAttribute};
use crossterm::terminal::{self, ClearType};
use std::io::{Stdout, Write};
use std::time::Duration;

pub fn render(
    out: &mut Stdout,
    parser: &vt100::Parser,
    view: &ViewState,
    metrics: &Metrics,
) -> Result<()> {
    let content_rows = view.local_rows.saturating_sub(1);
    let draw_cols = view.local_cols.min(view.pty_cols);

    queue!(out, cursor::Hide, SetAttribute(Attribute::Reset))?;
    let rows = parser
        .screen()
        .rows_formatted(0, draw_cols)
        .take(content_rows as usize)
        .collect::<Vec<_>>();

    for y in 0..content_rows {
        queue!(
            out,
            cursor::MoveTo(0, y),
            SetAttribute(Attribute::Reset),
            terminal::Clear(ClearType::CurrentLine)
        )?;
        if let Some(row) = rows.get(y as usize) {
            out.write_all(row)?;
        }
    }

    let status_cursor = draw_status(out, view, metrics)?;

    if let Some((cur_col, cur_row)) = status_cursor {
        queue!(out, cursor::MoveTo(cur_col, cur_row), cursor::Show)?;
    } else {
        let (cur_row, cur_col) = parser.screen().cursor_position();
        if cur_row < content_rows && cur_col < view.local_cols {
            queue!(out, cursor::MoveTo(cur_col, cur_row), cursor::Show)?;
        } else {
            queue!(
                out,
                cursor::MoveTo(0, view.local_rows.saturating_sub(1)),
                cursor::Hide
            )?;
        }
    }

    out.flush()?;
    Ok(())
}

pub fn draw_message(out: &mut Stdout, message: &str, view: &ViewState) -> Result<()> {
    queue!(
        out,
        cursor::MoveTo(0, view.local_rows.saturating_sub(1)),
        SetAttribute(Attribute::Reverse),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    let text = trim_to_chars(message, view.local_cols as usize);
    write!(out, "{text:<width$}", width = view.local_cols as usize)?;
    queue!(out, SetAttribute(Attribute::Reset))?;
    out.flush()?;
    Ok(())
}

fn draw_status(
    out: &mut Stdout,
    view: &ViewState,
    metrics: &Metrics,
) -> Result<Option<(u16, u16)>> {
    queue!(
        out,
        cursor::MoveTo(0, view.local_rows.saturating_sub(1)),
        SetAttribute(Attribute::Reset),
        terminal::Clear(ClearType::CurrentLine)
    )?;

    let cursor_target = match view.focus {
        FocusMode::Input if view.ctrl_c_count > 0 => {
            write_status_text(
                out,
                view,
                &format!("[ctrl c x{}] x3 to switch Mode", view.ctrl_c_count),
            )?;
            None
        }
        FocusMode::Input => match view.status_view {
            StatusView::Normal => {
                write_status_text(
                    out,
                    view,
                    &format!(
                        "[INPUT] [{}:{}] [{}x{}] pty={}",
                        view.role, view.id, view.pty_cols, view.pty_rows, view.pty
                    ),
                )?;
                None
            }
            StatusView::Link => {
                write_status_text(
                    out,
                    view,
                    &format!(
                        "[LINK] [{}:{}] rtt={} rx={} tx={} idle={} pty={}",
                        view.role,
                        view.id,
                        metrics.latency_text(),
                        format_bytes(metrics.rx_bytes),
                        format_bytes(metrics.tx_bytes),
                        metrics.idle_text(),
                        view.pty
                    ),
                )?;
                None
            }
        },
        FocusMode::Command => Some(draw_command_status(out, view)?),
    };

    queue!(out, SetAttribute(Attribute::Reset))?;
    Ok(cursor_target)
}

fn write_status_text(out: &mut Stdout, view: &ViewState, text: &str) -> Result<()> {
    let text = trim_to_chars(text, view.local_cols as usize);
    write!(out, "{text:<width$}", width = view.local_cols as usize)?;
    Ok(())
}

fn draw_command_status(out: &mut Stdout, view: &ViewState) -> Result<(u16, u16)> {
    let command_x = 0;
    write_status_segment(
        out,
        "[Command]",
        view.command_selection == CommandSelection::Mode,
    )?;
    write!(out, " ")?;
    write_status_segment(
        out,
        &format!("[{}:{}]", view.role, view.id),
        view.command_selection == CommandSelection::Identity,
    )?;
    write!(
        out,
        " [{}x{}] pty={}",
        view.pty_cols, view.pty_rows, view.pty
    )?;
    let cursor_x = match view.command_selection {
        CommandSelection::Mode => command_x,
        CommandSelection::Identity => command_x + "[Command] ".len() as u16,
    };
    Ok((cursor_x, view.local_rows.saturating_sub(1)))
}

fn write_status_segment(out: &mut Stdout, text: &str, selected: bool) -> Result<()> {
    if selected {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    } else {
        queue!(out, SetAttribute(Attribute::Reset))?;
    }
    write!(out, "{text}")?;
    queue!(out, SetAttribute(Attribute::Reset))?;
    Ok(())
}

fn trim_to_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{}{}", bytes, UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}
