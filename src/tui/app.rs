// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Application state: the song, the clock, the mode, and the transient notice.

use std::time::{Duration, Instant};

use crate::lrc::SyncedLine;
use crate::theme::Theme;
use crate::tui::clock::{Clock, SignedDuration};
use crate::tui::input::Action;
use crate::tui::picker::Picker;

pub const COUNTDOWN_STEP: Duration = Duration::from_secs(1);
const SHORT_SEEK: Duration = Duration::from_secs(5);
const LONG_SEEK: Duration = Duration::from_secs(10);
const SHORT_NUDGE: Duration = Duration::from_millis(100);
const LONG_NUDGE: Duration = Duration::from_millis(500);
const MIN_BREAK: Duration = Duration::from_secs(5);
const NOTICE_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct Song {
    pub title: String,
    pub artist: Option<String>,
    pub group: Option<String>,
    pub lines: Vec<SyncedLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Countdown { step: u8 },
    Playing,
    Help,
    Picker,
    Filter,
}

impl Mode {
    #[must_use]
    pub const fn is_listing(self) -> bool {
        matches!(self, Self::Picker | Self::Filter)
    }
}

pub struct App {
    pub song: Song,
    pub theme: Theme,
    pub clock: Clock,
    pub mode: Mode,
    pub quit: bool,
    picker: Option<Picker>,
    opening: Mode,
    help_return: Mode,
    notice: Option<(String, Instant)>,
}

impl App {
    #[must_use]
    pub const fn new(song: Song, theme: Theme, counter: bool) -> Self {
        let mode = start_mode(counter);
        Self {
            song,
            theme,
            clock: Clock::new(),
            mode,
            quit: false,
            picker: None,
            opening: mode,
            help_return: mode,
            notice: None,
        }
    }

    #[must_use]
    pub fn with_library(songs: Vec<Song>, theme: Theme, counter: bool) -> Option<Self> {
        let picker = Picker::new(songs);
        let song = picker.current()?.clone();
        Some(Self {
            mode: Mode::Picker,
            help_return: Mode::Picker,
            picker: Some(picker),
            ..Self::new(song, theme, counter)
        })
    }

    #[must_use]
    pub const fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    #[must_use]
    pub const fn browsing(&self) -> Option<&Picker> {
        let listing = match self.mode {
            Mode::Help => self.help_return.is_listing(),
            mode => mode.is_listing(),
        };
        if listing { self.picker.as_ref() } else { None }
    }

    #[must_use]
    pub fn current_index(&self, t: Duration) -> Option<usize> {
        let ms = u64::try_from(t.as_millis()).unwrap_or(u64::MAX);
        let after = self.song.lines.partition_point(|line| line.at_ms <= ms);
        after.checked_sub(1)
    }

    #[must_use]
    pub fn break_remaining(&self, t: Duration) -> Option<Duration> {
        let current = self.current_index(t);
        let (start_ms, from) = match current {
            None => (0, 0),
            Some(index) => {
                let line = self.song.lines.get(index)?;
                if !line.text.trim().is_empty() {
                    return None;
                }
                (line.at_ms, index.saturating_add(1))
            }
        };
        let next = self
            .song
            .lines
            .get(from..)?
            .iter()
            .find(|line| !line.text.trim().is_empty())?;
        let length = Duration::from_millis(next.at_ms.saturating_sub(start_ms));
        (length >= MIN_BREAK).then(|| Duration::from_millis(next.at_ms).saturating_sub(t))
    }

