// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Cargo-style status lines: a right-aligned, colored label, then the rest.

use std::io::IsTerminal;

use ratatui::style::Color;

use crate::theme::Palette;

pub const LABEL_WIDTH: usize = 12;
pub const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Progress,
    Outcome,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paint {
    progress: String,
    outcome: String,
    error: String,
}

impl Default for Paint {
    fn default() -> Self {
        Self::new(Color::Cyan, Color::Yellow, Color::Red)
    }
}

impl Paint {
    #[must_use]
    pub fn new(progress: Color, outcome: Color, error: Color) -> Self {
        Self {
            progress: bold(progress),
            outcome: bold(outcome),
            error: bold(error),
        }
    }

    #[must_use]
    pub fn from_palette(palette: &Palette) -> Self {
        Self::new(palette.accent, palette.warning, palette.error)
    }

    fn style(&self, tone: Tone) -> &str {
        match tone {
            Tone::Progress => &self.progress,
            Tone::Outcome => &self.outcome,
            Tone::Error => &self.error,
        }
    }
}

fn bold(color: Color) -> String {
    sgr(color).map_or_else(|| String::from("\x1b[1m"), |code| format!("\x1b[1;{code}m"))
}

fn sgr(color: Color) -> Option<String> {
    let code = match color {
        Color::Reset => return None,
        Color::Black => "30",
        Color::Red => "31",
        Color::Green => "32",
        Color::Yellow => "33",
        Color::Blue => "34",
        Color::Magenta => "35",
        Color::Cyan => "36",
        Color::Gray => "37",
        Color::DarkGray => "90",
        Color::LightRed => "91",
        Color::LightGreen => "92",
        Color::LightYellow => "93",
        Color::LightBlue => "94",
        Color::LightMagenta => "95",
        Color::LightCyan => "96",
        Color::White => "97",
        Color::Indexed(index) => return Some(format!("38;5;{index}")),
        Color::Rgb(r, g, b) => return Some(format!("38;2;{r};{g};{b}")),
    };
    Some(code.to_owned())
}

#[must_use]
pub fn line(label: &str, rest: &str, tone: Tone, paint: Option<&Paint>) -> String {
    paint.map_or_else(
        || format!("{label:>LABEL_WIDTH$} {rest}"),
        |paint| format!("{}{label:>LABEL_WIDTH$}{RESET} {rest}", paint.style(tone)),
    )
}

#[must_use]
pub fn paint_for<'a>(
    paint: &'a Paint,
    no_color: bool,
    stream: &impl IsTerminal,
) -> Option<&'a Paint> {
    (!no_color && color_allowed() && stream.is_terminal()).then_some(paint)
}

#[must_use]
pub fn color_allowed() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_right_align_to_cargo_width() {
        assert_eq!(
            line("fetched", "a.flac", Tone::Outcome, None),
            "     fetched a.flac"
        );
        assert_eq!(
            line("instrumental", "a.flac", Tone::Outcome, None),
            "instrumental a.flac"
        );
    }

    #[test]
    fn color_wraps_only_the_label() {
        assert_eq!(
            line(
                "error",
                "a.flac: boom",
                Tone::Error,
                Some(&Paint::default())
            ),
            "\x1b[1;31m       error\x1b[0m a.flac: boom"
        );
    }

    #[test]
    fn a_palette_paints_with_its_accent_warning_and_error_slots() {
        let palette = Palette {
            accent: Color::Rgb(1, 2, 3),
            warning: Color::Indexed(214),
            error: Color::Reset,
            ..Palette::default()
        };
        let paint = Paint::from_palette(&palette);
        assert_eq!(
            line("Scanning", "1/2", Tone::Progress, Some(&paint)),
            "\x1b[1;38;2;1;2;3m    Scanning\x1b[0m 1/2"
        );
        assert_eq!(
            line("fetched", "a", Tone::Outcome, Some(&paint)),
            "\x1b[1;38;5;214m     fetched\x1b[0m a"
        );
        assert_eq!(
            line("error", "a", Tone::Error, Some(&paint)),
            "\x1b[1m       error\x1b[0m a"
        );
    }
}
