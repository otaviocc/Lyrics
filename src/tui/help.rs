// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The `?` overlay: every key, grouped by purpose.

use ratatui::layout::Size;
use ratatui::text::{Line, Span};

use crate::theme::{Element, Theme};
use crate::tui::clamp_fraction;

pub const TITLE: &str = " Keys ";
const WIDTH_FRACTION: u16 = 60;
const MIN_WIDTH: u16 = 40;
const MAX_WIDTH: u16 = 64;
const HEIGHT_FRACTION: u16 = 85;
const MIN_HEIGHT: u16 = 10;
const MAX_HEIGHT: u16 = 30;
const KEYS_COLUMN: usize = 18;
const BORDER_ROWS: u16 = 2;

pub const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Playback",
        &[
            ("Space", "play · pause"),
            ("c", "replay the countdown"),
            ("0 r", "restart at 00:00, paused"),
        ],
    ),
    (
        "Leaving",
        &[
            ("? Esc q", "Esc or q closes this window"),
            ("Ctrl-c", "quit"),
        ],
    ),
    (
        "Library",
        &[
            ("Tab", "open · close the list"),
            ("Up k Down j", "move the cursor"),
            ("Enter", "follow this song"),
            ("/", "filter the list by typing"),
            ("Backspace", "erase · Esc clears it"),
        ],
    ),
    (
        "Seeking",
        &[
            ("Left h", "back 5s"),
            ("Right l", "forward 5s"),
            ("Shift-Left H", "back 10s"),
            ("Shift-Right L", "forward 10s"),
            ("Up k", "jump to the previous line"),
            ("Down j", "jump to the next line"),
            (", .", "nudge -0.1s · +0.1s, for fine sync"),
            ("< >", "nudge -0.5s · +0.5s"),
            ("Enter", "start this line (paused) · snap (playing)"),
        ],
    ),
];

#[must_use]
pub fn outer(area: Size, lines: &[Line<'_>]) -> Size {
    let width = clamp_fraction(area.width, WIDTH_FRACTION, MIN_WIDTH, MAX_WIDTH);
    let ceiling = clamp_fraction(area.height, HEIGHT_FRACTION, MIN_HEIGHT, MAX_HEIGHT);
    let inner = usize::from(width.saturating_sub(BORDER_ROWS)).max(1);
    let rows = lines
        .iter()
        .map(|line| {
            line.width()
                .saturating_add(inner)
                .saturating_sub(1)
                .checked_div(inner)
                .unwrap_or(1)
                .max(1)
        })
        .fold(0usize, usize::saturating_add);
    let wanted = u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(BORDER_ROWS);
    Size::new(width, wanted.max(MIN_HEIGHT).min(ceiling))
}

#[must_use]
pub fn lines(theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, (heading, keys)) in SECTIONS.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            (*heading).to_owned(),
            theme.style(Element::HeaderTitle),
        )));
        for (keys, meaning) in *keys {
            let padded = format!("{keys:<KEYS_COLUMN$}");
            lines.push(Line::from(vec![
                Span::styled(padded, theme.style(Element::Label)),
                Span::styled((*meaning).to_owned(), theme.style(Element::Body)),
            ]));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::Mode;
    use crate::tui::input;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn the_overlay_never_outgrows_the_area_it_floats_over() {
        for width in 1..200u16 {
            for height in 1..40u16 {
                let area = Size::new(width, height);
                let table = lines(&Theme::default());
                for rows in [&table[..], &[][..]] {
                    assert!(outer(area, rows).width <= width, "{area:?}");
                    assert!(outer(area, rows).height <= height, "{area:?}");
                }
            }
        }
    }

    #[test]
    fn the_box_counts_wrapped_rows_not_just_table_entries() {
        let table = lines(&Theme::default());
        let roomy = outer(Size::new(200, 200), &table);
        let narrow = outer(Size::new(MIN_WIDTH, 200), &table);

        assert!(
            roomy.height <= MAX_HEIGHT,
            "the table no longer fits MAX_HEIGHT at its widest"
        );
        assert!(
            narrow.height > roomy.height,
            "a narrower box wraps more rows and must be taller"
        );
    }

    #[test]
    fn every_key_it_advertises_is_actually_bound() {
        for (heading, keys) in SECTIONS {
            let modes: &[Mode] = if *heading == "Library" {
                &[Mode::Picker, Mode::Filter]
            } else {
                &[Mode::Playing]
            };
            for (keys, _) in *keys {
                for token in keys.split(' ') {
                    let event = to_event(token);
                    assert!(
                        modes
                            .iter()
                            .any(|mode| input::action(&event, *mode).is_some()),
                        "{token} in the help table does not resolve"
                    );
                }
            }
        }
    }

    fn to_event(token: &str) -> KeyEvent {
        match token {
            "Space" => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            "Backspace" => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            "Esc" => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            "Tab" => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            "Enter" => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            "Left" => KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            "Right" => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            "Up" => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            "Down" => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            "Shift-Left" => KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
            "Shift-Right" => KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
            "Ctrl-c" => KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            other if other.chars().count() == 1 => KeyEvent::new(
                KeyCode::Char(other.chars().next().unwrap()),
                KeyModifiers::NONE,
            ),
            other => panic!("no event mapping for token {other:?}"),
        }
    }

    #[test]
    fn sections_are_separated_by_a_blank_line() {
        assert!(
            lines(&Theme::default())
                .iter()
                .map(Line::to_string)
                .any(|line| line.is_empty())
        );
    }
}