    #[must_use]
    pub fn notice(&self, at: Instant) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, set_at)| at.saturating_duration_since(*set_at) < NOTICE_TTL)
            .map(|(text, _)| text.as_str())
    }

    fn set_notice(&mut self, text: impl Into<String>, at: Instant) {
        self.notice = Some((text.into(), at));
    }

    pub const fn advance_countdown(&mut self, at: Instant) {
        let Mode::Countdown { step } = self.mode else {
            return;
        };
        let next = step.saturating_add(1);
        if next >= 4 {
            self.mode = Mode::Playing;
            self.clock.play(at);
        } else {
            self.mode = Mode::Countdown { step: next };
        }
    }

    fn tap_sync(&mut self, at: Instant) {
        if matches!(self.mode, Mode::Countdown { .. }) {
            return;
        }
        if !self.clock.is_playing() {
            self.start_current_line(at);
            return;
        }
        let now = self.clock.now(at);
        let now_ms = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
        let after = self.song.lines.partition_point(|line| line.at_ms <= now_ms);
        let before = after
            .checked_sub(1)
            .and_then(|index| self.song.lines.get(index));
        let Some(nearest) = [before, self.song.lines.get(after)]
            .into_iter()
            .flatten()
            .min_by_key(|line| line.at_ms.abs_diff(now_ms))
        else {
            return;
        };
        let target = Duration::from_millis(nearest.at_ms);
        self.clock.set(at, target);
        let correction = if target >= now {
            SignedDuration::Forward(target.saturating_sub(now))
        } else {
            SignedDuration::Backward(now.saturating_sub(target))
        };
        self.set_notice(format!("synced {}", format_offset(correction)), at);
    }

    fn start_current_line(&mut self, at: Instant) {
        if let Some(line) = self
            .current_index(self.clock.now(at))
            .and_then(|index| self.song.lines.get(index))
        {
            self.clock.set(at, Duration::from_millis(line.at_ms));
        }
        self.clock.play(at);
    }

    fn play_selected(&mut self) {
        let Some(song) = self.picker.as_mut().and_then(Picker::choose).cloned() else {
            return;
        };
        self.song = song;
        self.clock.restart();
        self.mode = self.opening;
    }

    fn filter_action(&mut self, action: Action) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        match action {
            Action::StartFilter => self.mode = Mode::Filter,
            Action::PickerType(c) => picker.type_char(c),
            Action::PickerErase => {
                if !picker.erase() {
                    self.mode = Mode::Picker;
                }
            }
            Action::PickerClearFilter => {
                picker.clear_filter();
                self.mode = Mode::Picker;
            }
            _ => {}
        }
    }

    fn toggle_picker(&mut self) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if !self.mode.is_listing() {
            picker.rewind();
            self.mode = Mode::Picker;
        } else if picker.chosen().is_some() {
            self.mode = if self.clock.is_playing() {
                Mode::Playing
            } else {
                self.opening
            };
        }
    }

    pub fn apply(&mut self, action: Action, at: Instant) {
        match action {
            Action::TogglePlay => {
                if matches!(self.mode, Mode::Countdown { .. }) {
                    self.mode = Mode::Playing;
                    self.clock.restart();
                } else {
                    self.clock.toggle(at);
                }
            }
            Action::SeekBackward(short) => {
                let by = if short { SHORT_SEEK } else { LONG_SEEK };
                self.clock.seek(at, SignedDuration::Backward(by));
                self.set_notice(format!("-{}s", by.as_secs()), at);
            }
            Action::SeekForward(short) => {
                let by = if short { SHORT_SEEK } else { LONG_SEEK };
                self.clock.seek(at, SignedDuration::Forward(by));
                self.set_notice(format!("+{}s", by.as_secs()), at);
            }
            Action::PreviousLine => {
                if let Some(index) = self.current_index(self.clock.now(at)) {
                    let target = index
                        .checked_sub(1)
                        .and_then(|previous| self.song.lines.get(previous))
                        .map_or(Duration::ZERO, |line| Duration::from_millis(line.at_ms));
                    self.clock.set(at, target);
                }
            }
            Action::NextLine => {
                let now_ms = u64::try_from(self.clock.now(at).as_millis()).unwrap_or(u64::MAX);
                if let Some(line) = self.song.lines.iter().find(|line| line.at_ms > now_ms) {
                    self.clock.set(at, Duration::from_millis(line.at_ms));
                }
            }
            Action::NudgeEarlier(short) => {
                let by = if short { SHORT_NUDGE } else { LONG_NUDGE };
                self.clock.seek(at, SignedDuration::Backward(by));
                self.set_notice(format_offset(SignedDuration::Backward(by)), at);
            }
            Action::NudgeLater(short) => {
                let by = if short { SHORT_NUDGE } else { LONG_NUDGE };
                self.clock.seek(at, SignedDuration::Forward(by));
                self.set_notice(format_offset(SignedDuration::Forward(by)), at);
            }
            Action::TapSync => self.tap_sync(at),
            Action::Restart => {
                self.clock.restart();
            }
            Action::ReplayCountdown => {
                self.clock.restart();
                self.mode = Mode::Countdown { step: 0 };
            }
            Action::PickerUp => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.select_previous();
                }
            }
            Action::PickerDown => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.select_next();
                }
            }
            Action::PickerSelect => self.play_selected(),
            Action::TogglePicker => self.toggle_picker(),
            Action::StartFilter
            | Action::PickerType(_)
            | Action::PickerErase
            | Action::PickerClearFilter => self.filter_action(action),
            Action::ToggleHelp => {
                if self.mode == Mode::Help {
                    self.mode = self.help_return;
                } else {
                    self.help_return = self.mode;
                    self.mode = Mode::Help;
                }
            }
            Action::Quit => self.quit = true,
        }
    }
}

