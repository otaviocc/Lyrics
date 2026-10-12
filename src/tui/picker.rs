// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The `--folder` song list: the cursor over it, the filter it's narrowed by, and the overlay's geometry.

use std::ops::Range;

use ratatui::layout::Size;

use crate::tui::app::Song;
use crate::tui::clamp_fraction;

pub const TITLE: &str = " Lyrics ";
const LABEL_SEPARATOR: &str = " — ";
const GROUP_SEPARATOR: &str = " / ";
const PLAYING_MARKER: &str = "▸ ";
const IDLE_MARKER: &str = "  ";
const WIDTH_FRACTION: u16 = 70;
const MIN_WIDTH: u16 = 24;
const MAX_WIDTH: u16 = 72;
const HEIGHT_FRACTION: u16 = 80;
const MIN_HEIGHT: u16 = 5;
const MAX_HEIGHT: u16 = 26;
const BORDER_ROWS: u16 = 2;

#[must_use]
pub fn outer(area: Size, rows: usize) -> Size {
    let width = clamp_fraction(area.width, WIDTH_FRACTION, MIN_WIDTH, MAX_WIDTH);
    let ceiling = clamp_fraction(area.height, HEIGHT_FRACTION, MIN_HEIGHT, MAX_HEIGHT);
    let wanted = u16::try_from(rows.max(1))
        .unwrap_or(u16::MAX)
        .saturating_add(BORDER_ROWS);
    Size::new(width, wanted.min(ceiling))
}

#[must_use]
pub fn title(query: &str, filtering: bool) -> String {
    if query.is_empty() && !filtering {
        return TITLE.to_owned();
    }
    format!(" Lyrics  /{query} ")
}

#[derive(Debug, Clone)]
pub struct Picker {
    songs: Vec<Song>,
    bodies: Vec<String>,
    query: String,
    visible: Vec<usize>,
    selected: usize,
    chosen: Option<usize>,
}

