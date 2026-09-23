// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Painting one frame: header, hairline rules, the centered lyric, and the status bar.
//!
//! The layout is the same five-row shape the sibling TUIs (`rewind`, `vademecum`) use: a
//! 1-row header, a hairline rule, the content, another hairline rule, and a 1-row status bar.
//! What's different here is the content row: instead of a scrolling list, the current lyric
//! line's vertical middle is pinned to the content area's center row every frame, with earlier
//! lines stacked above it and later lines below — a teleprompter, not a pager.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Widget, Wrap};
use ratatui::{Frame, symbols};
use unicode_width::UnicodeWidthStr;

use crate::theme::Element;
use crate::tui::app::{App, Mode};
use crate::tui::{bigtext, help};

const HINTS: &str = "? keys";
/// The fine-sync keys, kept on the status bar (not only behind `?`) because lining the clock
/// up is the thing a listener does most while a song plays.
const SYNC_HINTS: &str = ", . ±0.1s · < > ±0.5s · Enter sync";
const HINT_GAP: usize = 2;
const EDGE_PAD: u16 = 1;
const SEPARATOR: &str = " · ";
const ELIDED: &str = "…";
/// Padding, in cells, between the lyric text and either edge of the content area.
const CONTENT_PAD: u16 = 2;
const BREAK_GLYPH: &str = "♪";
/// How many rows past the screen's edge to keep generating context lines for — cheap
/// insurance against a resize between deadline math and drawing.
const SLACK_ROWS: usize = 4;

pub fn draw(frame: &mut Frame, app: &App, elapsed: Duration) {
    frame.render_widget(Screen { app, elapsed }, frame.area());
}

struct Screen<'a> {
    app: &'a App,
    elapsed: Duration,
}

impl Widget for Screen<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ];
        let [header_row, top_rule, content_rows, bottom_rule, status_row] =
            Layout::vertical(rows).areas(area);

        // Computed once and threaded through: `content()` and `statusbar()` both need "which
        // line is current right now", and it's the same answer for both on a given frame.
        let current_index = self.app.current_index(self.elapsed);
        let break_remaining = self.app.break_remaining(self.elapsed);

        header(header_row, buf, self.app);
        rule(top_rule, buf, self.app.theme.style(Element::Hint));
        progress(top_rule, buf, self.app, self.elapsed);
        content(content_rows, buf, self.app, current_index, break_remaining);
        rule(bottom_rule, buf, self.app.theme.style(Element::Hint));
        statusbar(status_row, buf, self.app, self.elapsed, current_index);

        if self.app.mode == Mode::Help {
            help_overlay(content_rows, buf, self.app);
        }
    }
}

fn row_at(area: Rect, buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return;
    }
    buf.set_stringn(
        x,
        y,
        text,
        usize::from(area.right().saturating_sub(x)),
        style,
    );
}

/// `row_at` at `area.y`, for the single-row areas (header, rules, status bar).
fn row(area: Rect, buf: &mut Buffer, x: u16, text: &str, style: Style) {
    row_at(area, buf, x, area.y, text, style);
}

fn padded(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(EDGE_PAD.min(area.width)),
        width: area.width.saturating_sub(EDGE_PAD.saturating_mul(2)),
        ..area
    }
}

fn header(area: Rect, buf: &mut Buffer, app: &App) {
    let area = padded(area);
    let title_style = app.theme.style(Element::HeaderTitle);
    let quiet = app.theme.style(Element::Status);

    let mut segments = vec![String::from("lyrics")];
    if let Some(artist) = &app.song.artist {
        segments.push(artist.clone());
    }
    segments.push(app.song.title.clone());

    let hints_width = HINTS.width();
    let room = usize::from(area.width)
        .saturating_sub(HINT_GAP)
        .saturating_sub(hints_width);
    let shown = elide(&segments, room);

    let mut x = area.x;
    for (index, segment) in shown.iter().enumerate() {
        if index > 0 {
            row(area, buf, x, SEPARATOR, quiet);
            x = x.saturating_add(u16::try_from(SEPARATOR.width()).unwrap_or(area.width));
        }
        let style = if index == 0 && segment == "lyrics" {
            title_style
        } else {
            quiet
        };
        row(area, buf, x, segment, style);
        x = x.saturating_add(u16::try_from(segment.width()).unwrap_or(area.width));
    }

    let full_width = shown
        .iter()
        .map(|s: &String| s.width())
        .fold(0usize, usize::saturating_add)
        .saturating_add(
            SEPARATOR
                .width()
                .saturating_mul(shown.len().saturating_sub(1)),
        );
    if full_width
        .saturating_add(HINT_GAP)
        .saturating_add(hints_width)
        <= usize::from(area.width)
    {
        let hint_x = area
            .right()
            .saturating_sub(u16::try_from(hints_width).unwrap_or(area.width));
        row(area, buf, hint_x, HINTS, app.theme.style(Element::Hint));
    }
}

