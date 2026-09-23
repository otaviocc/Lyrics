// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Application state: the song, the clock, the mode, and the transient notice.

use std::time::{Duration, Instant};

use crate::lrc::SyncedLine;
use crate::theme::Theme;
use crate::tui::clock::{Clock, SignedDuration};
use crate::tui::input::Action;

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
    pub lines: Vec<SyncedLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Countdown { step: u8 },
    Playing,
    Help,
}

pub struct App {
    pub song: Song,
    pub theme: Theme,
    pub clock: Clock,
    pub mode: Mode,
    pub quit: bool,
    notice: Option<(String, Instant)>,
}

impl App {
    #[must_use]
    pub const fn new(song: Song, theme: Theme, counter: bool) -> Self {
        Self {
            song,
            theme,
            clock: Clock::new(),
            mode: if counter {
                Mode::Countdown { step: 0 }
            } else {
                Mode::Playing
            },
            quit: false,
            notice: None,
        }
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
            Action::ToggleHelp => {
                self.mode = if self.mode == Mode::Help {
                    Mode::Playing
                } else {
                    Mode::Help
                };
            }
            Action::Quit => self.quit = true,
        }
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
}
