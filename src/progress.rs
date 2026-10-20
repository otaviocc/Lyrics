// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The one-line counter `scan` keeps on stderr while it works.

use std::io::{IsTerminal as _, Write as _};
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr as _;

use crate::cli::Options;
use crate::runner::Summary;
use crate::status::{self, LABEL_WIDTH, Paint, Tone};

const REDRAW_EVERY: Duration = Duration::from_millis(100);
const CLEAR_LINE: &str = "\r\x1b[K";
const HEADER: &str = "Scanning";
const FALLBACK_COLUMNS: usize = 80;
const MIN_BAR: usize = 10;
const MAX_BAR: usize = 40;
const BAR_CHROME: usize = "[] ".len();
const SPARE_COLUMN: usize = 1;
const SEPARATOR: &str = " · ";

#[derive(Debug)]
pub struct Progress {
    enabled: bool,
    paint: Option<Paint>,
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
        Self {
            paint: status::paint_for(&opts.paint, opts.no_color, &std::io::stderr()).cloned(),
            ..Self::new(total, enabled)
        }
    }

    #[must_use]
    pub const fn new(total: usize, enabled: bool) -> Self {
        Self {
            enabled,
            paint: None,
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
        let columns = ratatui::crossterm::terminal::size()
            .map_or(FALLBACK_COLUMNS, |(cols, _)| usize::from(cols));
        let text = line(self.done, self.total, summary, columns, self.paint.as_ref());
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "{CLEAR_LINE}{text}");
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
pub fn line(
    done: usize,
    total: usize,
    summary: &Summary,
    columns: usize,
    paint: Option<&Paint>,
) -> String {
    let room = columns.saturating_sub(LABEL_WIDTH.saturating_add(1).saturating_add(SPARE_COLUMN));
    if room == 0 {
        return String::new();
    }
    let counts = counts(done, total, summary);
    let width = columns
        .saturating_sub(status::line(HEADER, &counts, Tone::Progress, None).width())
        .saturating_sub(BAR_CHROME)
        .saturating_sub(SPARE_COLUMN)
        .min(MAX_BAR);
    let rest = if width < MIN_BAR {
        fit(&counts, room)
    } else {
        format!("{} {counts}", bar(done, total, width))
    };
    status::line(HEADER, &rest, Tone::Progress, paint)
}

fn fit(text: &str, max: usize) -> String {
    let mut kept = String::new();
    for segment in text.split(SEPARATOR) {
        let candidate = if kept.is_empty() {
            segment.to_owned()
        } else {
            format!("{kept}{SEPARATOR}{segment}")
        };
        if candidate.width() > max {
            break;
        }
        kept = candidate;
    }
    kept
}

fn counts(done: usize, total: usize, summary: &Summary) -> String {
    let fetched = summary.synced.saturating_add(summary.upgraded);
    format!(
        "{done}/{total}: fetched {fetched}{SEPARATOR}plain {}{SEPARATOR}missing {}",
        summary.plain, summary.missing
    )
}

fn bar(done: usize, total: usize, width: usize) -> String {
    let filled = done
        .min(total)
        .saturating_mul(width)
        .checked_div(total)
        .unwrap_or(width);
    let body = if filled == 0 || filled == width {
        "=".repeat(filled)
    } else {
        format!("{}>", "=".repeat(filled.saturating_sub(1)))
    };
    format!("[{body:<width$}]")
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
            counts(7, 100, &summary),
            "7/100: fetched 5 · plain 1 · missing 4"
        );
    }

    #[test]
    fn the_bar_is_empty_at_the_start() {
        assert_eq!(bar(0, 10, 10), "[          ]");
    }

    #[test]
    fn the_bar_tips_its_fill_with_an_arrow() {
        assert_eq!(bar(5, 10, 10), "[====>     ]");
    }

    #[test]
    fn the_bar_is_solid_when_done() {
        assert_eq!(bar(10, 10, 10), "[==========]");
        assert_eq!(bar(12, 10, 10), "[==========]");
        assert_eq!(bar(0, 0, 10), "[==========]");
    }

    #[test]
    fn the_bar_survives_a_huge_count() {
        assert_eq!(bar(usize::MAX - 1, usize::MAX, 10).width(), 12);
    }

    #[test]
    fn the_line_leaves_the_last_column_free() {
        let summary = Summary::default();
        for columns in [70, 80, 120, 200] {
            let text = line(42, 120, &summary, columns, None);
            assert!(text.width() < columns, "{columns}: {text}");
            assert!(text.contains('['), "{columns}: {text}");
        }
    }

    #[test]
    fn the_bar_is_capped_on_a_wide_terminal() {
        let text = line(42, 120, &Summary::default(), 500, None);
        assert!(text.contains(&bar(42, 120, MAX_BAR)), "{text}");
    }

    #[test]
    fn a_narrow_terminal_drops_the_bar() {
        assert_eq!(
            line(42, 120, &Summary::default(), 60, None),
            "    Scanning 42/120: fetched 0 · plain 0 · missing 0"
        );
    }

    #[test]
    fn a_very_narrow_terminal_cuts_the_counts_short_rather_than_wrapping() {
        let text = line(42, 120, &Summary::default(), 40, None);
        assert_eq!(text, "    Scanning 42/120: fetched 0");
        assert!(text.width() < 40);
    }

    #[test]
    fn cutting_the_counts_never_truncates_a_number() {
        let summary = Summary {
            synced: 123,
            ..Summary::default()
        };
        assert_eq!(
            line(42, 120, &summary, 34, None),
            "    Scanning 42/120: fetched 123"
        );
        assert_eq!(line(42, 120, &summary, 32, None), "    Scanning ");
    }

    #[test]
    fn a_terminal_too_narrow_for_the_label_draws_nothing() {
        assert_eq!(line(42, 120, &Summary::default(), 14, None), "");
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