const fn start_mode(counter: bool) -> Mode {
    if counter {
        Mode::Countdown { step: 0 }
    } else {
        Mode::Playing
    }
}

fn format_offset(offset: SignedDuration) -> String {
    let (sign, by) = match offset {
        SignedDuration::Forward(by) => ('+', by),
        SignedDuration::Backward(by) => ('-', by),
    };
    let tenths = by.as_millis().saturating_add(50).saturating_div(100);
    let sign = if tenths == 0 {
        String::new()
    } else {
        sign.to_string()
    };
    format!(
        "{sign}{}.{}s",
        tenths.saturating_div(10),
        tenths.checked_rem(10).unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Instant {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        EPOCH
            .get_or_init(Instant::now)
            .checked_add(Duration::from_secs(secs))
            .unwrap()
    }

    fn song() -> Song {
        Song {
            title: String::from("Test"),
            artist: Some(String::from("Someone")),
            group: None,
            lines: vec![
                SyncedLine {
                    at_ms: 1_000,
                    text: String::from("first"),
                },
                SyncedLine {
                    at_ms: 5_000,
                    text: String::from("second"),
                },
                SyncedLine {
                    at_ms: 10_000,
                    text: String::from("third"),
                },
            ],
        }
    }

    #[test]
    fn current_index_is_none_before_the_first_line() {
        let app = App::new(song(), Theme::default(), false);
        assert_eq!(app.current_index(Duration::from_millis(500)), None);
    }

    #[test]
    fn current_index_lands_exactly_on_a_boundary() {
        let app = App::new(song(), Theme::default(), false);
        assert_eq!(app.current_index(Duration::from_millis(1_000)), Some(0));
        assert_eq!(app.current_index(Duration::from_millis(4_999)), Some(0));
        assert_eq!(app.current_index(Duration::from_millis(5_000)), Some(1));
    }

    #[test]
    fn current_index_is_the_last_line_past_the_end() {
        let app = App::new(song(), Theme::default(), false);
        assert_eq!(app.current_index(Duration::from_secs(999)), Some(2));
    }

    #[test]
    fn without_counter_playback_starts_paused_at_zero() {
        let app = App::new(song(), Theme::default(), false);
        assert_eq!(app.mode, Mode::Playing);
        assert!(!app.clock.is_playing());
    }

    #[test]
    fn with_counter_playback_starts_in_countdown() {
        let app = App::new(song(), Theme::default(), true);
        assert_eq!(app.mode, Mode::Countdown { step: 0 });
    }

    #[test]
    fn the_countdown_runs_three_two_one_play_then_starts_the_clock() {
        let mut app = App::new(song(), Theme::default(), true);
        for _ in 0..3 {
            app.advance_countdown(t(0));
            assert!(matches!(app.mode, Mode::Countdown { .. }));
            assert!(!app.clock.is_playing());
        }
        app.advance_countdown(t(0));
        assert_eq!(app.mode, Mode::Playing);
        assert!(
            app.clock.is_playing(),
            "the clock did not start when PLAY appeared"
        );
    }

    #[test]
    fn space_during_the_countdown_cancels_it_and_leaves_the_clock_paused_at_zero() {
        let mut app = App::new(song(), Theme::default(), true);
        app.advance_countdown(t(0));
        app.apply(Action::TogglePlay, t(0));
        assert_eq!(app.mode, Mode::Playing);
        assert!(!app.clock.is_playing());
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn seek_actions_set_a_notice() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::SeekForward(true), t(0));
        assert_eq!(app.notice(t(0)), Some("+5s"));
        app.apply(Action::SeekBackward(false), t(0));
        assert_eq!(app.notice(t(0)), Some("-10s"));
    }

    #[test]
    fn notice_expires_after_its_ttl() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::SeekForward(true), t(0));
        assert!(app.notice(t(0)).is_some());
        assert_eq!(app.notice(t(10)), None);
    }

    #[test]
    fn previous_and_next_line_jump_to_line_boundaries() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.set(t(0), Duration::from_millis(6_000));
        app.apply(Action::PreviousLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(1_000));
        app.apply(Action::NextLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        app.apply(Action::NextLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(10_000));
    }

    #[test]
    fn previous_line_steps_up_one_line_per_press() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.set(t(0), Duration::from_millis(10_000));
        app.apply(Action::PreviousLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        app.apply(Action::PreviousLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(1_000));
        app.apply(Action::PreviousLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
        app.apply(Action::PreviousLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn enter_while_paused_starts_the_current_line_now() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.set(t(0), Duration::from_millis(6_200));
        app.apply(Action::TapSync, t(0));
        assert!(app.clock.is_playing());
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        assert_eq!(app.clock.now(t(2)), Duration::from_millis(7_000));
    }

    #[test]
    fn enter_while_paused_before_the_first_line_just_plays() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::TapSync, t(0));
        assert!(app.clock.is_playing());
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn enter_during_the_countdown_does_nothing() {
        let mut app = App::new(song(), Theme::default(), true);
        app.apply(Action::TapSync, t(0));
        assert_eq!(app.mode, Mode::Countdown { step: 0 });
        assert!(!app.clock.is_playing());
    }

    #[test]
    fn nudge_actions_move_the_clock_and_set_a_notice() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.set(t(0), Duration::from_secs(2));
        app.apply(Action::NudgeEarlier(true), t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(1_900));
        assert_eq!(app.notice(t(0)), Some("-0.1s"));
        app.apply(Action::NudgeLater(false), t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(2_400));
        assert_eq!(app.notice(t(0)), Some("+0.5s"));
    }

    #[test]
    fn nudging_backward_saturates_at_zero() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::NudgeEarlier(false), t(0));
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn tap_sync_snaps_to_the_nearest_line_start_in_either_direction() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.play(t(0));
        app.clock.set(t(0), Duration::from_millis(4_600));
        app.apply(Action::TapSync, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        assert_eq!(app.notice(t(0)), Some("synced +0.4s"));

        app.clock.set(t(0), Duration::from_millis(6_200));
        app.apply(Action::TapSync, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        assert_eq!(app.notice(t(0)), Some("synced -1.2s"));
    }

    #[test]
    fn tap_sync_while_playing_the_intro_snaps_to_the_first_line() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.play(t(0));
        app.apply(Action::TapSync, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(1_000));
    }

    #[test]
    fn tap_sync_keeps_a_playing_clock_playing() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.play(t(0));
        app.apply(Action::TapSync, t(4));
        assert!(app.clock.is_playing());
        assert_eq!(app.clock.now(t(6)), Duration::from_secs(7));
    }

    #[test]
    fn tap_sync_on_an_empty_song_does_nothing() {
        let mut app = App::new(
            Song {
                lines: Vec::new(),
                ..song()
            },
            Theme::default(),
            false,
        );
        app.clock.play(t(0));
        app.clock.set(t(0), Duration::from_secs(3));
        app.apply(Action::TapSync, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_secs(3));
        assert_eq!(app.notice(t(0)), None);
    }

    #[test]
    fn format_offset_rounds_to_tenths() {
        assert_eq!(
            format_offset(SignedDuration::Forward(Duration::from_millis(1_250))),
            "+1.3s"
        );
        assert_eq!(
            format_offset(SignedDuration::Backward(Duration::from_millis(40))),
            "0.0s"
        );
    }

    fn song_with_breaks() -> Song {
        let line = |at_ms, text: &str| SyncedLine {
            at_ms,
            text: text.to_owned(),
        };
        Song {
            lines: vec![
                line(12_000, "first"),
                line(14_000, ""),
                line(16_000, "after a short break"),
                line(20_000, ""),
                line(30_000, ""),
                line(45_000, "after the solo"),
                line(50_000, ""),
            ],
            ..song()
        }
    }

    #[test]
    fn break_remaining_counts_down_the_intro() {
        let app = App::new(song_with_breaks(), Theme::default(), false);
        assert_eq!(
            app.break_remaining(Duration::ZERO),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            app.break_remaining(Duration::from_millis(11_500)),
            Some(Duration::from_millis(500))
        );
    }

    #[test]
    fn break_remaining_is_none_during_a_lyric() {
        let app = App::new(song_with_breaks(), Theme::default(), false);
        assert_eq!(app.break_remaining(Duration::from_secs(13)), None);
    }

    #[test]
    fn break_remaining_skips_breaks_too_short_to_matter() {
        let app = App::new(song_with_breaks(), Theme::default(), false);
        assert_eq!(app.break_remaining(Duration::from_secs(15)), None);
    }

    #[test]
    fn break_remaining_reads_consecutive_blank_markers_as_one_break() {
        let app = App::new(song_with_breaks(), Theme::default(), false);
        assert_eq!(
            app.break_remaining(Duration::from_secs(22)),
            Some(Duration::from_secs(23))
        );
        assert_eq!(
            app.break_remaining(Duration::from_secs(40)),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn break_remaining_is_none_after_the_last_lyric() {
        let app = App::new(song_with_breaks(), Theme::default(), false);
        assert_eq!(app.break_remaining(Duration::from_secs(55)), None);
    }

    #[test]
    fn restart_returns_to_zero_paused() {
        let mut app = App::new(song(), Theme::default(), false);
        app.clock.play(t(0));
        app.apply(Action::Restart, t(5));
        assert!(!app.clock.is_playing());
        assert_eq!(app.clock.now(t(5)), Duration::ZERO);
    }

    #[test]
    fn help_toggles_on_and_off() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::ToggleHelp, t(0));
        assert_eq!(app.mode, Mode::Help);
        app.apply(Action::ToggleHelp, t(0));
        assert_eq!(app.mode, Mode::Playing);
    }

    #[test]
    fn quit_sets_the_flag() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::Quit, t(0));
        assert!(app.quit);
    }

    fn library() -> Vec<Song> {
        vec![
            Song {
                title: String::from("One"),
                artist: None,
                group: None,
                lines: vec![SyncedLine {
                    at_ms: 1_000,
                    text: String::from("a"),
                }],
            },
            Song {
                title: String::from("Two"),
                artist: None,
                group: None,
                lines: vec![SyncedLine {
                    at_ms: 2_000,
                    text: String::from("b"),
                }],
            },
        ]
    }

    #[test]
    fn a_library_opens_on_the_list_with_the_clock_untouched() {
        let app = App::with_library(library(), Theme::default(), false).unwrap();

        assert_eq!(app.mode, Mode::Picker);
        assert_eq!(app.song.title, "One");
        assert_eq!(app.clock.now(t(0)), Duration::ZERO);
        assert!(!app.clock.is_playing());
        assert_eq!(app.browsing().map(Picker::len), Some(2));
    }

    #[test]
    fn an_empty_library_has_nothing_to_open() {
        assert!(App::with_library(Vec::new(), Theme::default(), false).is_none());
    }

    #[test]
    fn choosing_a_song_loads_it_and_starts_playback_from_zero() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::PickerDown, t(0));
        app.apply(Action::PickerSelect, t(0));

        assert_eq!(app.mode, Mode::Playing);
        assert_eq!(app.song.title, "Two");
        assert_eq!(app.clock.now(t(5)), Duration::ZERO);
        assert_eq!(app.picker().and_then(Picker::chosen), Some(1));
    }

    #[test]
    fn choosing_a_song_runs_the_countdown_again_when_it_was_asked_for() {
        let mut app = App::with_library(library(), Theme::default(), true).unwrap();

        app.apply(Action::PickerSelect, t(0));

        assert_eq!(app.mode, Mode::Countdown { step: 0 });
    }

    #[test]
    fn the_list_cannot_be_closed_until_a_song_has_been_chosen() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::TogglePicker, t(0));
        assert_eq!(app.mode, Mode::Picker, "there is nothing to go back to yet");

        app.apply(Action::PickerSelect, t(0));
        app.apply(Action::TogglePicker, t(0));
        assert_eq!(app.mode, Mode::Picker);

        app.apply(Action::TogglePicker, t(0));
        assert_eq!(app.mode, Mode::Playing);
    }

    #[test]
    fn reopening_the_list_puts_the_cursor_back_on_the_song_being_followed() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::PickerSelect, t(0));
        app.apply(Action::TogglePicker, t(0));
        app.apply(Action::PickerDown, t(0));
        app.apply(Action::TogglePicker, t(0));
        app.apply(Action::TogglePicker, t(0));

        assert_eq!(app.picker().map(Picker::selected), Some(0));
    }

    #[test]
    fn closing_the_list_leaves_the_clock_running() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();
        app.apply(Action::PickerSelect, t(0));
        app.apply(Action::TogglePlay, t(0));

        app.apply(Action::TogglePicker, t(1));
        app.apply(Action::TogglePicker, t(2));

        assert!(app.clock.is_playing(), "browsing must not stop the clock");
        assert_eq!(app.clock.now(t(3)), Duration::from_secs(3));
    }

    #[test]
    fn the_picker_keys_do_nothing_without_a_library() {
        let mut app = App::new(song(), Theme::default(), false);

        app.apply(Action::TogglePicker, t(0));
        app.apply(Action::PickerDown, t(0));
        app.apply(Action::PickerSelect, t(0));

        assert_eq!(app.mode, Mode::Playing);
        assert_eq!(app.song.title, "Test");
    }

    #[test]
    fn help_returns_to_the_list_it_was_opened_from() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::ToggleHelp, t(0));
        assert_eq!(app.mode, Mode::Help);

        app.apply(Action::ToggleHelp, t(0));
        assert_eq!(app.mode, Mode::Picker);
    }

    #[test]
    fn help_resumes_a_countdown_rather_than_skipping_it() {
        let mut app = App::new(song(), Theme::default(), true);

        app.apply(Action::ToggleHelp, t(0));
        app.apply(Action::ToggleHelp, t(0));

        assert_eq!(app.mode, Mode::Countdown { step: 0 });
    }

    #[test]
    fn help_opened_over_the_list_still_counts_as_browsing() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();
        assert!(app.browsing().is_some());

        app.apply(Action::ToggleHelp, t(0));
        assert!(
            app.browsing().is_some(),
            "help floats over the list, it does not leave it"
        );

        app.apply(Action::ToggleHelp, t(0));
        app.apply(Action::PickerSelect, t(0));
        assert!(app.browsing().is_none());

        app.apply(Action::ToggleHelp, t(0));
        assert!(
            app.browsing().is_none(),
            "help over playback is not browsing"
        );
    }

    #[test]
    fn leaving_the_list_mid_countdown_and_coming_back_replays_it() {
        let mut app = App::with_library(library(), Theme::default(), true).unwrap();
        app.apply(Action::PickerSelect, t(0));
        app.advance_countdown(t(1));
        assert_eq!(app.mode, Mode::Countdown { step: 1 });

        app.apply(Action::TogglePicker, t(2));
        app.apply(Action::TogglePicker, t(3));

        assert_eq!(
            app.mode,
            Mode::Countdown { step: 0 },
            "the pre-roll the picker interrupted is re-armed, not dropped"
        );
        assert!(!app.clock.is_playing());
    }

    #[test]
    fn leaving_the_list_mid_playback_does_not_replay_the_countdown() {
        let mut app = App::with_library(library(), Theme::default(), true).unwrap();
        app.apply(Action::PickerSelect, t(0));
        for step in 0..4 {
            app.advance_countdown(t(1 + step));
        }
        assert_eq!(app.mode, Mode::Playing);
        assert!(app.clock.is_playing());

        app.apply(Action::TogglePicker, t(10));
        app.apply(Action::TogglePicker, t(11));

        assert_eq!(
            app.mode,
            Mode::Playing,
            "browsing mid-song must not restart the countdown over a running clock"
        );
        assert!(app.clock.is_playing());
    }

    #[test]
    fn slash_opens_the_filter_and_typing_narrows_the_list() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::StartFilter, t(0));
        assert_eq!(app.mode, Mode::Filter);
        assert!(app.browsing().is_some(), "filtering is still browsing");

        app.apply(Action::PickerType('t'), t(0));
        app.apply(Action::PickerType('w'), t(0));
        assert_eq!(app.browsing().map(Picker::len), Some(1));
        assert_eq!(app.browsing().map(Picker::query), Some("tw"));

        app.apply(Action::PickerSelect, t(0));
        assert_eq!(app.mode, Mode::Playing);
        assert_eq!(app.song.title, "Two");
        assert_eq!(
            app.picker().map(Picker::query),
            Some("tw"),
            "the filter survives the pick so the next pick starts from it"
        );
    }

    #[test]
    fn escape_clears_the_filter_and_backspace_on_an_empty_query_leaves_filter_mode() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();

        app.apply(Action::StartFilter, t(0));
        app.apply(Action::PickerType('x'), t(0));
        assert_eq!(app.browsing().map(Picker::len), Some(0));

        app.apply(Action::PickerClearFilter, t(0));
        assert_eq!(app.mode, Mode::Picker);
        assert_eq!(app.browsing().map(Picker::len), Some(2));

        app.apply(Action::StartFilter, t(0));
        app.apply(Action::PickerType('o'), t(0));
        app.apply(Action::PickerErase, t(0));
        assert_eq!(
            app.mode,
            Mode::Filter,
            "erasing a character stays in the filter"
        );
        app.apply(Action::PickerErase, t(0));
        assert_eq!(app.mode, Mode::Picker, "erasing past the start leaves it");
    }

    #[test]
    fn tab_closes_the_list_from_filter_mode_too_and_help_returns_to_it() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();
        app.apply(Action::PickerSelect, t(0));
        app.apply(Action::TogglePicker, t(0));
        app.apply(Action::StartFilter, t(0));

        app.apply(Action::ToggleHelp, t(0));
        assert!(app.browsing().is_some());
        app.apply(Action::ToggleHelp, t(0));
        assert_eq!(app.mode, Mode::Filter);

        app.apply(Action::TogglePicker, t(0));
        assert_eq!(app.mode, Mode::Playing);
    }

    #[test]
    fn the_filter_keys_do_nothing_without_a_library() {
        let mut app = App::new(song(), Theme::default(), false);
        app.apply(Action::StartFilter, t(0));
        app.apply(Action::PickerType('x'), t(0));
        assert_eq!(app.mode, Mode::Playing);
    }

    #[test]
    fn without_the_counter_flag_the_list_always_closes_into_playback() {
        let mut app = App::with_library(library(), Theme::default(), false).unwrap();
        app.apply(Action::PickerSelect, t(0));

        app.apply(Action::TogglePicker, t(1));
        app.apply(Action::TogglePicker, t(2));

        assert_eq!(app.mode, Mode::Playing);
    }
}
