// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The `?` overlay: every key, grouped by purpose.

use ratatui::layout::Size;
use ratatui::text::{Line, Span};

use crate::theme::{Element, Theme};

pub const TITLE: &str = " Keys ";
const WIDTH_FRACTION: u16 = 60;
const MIN_WIDTH: u16 = 40;
const MAX_WIDTH: u16 = 64;
const HEIGHT_FRACTION: u16 = 70;
const MIN_HEIGHT: u16 = 10;
const MAX_HEIGHT: u16 = 22;
const KEYS_COLUMN: usize = 18;

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
            ("Enter", "snap to the nearest line, tap as it's sung"),
        ],
    ),
    (
        "Leaving",
        &[
            ("? Esc q", "these keys · Esc or q closes this window"),
            ("Ctrl-c", "quit"),
        ],
    ),
];

#[must_use]
pub fn outer(area: Size) -> Size {
    let width = area
        .width
        .saturating_mul(WIDTH_FRACTION)
        .saturating_div(100)
        .clamp(MIN_WIDTH, MAX_WIDTH)
        .min(area.width);
    let height = area
        .height
        .saturating_mul(HEIGHT_FRACTION)
        .saturating_div(100)
        .clamp(MIN_HEIGHT, MAX_HEIGHT)
        .min(area.height);
    Size::new(width, height)
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
                assert!(outer(area).width <= width, "{area:?}");
                assert!(outer(area).height <= height, "{area:?}");
            }
        }
    }

    #[test]
    fn a_tall_enough_screen_shows_every_row_without_clipping() {
        let rows = u16::try_from(lines(&Theme::default()).len()).unwrap();
        assert!(
            rows.saturating_add(2) <= MAX_HEIGHT,
            "{rows} rows no longer fit MAX_HEIGHT"
        );
    }

    #[test]
    fn every_key_it_advertises_is_actually_bound() {
        for (_, keys) in SECTIONS {
            for (keys, _) in *keys {
                for token in keys.split(' ') {
                    let event = to_event(token);
                    assert!(
                        input::action(&event, Mode::Playing).is_some(),
                        "{token} in the help table does not resolve"
                    );
                }
            }
        }
    }

    fn to_event(token: &str) -> KeyEvent {
        match token {
            "Space" => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            "Esc" => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
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