/// Drop segments from the front (replacing them with `…`) until what remains fits in `room`
/// columns. Falls back to truncating the last segment alone if even that doesn't fit.
fn elide(segments: &[String], room: usize) -> Vec<String> {
    let width_of = |pieces: &[String]| {
        pieces
            .iter()
            .map(|s: &String| s.width())
            .fold(0usize, usize::saturating_add)
            .saturating_add(
                SEPARATOR
                    .width()
                    .saturating_mul(pieces.len().saturating_sub(1)),
            )
    };
    if width_of(segments) <= room {
        return segments.to_vec();
    }
    for from in 1..segments.len() {
        let candidate: Vec<String> = std::iter::once(ELIDED.to_owned())
            .chain(segments.get(from..).unwrap_or_default().iter().cloned())
            .collect();
        if width_of(&candidate) <= room {
            return candidate;
        }
    }
    segments
        .last()
        .map_or_else(Vec::new, |last| vec![truncate(last, room)])
}

/// Truncate `text` to `width` columns, replacing the tail with `…` if it was cut. Character-
/// based (never byte slicing) so it can't land inside a multi-byte codepoint.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let budget = width.saturating_sub(ELIDED.width());
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = ch.to_string().width();
        if used.saturating_add(w) > budget {
            break;
        }
        out.push(ch);
        used = used.saturating_add(w);
    }
    out.push_str(ELIDED);
    out
}

fn rule(area: Rect, buf: &mut Buffer, style: Style) {
    row(
        area,
        buf,
        area.x,
        &symbols::line::HORIZONTAL.repeat(usize::from(area.width)),
        style,
    );
}

/// Paints the leading portion of the top rule in `ScrollProgress`, proportional to how far
/// into the song the clock is. The denominator is the last line's timestamp plus a few
/// seconds' grace, so the bar reaches full only once the last line has had time to be read.
fn progress(area: Rect, buf: &mut Buffer, app: &App, elapsed: Duration) {
    if area.height == 0 || app.song.lines.is_empty() {
        return;
    }
    let Some(last) = app.song.lines.last() else {
        return;
    };
    let denominator = last.at_ms.saturating_add(5_000).max(1);
    let position = u64::try_from(elapsed.as_millis())
        .unwrap_or(u64::MAX)
        .min(denominator);
    let filled = u16::try_from(
        position
            .saturating_mul(u64::from(area.width))
            .checked_div(denominator)
            .unwrap_or(0),
    )
    .unwrap_or(u16::MAX)
    .min(area.width);
    let style = app.theme.style(Element::ScrollProgress);
    for x in area.x..area.x.saturating_add(filled) {
        if let Some(cell) = buf.cell_mut((x, area.y)) {
            cell.set_style(style);
        }
    }
}