impl Picker {
    #[must_use]
    pub fn new(songs: Vec<Song>) -> Self {
        let bodies = songs.iter().map(body).collect();
        let visible = (0..songs.len()).collect();
        Self {
            songs,
            bodies,
            query: String::new(),
            visible,
            selected: 0,
            chosen: None,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.visible.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.songs.len()
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn is_filtered(&self) -> bool {
        !self.query.is_empty()
    }

    fn song_at(&self, row: usize) -> Option<usize> {
        self.visible.get(row).copied()
    }

    #[must_use]
    pub fn current(&self) -> Option<&Song> {
        self.songs.get(self.song_at(self.selected)?)
    }

    #[must_use]
    pub const fn chosen(&self) -> Option<usize> {
        self.chosen
    }

    pub fn choose(&mut self) -> Option<&Song> {
        let index = self.song_at(self.selected)?;
        let song = self.songs.get(index)?;
        self.chosen = Some(index);
        Some(song)
    }

    pub fn rewind(&mut self) {
        if let Some(row) = self
            .chosen
            .and_then(|chosen| self.visible.iter().position(|&index| index == chosen))
        {
            self.selected = row;
        }
    }

    pub const fn select_next(&mut self) {
        let last = self.visible.len().saturating_sub(1);
        if self.selected < last {
            self.selected = self.selected.saturating_add(1);
        }
    }

    pub const fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn erase(&mut self) -> bool {
        if self.query.pop().is_none() {
            return false;
        }
        self.refilter();
        true
    }

    pub fn clear_filter(&mut self) {
        self.query.clear();
        self.refilter();
    }

    fn refilter(&mut self) {
        let keep = self.song_at(self.selected);
        let needle = self.query.to_lowercase();
        self.visible = self
            .bodies
            .iter()
            .enumerate()
            .filter(|(_, body)| needle.is_empty() || body.to_lowercase().contains(&needle))
            .map(|(index, _)| index)
            .collect();
        self.selected = keep
            .and_then(|index| self.visible.iter().position(|&shown| shown == index))
            .unwrap_or(0);
    }

    #[must_use]
    pub fn label(&self, row: usize) -> Option<String> {
        let index = self.song_at(row)?;
        let body = self.bodies.get(index)?;
        let marker = if self.chosen == Some(index) {
            PLAYING_MARKER
        } else {
            IDLE_MARKER
        };
        Some(format!("{marker}{body}"))
    }

    #[must_use]
    pub fn window(&self, height: usize) -> Range<usize> {
        let total = self.visible.len();
        let start = self
            .selected
            .saturating_sub(height.saturating_div(2))
            .min(total.saturating_sub(height));
        start..start.saturating_add(height).min(total)
    }
}

fn body(song: &Song) -> String {
    let name = song.artist.as_ref().map_or_else(
        || song.title.clone(),
        |artist| format!("{artist}{LABEL_SEPARATOR}{}", song.title),
    );
    song.group.as_ref().map_or_else(
        || name.clone(),
        |group| format!("{group}{GROUP_SEPARATOR}{name}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: Option<&str>, group: Option<&str>) -> Song {
        Song {
            title: title.to_owned(),
            artist: artist.map(str::to_owned),
            group: group.map(str::to_owned),
            lines: Vec::new(),
        }
    }

    #[test]
    fn an_artist_reaches_the_label_and_a_missing_row_has_none() {
        let picker = Picker::new(vec![
            song("01 First", None, None),
            song("Second", Some("A Band"), None),
        ]);

        assert_eq!(picker.label(0).unwrap(), "  01 First");
        assert_eq!(picker.label(1).unwrap(), "  A Band — Second");
        assert_eq!(picker.label(2), None);
    }

    #[test]
    fn a_nested_song_is_labelled_with_its_folder_first() {
        let picker = Picker::new(vec![song(
            "Fade to Black",
            Some("Metallica"),
            Some("Metallica/Ride the Lightning"),
        )]);
        assert_eq!(
            picker.label(0).unwrap(),
            "  Metallica/Ride the Lightning / Metallica — Fade to Black"
        );
    }

    fn picker_of(count: usize) -> Picker {
        let songs = (0..count)
            .map(|index| song(&format!("Song {index}"), None, None))
            .collect();
        Picker::new(songs)
    }

    #[test]
    fn the_cursor_clamps_at_both_ends_rather_than_wrapping() {
        let mut picker = picker_of(3);

        picker.select_previous();
        assert_eq!(picker.selected(), 0, "up from the top stays at the top");

        picker.select_next();
        picker.select_next();
        picker.select_next();
        picker.select_next();
        assert_eq!(picker.selected(), 2, "down from the bottom stays put");
    }

    #[test]
    fn the_cursor_does_not_move_in_an_empty_list() {
        let mut picker = picker_of(0);
        picker.select_next();
        picker.select_previous();
        assert_eq!(picker.selected(), 0);
        assert!(picker.current().is_none());
    }

    #[test]
    fn choosing_records_the_row_and_marks_it_in_the_label() {
        let mut picker = picker_of(3);
        picker.select_next();

        assert_eq!(picker.chosen(), None);
        assert_eq!(picker.choose().unwrap().title, "Song 1");
        assert_eq!(picker.chosen(), Some(1));
        assert_eq!(picker.label(1).unwrap(), "▸ Song 1");
        assert_eq!(picker.label(0).unwrap(), "  Song 0");
    }

    #[test]
    fn rewind_returns_the_cursor_to_the_song_being_followed() {
        let mut picker = picker_of(5);
        picker.select_next();
        picker.choose();
        picker.select_next();
        picker.select_next();
        assert_eq!(picker.selected(), 3);

        picker.rewind();
        assert_eq!(picker.selected(), 1);
    }

    #[test]
    fn rewind_is_a_no_op_before_anything_has_been_chosen() {
        let mut picker = picker_of(5);
        picker.select_next();
        picker.rewind();
        assert_eq!(picker.selected(), 1);
    }

    #[test]
    fn typing_narrows_the_list_case_insensitively_and_erasing_widens_it_again() {
        let mut picker = Picker::new(vec![
            song("Blackened", Some("Metallica"), None),
            song("One", Some("Metallica"), None),
            song("Black Dog", Some("Led Zeppelin"), None),
        ]);

        picker.type_char('b');
        picker.type_char('L');
        assert_eq!(picker.len(), 2);
        assert_eq!(picker.total(), 3);
        assert_eq!(picker.query(), "bL");
        assert_eq!(picker.label(0).unwrap(), "  Metallica — Blackened");
        assert_eq!(picker.label(1).unwrap(), "  Led Zeppelin — Black Dog");

        picker.type_char('a');
        picker.type_char('c');
        picker.type_char('k');
        picker.type_char(' ');
        assert_eq!(picker.len(), 1);
        assert_eq!(picker.current().unwrap().title, "Black Dog");

        assert!(picker.erase());
        assert_eq!(picker.len(), 2);

        picker.clear_filter();
        assert_eq!(picker.len(), 3);
        assert!(!picker.is_filtered());
        assert!(!picker.erase(), "nothing left to erase");
    }

    #[test]
    fn the_cursor_follows_its_song_through_a_filter_or_falls_back_to_the_top() {
        let mut picker = Picker::new(vec![
            song("Alpha", None, None),
            song("Beta", None, None),
            song("Gamma", None, None),
        ]);
        picker.select_next();
        picker.select_next();
        assert_eq!(picker.current().unwrap().title, "Gamma");

        picker.type_char('m');
        assert_eq!(picker.current().unwrap().title, "Gamma");
        assert_eq!(picker.selected(), 0, "Gamma is now the only row");

        picker.clear_filter();
        assert_eq!(picker.selected(), 2, "Gamma is back on its own row");

        picker.type_char('a');
        picker.type_char('l');
        assert_eq!(picker.current().unwrap().title, "Alpha");
        assert_eq!(picker.selected(), 0);
    }

    #[test]
    fn a_filter_with_no_match_leaves_an_empty_list_that_cannot_choose() {
        let mut picker = picker_of(3);
        picker.type_char('z');
        assert!(picker.is_empty());
        assert!(picker.choose().is_none());
        assert_eq!(picker.label(0), None);
    }

    #[test]
    fn the_chosen_song_is_marked_even_when_the_filter_moves_its_row() {
        let mut picker = Picker::new(vec![song("Alpha", None, None), song("Beta", None, None)]);
        picker.select_next();
        picker.choose();
        picker.type_char('b');
        assert_eq!(picker.label(0).unwrap(), "▸ Beta");
        picker.rewind();
        assert_eq!(picker.selected(), 0);
    }

    #[test]
    fn the_whole_list_is_visible_when_it_fits() {
        let picker = picker_of(4);
        assert_eq!(picker.window(10), 0..4);
    }

    #[test]
    fn the_window_scrolls_to_keep_the_cursor_visible() {
        let mut picker = picker_of(20);
        assert_eq!(picker.window(5), 0..5);

        for _ in 0..10 {
            picker.select_next();
        }
        let window = picker.window(5);
        assert!(
            window.contains(&picker.selected()),
            "{window:?} lost the cursor at {}",
            picker.selected()
        );

        for _ in 0..20 {
            picker.select_next();
        }
        let window = picker.window(5);
        assert_eq!(window, 15..20, "the last row sits at the bottom edge");
    }

    #[test]
    fn a_zero_height_window_is_empty_rather_than_a_panic() {
        let picker = picker_of(20);
        assert_eq!(picker.window(0), 0..0);
        assert_eq!(picker_of(0).window(5), 0..0);
    }

    #[test]
    fn the_title_shows_the_query_while_filtering() {
        assert_eq!(title("", false), TITLE);
        assert_eq!(title("", true), " Lyrics  / ");
        assert_eq!(title("one", false), " Lyrics  /one ");
    }

    #[test]
    fn the_box_never_outgrows_the_area_it_floats_over() {
        for width in 1u16..200 {
            for height in 1u16..40 {
                for rows in [0usize, 1, 7, 200] {
                    let size = outer(Size::new(width, height), rows);
                    assert!(size.width <= width, "{width}x{height} rows={rows}");
                    assert!(size.height <= height, "{width}x{height} rows={rows}");
                }
            }
        }
    }

    #[test]
    fn the_box_shrinks_to_a_short_list_instead_of_filling_the_screen() {
        let short = outer(Size::new(80, 40), 3);
        assert_eq!(short.height, 5, "three rows plus the border");

        let long = outer(Size::new(80, 40), 100);
        assert_eq!(long.height, MAX_HEIGHT);

        let empty = outer(Size::new(80, 40), 0);
        assert_eq!(
            empty.height, 3,
            "one row for the no-match notice, plus the border"
        );
    }
}
