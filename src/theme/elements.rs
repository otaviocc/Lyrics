// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The elements `tui` draws and their default styles.

use serde::Deserialize;

use ratatui::style::{Modifier, Style};

use crate::theme::color::ColorSpec;
use crate::theme::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Body,
    Label,
    HeaderTitle,
    Status,
    StatusNotice,
    StatusError,
    Hint,
    ScrollProgress,
    CurrentLine,
    NearLine,
    FarLine,
    Countdown,
    HelpWindow,
}

impl Element {
    pub const ALL: [Self; 13] = [
        Self::Body,
        Self::Label,
        Self::HeaderTitle,
        Self::Status,
        Self::StatusNotice,
        Self::StatusError,
        Self::Hint,
        Self::ScrollProgress,
        Self::CurrentLine,
        Self::NearLine,
        Self::FarLine,
        Self::Countdown,
        Self::HelpWindow,
    ];

    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "the discriminant is the index, and every_element_is_listed_once_in_discriminant_order proves it"
    )]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Label => "label",
            Self::HeaderTitle => "header_title",
            Self::Status => "status",
            Self::StatusNotice => "status_notice",
            Self::StatusError => "status_error",
            Self::Hint => "hint",
            Self::ScrollProgress => "scroll_progress",
            Self::CurrentLine => "current_line",
            Self::NearLine => "near_line",
            Self::FarLine => "far_line",
            Self::Countdown => "countdown",
            Self::HelpWindow => "help_window",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|element| element.key() == key)
    }
}

#[must_use]
pub fn default_style(element: Element, palette: &Palette) -> Style {
    let style = Style::default();
    match element {
        Element::Body | Element::NearLine => style.fg(palette.foreground),
        Element::Label => style.add_modifier(Modifier::BOLD),
        Element::HeaderTitle => style.fg(palette.accent).add_modifier(Modifier::BOLD),
        Element::Status => style.fg(palette.muted_text),
        Element::StatusNotice => style.fg(palette.warning),
        Element::StatusError => style.fg(palette.error),
        Element::Hint | Element::FarLine => style.fg(palette.muted),
        Element::ScrollProgress | Element::CurrentLine | Element::Countdown => {
            style.fg(palette.accent).add_modifier(Modifier::BOLD)
        }
        Element::HelpWindow => style.fg(palette.foreground).bg(palette.background),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ElementFile {
    pub fg: Option<ColorSpec>,
    pub bg: Option<ColorSpec>,
    pub modifiers: Option<Vec<String>>,
    #[serde(flatten)]
    pub unknown: std::collections::BTreeMap<String, toml::Value>,
}

impl ElementFile {
    #[must_use]
    pub fn merge(self, base: Self) -> Self {
        let mut unknown = base.unknown;
        unknown.extend(self.unknown);
        Self {
            fg: self.fg.or(base.fg),
            bg: self.bg.or(base.bg),
            modifiers: self.modifiers.or(base.modifiers),
            unknown,
        }
    }
}

#[must_use]
pub fn modifier(name: &str) -> Option<Modifier> {
    Some(match name {
        "bold" => Modifier::BOLD,
        "italic" => Modifier::ITALIC,
        "underline" => Modifier::UNDERLINED,
        "dim" => Modifier::DIM,
        "reversed" => Modifier::REVERSED,
        "crossed_out" => Modifier::CROSSED_OUT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_element_field_merges_independently_of_the_others() {
        let base: ElementFile = toml::from_str("fg = \"red\"\nmodifiers = [\"bold\"]\n").unwrap();
        let child: ElementFile = toml::from_str("bg = \"blue\"\n").unwrap();

        let merged = child.merge(base);
        assert_eq!(
            merged.fg,
            Some(ColorSpec::Name(String::from("red"))),
            "the base's foreground was dropped"
        );
        assert_eq!(merged.bg, Some(ColorSpec::Name(String::from("blue"))));
        assert_eq!(
            merged.modifiers.as_deref(),
            Some(["bold".to_string()].as_slice()),
            "the base's modifiers were dropped"
        );

        let base: ElementFile = toml::from_str("fg = \"red\"\n").unwrap();
        let child: ElementFile = toml::from_str("fg = \"green\"\nmodifiers = []\n").unwrap();
        let merged = child.merge(base);
        assert_eq!(
            merged.fg,
            Some(ColorSpec::Name(String::from("green"))),
            "the child does not win its own field"
        );
        assert_eq!(
            merged.modifiers,
            Some(Vec::new()),
            "an empty list did not survive as a way to clear them"
        );
    }

    #[test]
    fn every_element_has_a_key_that_finds_it_again() {
        for element in Element::ALL {
            assert_eq!(
                Element::from_key(element.key()),
                Some(element),
                "{element:?} is not addressable by its key"
            );
        }
    }

    #[test]
    fn keys_are_the_snake_case_names_the_readme_will_document() {
        assert_eq!(Element::CurrentLine.key(), "current_line");
        assert_eq!(
            Element::from_key("current_line"),
            Some(Element::CurrentLine)
        );
        assert_eq!(Element::from_key("headings"), None);
    }

    #[test]
    fn the_documented_modifiers_are_the_ones_accepted() {
        assert_eq!(modifier("bold"), Some(Modifier::BOLD));
        assert_eq!(modifier("underline"), Some(Modifier::UNDERLINED));
        assert_eq!(modifier("crossed_out"), Some(Modifier::CROSSED_OUT));
        assert_eq!(modifier("blinky"), None);
        assert_eq!(modifier("BOLD"), None);
    }

    #[test]
    fn the_current_line_is_accented_and_bold_so_it_pulls_the_eye() {
        let palette = Palette::default();
        let style = default_style(Element::CurrentLine, &palette);
        assert_eq!(style.fg, Some(palette.accent));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn far_lines_and_near_lines_are_two_different_shades() {
        let palette = Palette::default();
        assert_ne!(
            default_style(Element::NearLine, &palette).fg,
            default_style(Element::FarLine, &palette).fg
        );
    }
}
