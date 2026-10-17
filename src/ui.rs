use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    style::{Attribute, Color, Print, SetAttribute, SetForegroundColor},
    terminal::{self, ClearType},
};
use unicode_width::UnicodeWidthChar;

use crate::result::SearchResult;

/// Restores the terminal when dropped, including on early return or panic.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        queue!(io::stdout(), cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = queue!(io::stdout(), cursor::Show);
        let _ = io::stdout().flush();
        let _ = terminal::disable_raw_mode();
    }
}

/// Shows an inline list of results and returns the index the user picked,
/// or `None` if they cancelled.
pub fn select(results: &[SearchResult]) -> io::Result<Option<usize>> {
    let _guard = RawModeGuard::enable()?;
    let mut out = io::stdout();
    let mut selected = 0;
    let mut offset = 0;
    let mut drawn = 0;

    let choice = loop {
        let (cols, rows) = terminal::size()?;
        let visible = (usize::from(rows.saturating_sub(1)) / 2).clamp(1, results.len());
        if selected < offset {
            offset = selected;
        } else if selected >= offset + visible {
            offset = selected + 1 - visible;
        }

        clear(&mut out, drawn)?;
        drawn = draw(&mut out, &results[offset..offset + visible], selected - offset, cols)?;
        out.flush()?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match action(key) {
                Action::Up => selected = selected.saturating_sub(1),
                Action::Down => selected = (selected + 1).min(results.len() - 1),
                Action::Open => break Some(selected),
                Action::Quit => break None,
                Action::None => {}
            }
        }
    };

    clear(&mut out, drawn)?;
    out.flush()?;
    Ok(choice)
}

enum Action {
    Up,
    Down,
    Open,
    Quit,
    None,
}

fn action(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::Enter | KeyCode::Char(' ') => Action::Open,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Esc | KeyCode::Char('q') => Action::Quit,
        _ => Action::None,
    }
}

/// Moves back to the start of the previously drawn list and erases it.
fn clear(out: &mut impl Write, lines: u16) -> io::Result<()> {
    if lines > 0 {
        queue!(out, cursor::MoveToPreviousLine(lines))?;
    }
    queue!(out, cursor::MoveToColumn(0), terminal::Clear(ClearType::FromCursorDown))
}

/// Draws the visible results and returns the number of lines written.
fn draw(out: &mut impl Write, results: &[SearchResult], selected: usize, cols: u16) -> io::Result<u16> {
    let width = usize::from(cols).saturating_sub(3);
    for (i, r) in results.iter().enumerate() {
        let title = truncate(&r.title, width);
        let url = truncate(&r.url, width);
        if i == selected {
            queue!(
                out,
                SetForegroundColor(Color::Cyan),
                SetAttribute(Attribute::Bold),
                Print("> "),
                Print(title),
                SetAttribute(Attribute::Reset),
            )?;
        } else {
            queue!(out, Print("  "), Print(title))?;
        }
        queue!(
            out,
            Print("\r\n  "),
            SetAttribute(Attribute::Dim),
            Print(url),
            SetAttribute(Attribute::Reset),
            Print("\r\n"),
        )?;
    }
    Ok((results.len() * 2) as u16)
}

/// Cuts `s` to fit in `width` terminal columns so lines never wrap.
fn truncate(s: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            out.push('…');
            break;
        }
        used += w;
        out.push(c);
    }
    out
}
