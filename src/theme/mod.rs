// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Themes: a palette plus a style for every element `tui` draws.

pub mod color;
pub mod elements;
pub mod loader;
pub mod palette;

use ratatui::style::Style;

pub use elements::Element;
pub use palette::Palette;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    styles: Vec<Style>,
    pub name: String,
}

impl Theme {
    #[must_use]
    pub fn new(palette: Palette) -> Self {
        let styles = Element::ALL
            .iter()
            .map(|element| elements::default_style(*element, &palette))
            .collect();
        Self {
            palette,
            styles,
            name: String::from("ansi"),
        }
    }

    #[must_use]
    pub fn style(&self, element: Element) -> Style {
        self.styles
            .get(element.index())
            .copied()
            .unwrap_or_default()
    }

    pub fn set_style(&mut self, element: Element, style: Style) {
        if let Some(slot) = self.styles.get_mut(element.index()) {
            *slot = style;
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(Palette::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn every_element_is_listed_once_in_discriminant_order() {
        for (index, element) in Element::ALL.iter().enumerate() {
            assert_eq!(
                element.index(),
                index,
                "{element:?} is out of order in Element::ALL"
            );
        }
    }

    #[test]
    fn styles_are_looked_up_by_element() {
        let theme = Theme::default();
        assert!(
            theme
                .style(Element::Label)
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            theme.style(Element::HelpWindow).bg,
            Some(theme.palette.background)
        );
    }

    #[test]
    fn a_repalette_moves_every_element_derived_from_that_slot() {
        let palette = Palette {
            accent: Color::Rgb(1, 2, 3),
            ..Palette::default()
        };
        let theme = Theme::new(palette);
        assert_eq!(
            theme.style(Element::CurrentLine).fg,
            Some(Color::Rgb(1, 2, 3))
        );
        assert_eq!(
            theme.style(Element::ScrollProgress).fg,
            Some(Color::Rgb(1, 2, 3))
        );
    }

    #[test]
    fn a_style_can_be_replaced_without_disturbing_its_neighbours() {
        let mut theme = Theme::default();
        let before = theme.style(Element::FarLine);
        theme.set_style(Element::Body, Style::default().fg(Color::Rgb(9, 9, 9)));
        assert_eq!(theme.style(Element::Body).fg, Some(Color::Rgb(9, 9, 9)));
        assert_eq!(
            theme.style(Element::FarLine),
            before,
            "replacing one style moved another"
        );
    }
}
