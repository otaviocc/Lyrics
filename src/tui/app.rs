// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Application state: the song, the clock, and what's currently on screen.

use std::time::{Duration, Instant};

use crate::lrc::SyncedLine;
use crate::theme::Theme;
use crate::tui::clock::{Clock, SignedDuration};
use crate::tui::input::Action;

/// How long each countdown digit (and `PLAY`) stays on screen.
pub const COUNTDOWN_STEP: Duration = Duration::from_secs(1);
/// Seek amounts bound to the short/long seek keys.
const SHORT_SEEK: Duration = Duration::from_secs(5);
const LONG_SEEK: Duration = Duration::from_secs(10);
/// Fine-tune nudge, for lining the clock up by ear.
const NUDGE: Duration = Duration::from_millis(100);
/// How long a transient status notice (like "+5s") stays visible.
const NOTICE_TTL: Duration = Duration::from_secs(2);

/// A song's title, artist and synced lyric timeline: what `tui::run` was handed, independent
/// of where it came from (a provider or `--file`).
#[derive(Debug, Clone)]
pub struct Song {
    pub title: String,
    pub artist: Option<String>,
    pub lines: Vec<SyncedLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Counting down to `PLAY`; the clock is not running yet. `started` is when the countdown
    /// began, so each step's remaining time is computed from it rather than accumulated.
    Countdown {
        step: u8,
    },
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

    /// Index of the line active at `t`: the last line whose `at_ms` is `<= t`. `None` before
    /// the first line starts.
    #[must_use]
    pub fn current_index(&self, t: Duration) -> Option<usize> {
        let ms = u64::try_from(t.as_millis()).unwrap_or(u64::MAX);
        let after = self.song.lines.partition_point(|line| line.at_ms <= ms);
        after.checked_sub(1)
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

    /// Advance the countdown (called on every tick while `mode` is `Countdown`), moving to
    /// `Playing` once it runs out. Returns `true` when the countdown just finished, so the
    /// caller can start the clock at exactly the frame `PLAY` first shows.
    pub const fn advance_countdown(&mut self, at: Instant) {
        let Mode::Countdown { step } = self.mode else {
            return;
        };
        // Five steps: 3, 2, 1, PLAY, then playing lyrics.
        let next = step.saturating_add(1);
        if next >= 4 {
            self.mode = Mode::Playing;
            self.clock.play(at);
        } else {
            self.mode = Mode::Countdown { step: next };
        }
    }

    pub fn apply(&mut self, action: Action, at: Instant) {
        match action {
            Action::TogglePlay => {
                if matches!(self.mode, Mode::Countdown { .. }) {
                    // Canceling the countdown leaves the clock paused at 00:00, per spec.
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
                if let Some(index) = self.current_index(self.clock.now(at))
                    && let Some(line) = self.song.lines.get(index)
                {
                    self.clock.set(at, Duration::from_millis(line.at_ms));
                }
            }
            Action::NextLine => {
                let now_ms = u64::try_from(self.clock.now(at).as_millis()).unwrap_or(u64::MAX);
                if let Some(line) = self.song.lines.iter().find(|line| line.at_ms > now_ms) {
                    self.clock.set(at, Duration::from_millis(line.at_ms));
                }
            }
            Action::NudgeEarlier => self.clock.seek(at, SignedDuration::Backward(NUDGE)),
            Action::NudgeLater => self.clock.seek(at, SignedDuration::Forward(NUDGE)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Instant {
        // See `clock::tests::t`: anchored off one fixed epoch so real time elapsed between
        // calls can't leak into the offsets these tests ask for.
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
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(5_000));
        app.apply(Action::NextLine, t(0));
        assert_eq!(app.clock.now(t(0)), Duration::from_millis(10_000));
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
