// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The listener-driven playback clock.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Clock {
    base: Duration,
    started: Option<Instant>,
}

impl Clock {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            base: Duration::ZERO,
            started: None,
        }
    }

    #[must_use]
    pub fn now(&self, at: Instant) -> Duration {
        self.started.map_or(self.base, |started| {
            self.base
                .saturating_add(at.saturating_duration_since(started))
        })
    }

    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.started.is_some()
    }

    pub const fn play(&mut self, at: Instant) {
        if self.started.is_none() {
            self.started = Some(at);
        }
    }

    pub fn pause(&mut self, at: Instant) {
        self.base = self.now(at);
        self.started = None;
    }

    pub fn toggle(&mut self, at: Instant) {
        if self.is_playing() {
            self.pause(at);
        } else {
            self.play(at);
        }
    }

    pub fn seek(&mut self, at: Instant, delta: SignedDuration) {
        let current = self.now(at);
        let moved = match delta {
            SignedDuration::Forward(by) => current.saturating_add(by),
            SignedDuration::Backward(by) => current.saturating_sub(by),
        };
        self.set(at, moved);
    }

    pub const fn set(&mut self, at: Instant, position: Duration) {
        self.base = position;
        if self.started.is_some() {
            self.started = Some(at);
        }
    }

    pub const fn restart(&mut self) {
        self.base = Duration::ZERO;
        self.started = None;
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SignedDuration {
    Forward(Duration),
    Backward(Duration),
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

    #[test]
    fn a_new_clock_is_paused_at_zero() {
        let clock = Clock::new();
        assert!(!clock.is_playing());
        assert_eq!(clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn playing_advances_with_the_instant_passed_to_now() {
        let mut clock = Clock::new();
        clock.play(t(0));
        assert_eq!(clock.now(t(5)), Duration::from_secs(5));
        assert_eq!(clock.now(t(10)), Duration::from_secs(10));
    }

    #[test]
    fn pausing_freezes_the_position_it_was_paused_at() {
        let mut clock = Clock::new();
        clock.play(t(0));
        clock.pause(t(5));
        assert_eq!(clock.now(t(5)), Duration::from_secs(5));
        assert_eq!(
            clock.now(t(20)),
            Duration::from_secs(5),
            "a paused clock kept advancing"
        );
    }

    #[test]
    fn playing_again_resumes_from_where_it_was_paused() {
        let mut clock = Clock::new();
        clock.play(t(0));
        clock.pause(t(5));
        clock.play(t(5));
        assert_eq!(clock.now(t(8)), Duration::from_secs(8));
    }

    #[test]
    fn toggle_flips_between_playing_and_paused() {
        let mut clock = Clock::new();
        assert!(!clock.is_playing());
        clock.toggle(t(0));
        assert!(clock.is_playing());
        clock.toggle(t(3));
        assert!(!clock.is_playing());
        assert_eq!(clock.now(t(100)), Duration::from_secs(3));
    }

    #[test]
    fn seek_forward_and_backward_move_a_paused_clock() {
        let mut clock = Clock::new();
        clock.set(t(0), Duration::from_secs(10));
        clock.seek(t(0), SignedDuration::Forward(Duration::from_secs(5)));
        assert_eq!(clock.now(t(0)), Duration::from_secs(15));
        clock.seek(t(0), SignedDuration::Backward(Duration::from_secs(3)));
        assert_eq!(clock.now(t(0)), Duration::from_secs(12));
    }

    #[test]
    fn seeking_backward_saturates_at_zero_rather_than_going_negative() {
        let mut clock = Clock::new();
        clock.set(t(0), Duration::from_secs(3));
        clock.seek(t(0), SignedDuration::Backward(Duration::from_secs(10)));
        assert_eq!(clock.now(t(0)), Duration::ZERO);
    }

    #[test]
    fn seeking_a_playing_clock_keeps_it_playing_from_the_new_position() {
        let mut clock = Clock::new();
        clock.play(t(0));
        clock.seek(t(2), SignedDuration::Forward(Duration::from_secs(5)));
        assert_eq!(clock.now(t(2)), Duration::from_secs(7));
        assert_eq!(clock.now(t(4)), Duration::from_secs(9));
    }

    #[test]
    fn restart_returns_to_zero_and_pauses() {
        let mut clock = Clock::new();
        clock.play(t(0));
        clock.seek(t(1), SignedDuration::Forward(Duration::from_secs(30)));
        clock.restart();
        assert!(!clock.is_playing());
        assert_eq!(clock.now(t(100)), Duration::ZERO);
    }
}