/// Greedy word-wrap of `text` into rows no wider than `width` columns. Never splits a word
/// unless the word alone is wider than `width`, in which case it's cut a character at a time
/// (never byte-sliced) so it can't land inside a multi-byte codepoint. `width == 0` returns the
/// text as a single unwrapped row rather than looping forever.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if width == 0 {
        return vec![text.to_owned()];
    }

    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = word.width();
        let extra = usize::from(!current.is_empty());
        if current_width
            .saturating_add(extra)
            .saturating_add(word_width)
            <= width
        {
            if !current.is_empty() {
                current.push(' ');
                current_width = current_width.saturating_add(1);
            }
            current.push_str(word);
            current_width = current_width.saturating_add(word_width);
            continue;
        }
        if !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width <= width {
            current.push_str(word);
            current_width = word_width;
        } else {
            // A single word wider than the whole line: hard-wrap it by character.
            for ch in word.chars() {
                let ch_width = ch.to_string().width();
                if current_width.saturating_add(ch_width) > width && !current.is_empty() {
                    rows.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(ch);
                current_width = current_width.saturating_add(ch_width);
            }
        }
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Distance {
    Current,
    Near,
    Far,
}

fn style_for(theme: &crate::theme::Theme, distance: Distance) -> Style {
    match distance {
        Distance::Current => theme.style(Element::CurrentLine),
        Distance::Near => theme.style(Element::NearLine),
        Distance::Far => theme.style(Element::FarLine),
    }
}

/// Text to show for line `index`: `♪` for a break entry (blank timed line), else its lyric.
fn display_text(app: &App, index: usize) -> String {
    app.song
        .lines
        .get(index)
        .map(|line| {
            if line.text.trim().is_empty() {
                BREAK_GLYPH.to_owned()
            } else {
                line.text.clone()
            }
        })
        .unwrap_or_default()
}

fn content(
    area: Rect,
    buf: &mut Buffer,
    app: &App,
    current_index: Option<usize>,
    break_remaining: Option<Duration>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    if let Mode::Countdown { step } = app.mode {
        countdown(area, buf, app, step);
        return;
    }

    let width = usize::from(area.width.saturating_sub(CONTENT_PAD.saturating_mul(2)));
    let half = usize::from(area.height).saturating_div(2);

    // In a long enough break the timer rides on the `♪` row itself: the current row is where
    // the eye already is, and a bare `0:12` next to the note needs no words to read as "until
    // the singing resumes".
    let current_wrapped = match (break_remaining, current_index) {
        (Some(left), _) => vec![format!("{BREAK_GLYPH} {}", format_remaining(left))],
        (None, None) => vec![BREAK_GLYPH.to_owned()],
        (None, Some(index)) => wrap(&display_text(app, index), width),
    };
    let current_mid = current_wrapped.len().saturating_sub(1).saturating_div(2);

    // Above: walk earlier lines until enough rows are built to cover from the center row up
    // to (and a little past) the top of the content area.
    let mut above: Vec<(String, Distance)> = Vec::new();
    let mut rows_above = current_mid;
    let mut cursor = current_index;
    let mut steps_up = 0usize;
    // Before the first line (`cursor` is `None`) nothing has been sung yet, so nothing goes
    // above the `♪`: line 0 is the first line *below* it, and drawing it here too would show it
    // twice.
    while rows_above < half.saturating_add(SLACK_ROWS) {
        let Some(index) = cursor.and_then(|i| i.checked_sub(1)) else {
            break;
        };
        cursor = Some(index);
        steps_up = steps_up.saturating_add(1);
        let distance = if steps_up <= 2 {
            Distance::Near
        } else {
            Distance::Far
        };
        let wrapped = wrap(&display_text(app, index), width);
        rows_above = rows_above.saturating_add(wrapped.len());
        for row_text in wrapped.into_iter().rev() {
            above.push((row_text, distance));
        }
    }
    above.reverse();

    let mut below: Vec<(String, Distance)> = Vec::new();
    let mut rows_below = current_wrapped
        .len()
        .saturating_sub(current_mid)
        .saturating_sub(1);
    let mut next_index = current_index.map_or(0, |i| i.saturating_add(1));
    let mut steps_down = 0usize;
    while rows_below < half.saturating_add(SLACK_ROWS) {
        if next_index >= app.song.lines.len() {
            break;
        }
        steps_down = steps_down.saturating_add(1);
        let distance = if steps_down <= 2 {
            Distance::Near
        } else {
            Distance::Far
        };
        let wrapped = wrap(&display_text(app, next_index), width);
        rows_below = rows_below.saturating_add(wrapped.len());
        for row_text in wrapped {
            below.push((row_text, distance));
        }
        next_index = next_index.saturating_add(1);
    }

    let above_len = u16::try_from(above.len()).unwrap_or(u16::MAX);
    let center_row = area
        .y
        .saturating_add(u16::try_from(half).unwrap_or(area.height));
    let current_mid_u16 = u16::try_from(current_mid).unwrap_or(0);
    let start_y = center_row
        .saturating_sub(above_len)
        .saturating_sub(current_mid_u16);

    let mut y = start_y;
    for (text, distance) in &above {
        draw_centered_row(area, buf, y, text, style_for(&app.theme, *distance));
        y = y.saturating_add(1);
    }
    for text in &current_wrapped {
        draw_centered_row(area, buf, y, text, style_for(&app.theme, Distance::Current));
        y = y.saturating_add(1);
    }
    for (text, distance) in &below {
        draw_centered_row(area, buf, y, text, style_for(&app.theme, *distance));
        y = y.saturating_add(1);
    }
}

fn draw_centered_row(area: Rect, buf: &mut Buffer, y: u16, text: &str, style: Style) {
    if y < area.y || y >= area.bottom() {
        return;
    }
    let row_area = Rect {
        y,
        height: 1,
        ..area
    };
    Line::styled(text, style).centered().render(row_area, buf);
}

fn countdown(area: Rect, buf: &mut Buffer, app: &App, step: u8) {
    let label = match step {
        0 => "3",
        1 => "2",
        2 => "1",
        _ => "PLAY",
    };
    let style = app.theme.style(Element::Countdown);
    if let Some(rows) = bigtext::render(label) {
        let block_height = u16::try_from(rows.len()).unwrap_or(0);
        let block_width = rows.first().map_or(0, |r| r.width());
        if block_height <= area.height
            && u16::try_from(block_width).unwrap_or(u16::MAX) <= area.width
        {
            let top = area
                .y
                .saturating_add(area.height.saturating_sub(block_height).saturating_div(2));
            for (offset, line) in rows.iter().enumerate() {
                let y = top.saturating_add(u16::try_from(offset).unwrap_or(0));
                draw_centered_row(area, buf, y, line, style);
            }
            return;
        }
    }
    // The terminal is too small for the block font (or the label had no glyphs): fall back to
    // plain centered text.
    let y = area.y.saturating_add(area.height.saturating_div(2));
    draw_centered_row(area, buf, y, label, style);
}

fn statusbar(
    area: Rect,
    buf: &mut Buffer,
    app: &App,
    elapsed: Duration,
    current_index: Option<usize>,
) {
    let area = padded(area);
    let text = if let Some(notice) = app.notice(std::time::Instant::now()) {
        row(
            area,
            buf,
            area.x,
            notice,
            app.theme.style(Element::StatusNotice),
        );
        notice.to_owned()
    } else {
        let total_lines = app.song.lines.len();
        let position = current_index.map_or(0, |index| index.saturating_add(1));
        let playing = matches!(app.mode, Mode::Playing) && app.clock.is_playing();
        let symbol = if playing { "▶" } else { "❚❚" };
        let clock_text = format_clock(elapsed);
        let text = if matches!(app.mode, Mode::Countdown { .. }) {
            String::from("counting down…")
        } else {
            format!("{symbol} {clock_text}{SEPARATOR}line {position}/{total_lines}")
        };
        row(area, buf, area.x, &text, app.theme.style(Element::Status));
        text
    };

    // Dropped whole rather than truncated when the terminal is too narrow, like `HINTS` in
    // the header: half a hint is worse than none.
    let hints_width = SYNC_HINTS.width();
    if text
        .width()
        .saturating_add(HINT_GAP)
        .saturating_add(hints_width)
        <= usize::from(area.width)
    {
        let hint_x = area
            .right()
            .saturating_sub(u16::try_from(hints_width).unwrap_or(area.width));
        row(
            area,
            buf,
            hint_x,
            SYNC_HINTS,
            app.theme.style(Element::Hint),
        );
    }
}

/// `0:12`, rounded *up*: the counter reads `0:01` through the last second and the line
/// arrives as it would tick to `0:00`, the way a countdown to an event should.
fn format_remaining(left: Duration) -> String {
    let millis = left.as_millis();
    let secs = millis.saturating_add(999).saturating_div(1_000);
    format!(
        "{}:{:02}",
        secs.saturating_div(60),
        secs.checked_rem(60).unwrap_or(0)
    )
}

fn format_clock(elapsed: Duration) -> String {
    let total_secs = elapsed.as_secs();
    let minutes = total_secs.saturating_div(60);
    let seconds = total_secs.checked_rem(60).unwrap_or(0);
    format!("{minutes:02}:{seconds:02}")
}

fn help_overlay(area: Rect, buf: &mut Buffer, app: &App) {
    let size = ratatui::layout::Size::new(area.width, area.height);
    let outer = help::outer(size);
    if outer.width == 0 || outer.height == 0 {
        return;
    }
    let x = area
        .x
        .saturating_add(area.width.saturating_sub(outer.width).saturating_div(2));
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(outer.height).saturating_div(2));
    let overlay_area = Rect::new(x, y, outer.width, outer.height);

    Clear.render(overlay_area, buf);
    let block = Block::bordered()
        .title(help::TITLE)
        .style(app.theme.style(Element::HelpWindow))
        .border_style(app.theme.style(Element::HeaderTitle));
    let inner = block.inner(overlay_area);
    block.render(overlay_area, buf);

    let lines = help::lines(&app.theme);
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(inner, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lrc::SyncedLine;
    use crate::theme::Theme;
    use crate::tui::app::Song;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn song() -> Song {
        Song {
            title: String::from("One"),
            artist: Some(String::from("Metallica")),
            lines: vec![
                SyncedLine {
                    at_ms: 0,
                    text: String::from("Line zero"),
                },
                SyncedLine {
                    at_ms: 1_000,
                    text: String::from("Line one"),
                },
                SyncedLine {
                    at_ms: 2_000,
                    text: String::from("Line two, the current one"),
                },
                SyncedLine {
                    at_ms: 3_000,
                    text: String::from("Line three"),
                },
                SyncedLine {
                    at_ms: 4_000,
                    text: String::from("Line four"),
                },
                SyncedLine {
                    at_ms: 5_000,
                    text: String::from("Line five"),
                },
            ],
        }
    }

    fn app() -> App {
        App::new(song(), Theme::default(), false)
    }

    fn row_text(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| {
                buffer
                    .cell((x, y))
                    .map_or(" ", ratatui::buffer::Cell::symbol)
            })
            .collect()
    }

    #[test]
    fn the_current_line_lands_on_the_content_areas_center_row() {
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::from_millis(2_000)))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();

        // Content rows are y=2..=8 (header, rule, content x7, rule, status) for a height-11
        // screen: 1+1+7+1+1 = 11. The content area's center row is y=2+3=5.
        let expected_center = 5u16;
        let mut found_at = None;
        for y in 2..9 {
            if row_text(&buffer, y).contains("Line two") {
                found_at = Some(y);
            }
        }
        assert_eq!(
            found_at,
            Some(expected_center),
            "the current line was not on the center row"
        );
    }

    #[test]
    fn the_current_line_is_horizontally_centered() {
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::from_millis(2_000)))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();

        let text = "Line two, the current one";
        let start = row_text(&buffer, 5).find(text).unwrap();
        let expected_pad = (usize::from(buffer.area.width) - text.width()) / 2;
        assert!(
            start.abs_diff(expected_pad) <= 1,
            "start={start} expected~{expected_pad}"
        );
    }

    #[test]
    fn a_narrow_screen_wraps_the_current_line_and_still_centers_its_middle_row() {
        let backend = TestBackend::new(15, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::from_millis(2_000)))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut hit_rows = Vec::new();
        for y in 2..13 {
            let text = row_text(&buffer, y);
            if text.contains("current") || text.contains("Line two") {
                hit_rows.push(y);
            }
        }
        assert!(
            !hit_rows.is_empty(),
            "the wrapped current line did not render at all"
        );
    }

    #[test]
    fn the_countdown_renders_something_in_the_content_area() {
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.mode = Mode::Countdown { step: 3 };
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let non_blank = buffer.content.iter().any(|cell| cell.symbol() == "#");
        assert!(non_blank, "the countdown drew nothing");
    }

    #[test]
    fn the_header_elides_from_the_front_when_it_does_not_fit() {
        let mut app = app();
        app.song.title = "A".repeat(200);
        let backend = TestBackend::new(30, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let header = row_text(&buffer, 0);
        assert!(header.contains(ELIDED), "{header:?}");
    }

    #[test]
    fn wrap_never_splits_a_word_that_fits() {
        let rows = wrap("hello world", 20);
        assert_eq!(rows, vec!["hello world"]);
        let rows = wrap("hello world", 8);
        assert_eq!(rows, vec!["hello", "world"]);
    }

    #[test]
    fn wrap_of_zero_width_returns_the_text_unwrapped_rather_than_looping() {
        assert_eq!(wrap("hi", 0), vec!["hi"]);
    }

    #[test]
    fn the_status_bar_shows_the_sync_keys_when_there_is_room() {
        let backend = TestBackend::new(80, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
        let status = row_text(terminal.backend().buffer(), 10);
        assert!(status.contains("line 1/"), "{status:?}");
        assert!(status.trim_end().ends_with(SYNC_HINTS), "{status:?}");
    }

    #[test]
    fn the_status_bar_drops_the_sync_keys_whole_on_a_narrow_screen() {
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
        let status = row_text(terminal.backend().buffer(), 10);
        assert!(status.contains("line 1/"), "{status:?}");
        assert!(!status.contains("±"), "{status:?}");
    }

    #[test]
    fn an_intro_shows_the_countdown_beside_the_break_glyph() {
        let mut song = song();
        for line in &mut song.lines {
            line.at_ms = line.at_ms.saturating_add(12_000);
        }
        let app = App::new(song, Theme::default(), false);
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, Duration::from_millis(1_500)))
            .unwrap();
        let buffer = terminal.backend().buffer();

        // Center row of the content area is y=5 (see the centering test above).
        assert!(
            row_text(buffer, 5).contains(&format!("{BREAK_GLYPH} 0:11")),
            "{:?}",
            row_text(buffer, 5)
        );
        assert!(row_text(buffer, 6).contains("Line zero"));
    }

    #[test]
    fn before_the_first_line_nothing_is_drawn_above_the_break_glyph() {
        let mut song = song();
        for line in &mut song.lines {
            line.at_ms = line.at_ms.saturating_add(12_000);
        }
        let app = App::new(song, Theme::default(), false);
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..11).map(|y| row_text(buffer, y)).collect();
        assert_eq!(
            rows.iter().filter(|row| row.contains("Line zero")).count(),
            1,
            "{rows:#?}"
        );
        assert!((2..5).all(|y| rows[y].trim().is_empty()), "{rows:#?}");
    }

    #[test]
    fn a_lyric_line_shows_no_countdown() {
        let backend = TestBackend::new(40, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, Duration::from_millis(2_000)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!((0..11).all(|y| !row_text(buffer, y).contains(BREAK_GLYPH)));
    }

    #[test]
    fn format_remaining_rounds_up_to_the_second() {
        assert_eq!(format_remaining(Duration::from_millis(10_500)), "0:11");
        assert_eq!(format_remaining(Duration::from_millis(1)), "0:01");
        assert_eq!(format_remaining(Duration::from_secs(75)), "1:15");
    }

    #[test]
    fn format_clock_pads_minutes_and_seconds() {
        assert_eq!(format_clock(Duration::from_secs(65)), "01:05");
        assert_eq!(format_clock(Duration::ZERO), "00:00");
    }

    #[test]
    fn help_overlay_does_not_panic_on_a_tiny_screen() {
        let backend = TestBackend::new(3, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.mode = Mode::Help;
        terminal
            .draw(|frame| draw(frame, &app, Duration::ZERO))
            .unwrap();
    }
}
