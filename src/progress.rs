// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The one-line counter `scan` keeps on stderr while it works.

use std::io::{IsTerminal as _, Write as _};
use std::time::{Duration, Instant};

use crate::cli::Options;
use crate::runner::Summary;

const REDRAW_EVERY: Duration = Duration::from_millis(100);
const CLEAR_LINE: &str = "\r\x1b[K";

#[derive(Debug)]
pub struct Progress {
    enabled: bool,
    total: usize,
    done: usize,
    showing: bool,
    last_draw: Option<Instant>,
}

impl Progress {
    #[must_use]
    pub fn for_scan(opts: &Options, total: usize) -> Self {
        let enabled =
            !opts.quiet && opts.verbose == 0 && total > 0 && std::io::stderr().is_terminal();
        Self::new(total, enabled)
    }

    #[must_use]
    pub const fn new(total: usize, enabled: bool) -> Self {
        Self {
            enabled,
            total,
            done: 0,
            showing: false,
            last_draw: None,
        }
    }

    #[must_use]
    pub const fn done(&self) -> usize {
        self.done
    }

    #[must_use]
    pub const fn is_showing(&self) -> bool {
        self.showing
    }

    pub fn tick(&mut self, summary: &Summary, now: Instant) {
        self.done = self.done.saturating_add(1);
        if !self.enabled {
            return;
        }
        let stale = self
            .last_draw
            .is_none_or(|at| now.saturating_duration_since(at) >= REDRAW_EVERY);
        if !self.showing || stale || self.done >= self.total {
            self.draw(summary, now);
        }
    }

    fn draw(&mut self, summary: &Summary, now: Instant) {
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "{CLEAR_LINE}{}", line(self.done, self.total, summary));
        let _ = err.flush();
        self.showing = true;
        self.last_draw = Some(now);
    }

    pub fn clear(&mut self) {
        if !self.showing {
            return;
        }
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "{CLEAR_LINE}");
        let _ = err.flush();
        self.showing = false;
    }

    pub fn finish(&mut self) {
        self.clear();
    }
}

#[must_use]
pub fn line(done: usize, total: usize, summary: &Summary) -> String {
    let fetched = summary.synced.saturating_add(summary.upgraded);
    format!(
        "{done}/{total} · fetched {fetched} · plain {} · missing {}",
        summary.plain, summary.missing
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_counts_both_kinds_of_synced_fetch_as_fetched() {
        let summary = Summary {
            synced: 3,
            upgraded: 2,
            plain: 1,
            missing: 4,
            ..Summary::default()
        };
        assert_eq!(
            line(7, 100, &summary),
            "7/100 · fetched 5 · plain 1 · missing 4"
        );
    }

    #[test]
    fn a_disabled_counter_still_counts_but_never_shows() {
        let mut progress = Progress::new(3, false);
        let summary = Summary::default();
        progress.tick(&summary, Instant::now());
        progress.tick(&summary, Instant::now());
        assert_eq!(progress.done(), 2);
        assert!(!progress.is_showing());
        progress.clear();
        progress.finish();
        assert!(!progress.is_showing());
    }

    #[test]
    fn the_counter_saturates_rather_than_overflowing() {
        let mut progress = Progress::new(usize::MAX, false);
        progress.done = usize::MAX;
        progress.tick(&Summary::default(), Instant::now());
        assert_eq!(progress.done(), usize::MAX);
    }
}
