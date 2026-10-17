use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    style::{Attribute, Color, Print, SetAttribute, SetForegroundColor},
    terminal::{self, ClearType},
};
use unicode_width::UnicodeWidthChar;

use crate::result::{SearchResult, Span};

/// Maximum wrapped text lines in the description box.
const DESC_LINES: usize = 3;
/// Columns taken by the box indent, borders and padding.
const BOX_CHROME: usize = 2 + 4;

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
        let inner = usize::from(cols).saturating_sub(BOX_CHROME).max(1);
        let popup = wrap(&results[selected].description, inner, DESC_LINES);
        let popup_height = if popup.is_empty() { 0 } else { popup.len() + 2 };
        let room = usize::from(rows.saturating_sub(1)).saturating_sub(popup_height);
        let visible = (room / 2).clamp(1, results.len());
        if selected < offset {
            offset = selected;
        } else if selected >= offset + visible {
            offset = selected + 1 - visible;
        }

        clear(&mut out, drawn)?;
        drawn = draw(&mut out, &results[offset..offset + visible], selected - offset, cols, &popup)?;
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
fn draw(
    out: &mut impl Write,
    results: &[SearchResult],
    selected: usize,
    cols: u16,
    popup: &[Vec<Span>],
) -> io::Result<u16> {
    let width = usize::from(cols).saturating_sub(3);
    let mut extra = 0;
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
        if i == selected && !popup.is_empty() {
            draw_popup(out, popup, usize::from(cols).saturating_sub(BOX_CHROME).max(1))?;
            extra = popup.len() + 2;
        }
    }
    Ok((results.len() * 2 + extra) as u16)
}

/// Draws the description box, indented to line up under the URL.
fn draw_popup(out: &mut impl Write, lines: &[Vec<Span>], inner: usize) -> io::Result<()> {
    let rule = "─".repeat(inner + 2);
    queue!(out, SetAttribute(Attribute::Dim), Print(format!("  ┌{rule}┐\r\n")), SetAttribute(Attribute::Reset))?;
    for line in lines {
        let used: usize = line.iter().map(|s| text_width(&s.text)).sum();
        queue!(out, SetAttribute(Attribute::Dim), Print("  │ "), SetAttribute(Attribute::Reset))?;
        for span in line {
            if span.bold {
                queue!(out, SetAttribute(Attribute::Bold), Print(&span.text), SetAttribute(Attribute::Reset))?;
            } else {
                queue!(out, Print(&span.text))?;
            }
        }
        queue!(
            out,
            Print(" ".repeat(inner - used)),
            SetAttribute(Attribute::Dim),
            Print(" │\r\n"),
            SetAttribute(Attribute::Reset),
        )?;
    }
    queue!(out, SetAttribute(Attribute::Dim), Print(format!("  └{rule}┘\r\n")), SetAttribute(Attribute::Reset))
}

fn text_width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Word-wraps styled text to `width` columns and at most `max_lines` lines,
/// ending the last line with `…` when text had to be dropped.
fn wrap(spans: &[Span], width: usize, max_lines: usize) -> Vec<Vec<Span>> {
    let chars: Vec<(char, bool)> = spans
        .iter()
        .flat_map(|s| s.text.chars().map(move |c| (c, s.bold)))
        .collect();
    let char_width = |c: char| c.width().unwrap_or(0);

    let mut lines: Vec<Vec<(char, bool)>> = Vec::new();
    let mut cur: Vec<(char, bool)> = Vec::new();
    let mut cur_w = 0;
    for word in chars.split(|&(c, _)| c == ' ').filter(|w| !w.is_empty()) {
        let word_w: usize = word.iter().map(|&(c, _)| char_width(c)).sum();
        if cur_w > 0 && cur_w + 1 + word_w <= width {
            cur.push((' ', false));
            cur_w += 1;
        } else if cur_w > 0 {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        for &(c, bold) in word {
            let w = char_width(c);
            if cur_w > 0 && cur_w + w > width {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            cur.push((c, bold));
            cur_w += w;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let mut w: usize = last.iter().map(|&(c, _)| char_width(c)).sum();
            while w + 1 > width {
                match last.pop() {
                    Some((c, _)) => w -= char_width(c),
                    None => break,
                }
            }
            last.push(('…', false));
        }
    }

    lines
        .into_iter()
        .map(|line| {
            let mut spans: Vec<Span> = Vec::new();
            for (c, bold) in line {
                match spans.last_mut() {
                    Some(last) if last.bold == bold => last.text.push(c),
                    _ => spans.push(Span { text: c.to_string(), bold }),
                }
            }
            spans
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, bold: bool) -> Span {
        Span { text: text.into(), bold }
    }

    fn flat(line: &[Span]) -> String {
        line.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn wraps_within_width() {
        let lines = wrap(&[span("aaa bbb ccc ddd", false)], 7, 5);
        assert_eq!(lines.iter().map(|l| flat(l)).collect::<Vec<_>>(), ["aaa bbb", "ccc ddd"]);
    }

    #[test]
    fn truncates_with_ellipsis() {
        let lines = wrap(&[span("aaa bbb ccc ddd", false)], 7, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(flat(&lines[0]), "aaa bb…");
    }

    #[test]
    fn keeps_bold_across_breaks() {
        let lines = wrap(&[span("one ", false), span("two three", true)], 7, 3);
        assert_eq!(lines[0], vec![span("one ", false), span("two", true)]);
        assert_eq!(lines[1], vec![span("three", true)]);
    }

    #[test]
    fn hard_breaks_long_words() {
        let lines = wrap(&[span("abcdefghij", false)], 4, 5);
        assert_eq!(lines.iter().map(|l| flat(l)).collect::<Vec<_>>(), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn empty_description_has_no_lines() {
        assert!(wrap(&[], 10, 3).is_empty());
    }
}
