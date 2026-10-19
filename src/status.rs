// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Cargo-style status lines: a right-aligned, colored label, then the rest.

use std::io::IsTerminal;

pub const LABEL_WIDTH: usize = 12;
pub const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Progress,
    Outcome,
    Error,
}

impl Tone {
    const fn style(self) -> &'static str {
        match self {
            Self::Progress => "\x1b[1;36m",
            Self::Outcome => "\x1b[1;33m",
            Self::Error => "\x1b[1;31m",
        }
    }
}

#[must_use]
pub fn line(label: &str, rest: &str, tone: Tone, color: bool) -> String {
    if color {
        format!("{}{label:>LABEL_WIDTH$}{RESET} {rest}", tone.style())
    } else {
        format!("{label:>LABEL_WIDTH$} {rest}")
    }
}

#[must_use]
pub fn color_for(no_color: bool, stream: &impl IsTerminal) -> bool {
    !no_color && color_allowed() && stream.is_terminal()
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
            line("fetched", "a.flac", Tone::Outcome, false),
            "     fetched a.flac"
        );
        assert_eq!(
            line("instrumental", "a.flac", Tone::Outcome, false),
            "instrumental a.flac"
        );
    }

    #[test]
    fn color_wraps_only_the_label() {
        assert_eq!(
            line("error", "a.flac: boom", Tone::Error, true),
            "\x1b[1;31m       error\x1b[0m a.flac: boom"
        );
    }
}
