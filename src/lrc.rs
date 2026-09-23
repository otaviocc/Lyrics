// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! LRC parsing: the checks `lyrics lint` runs and the timeline `lyrics tui` plays.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use walkdir::WalkDir;

const KNOWN_METADATA_KEYS: &[&str] = &[
    "ti", "ar", "al", "au", "by", "length", "offset", "re", "ve", "tool",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    const fn error(line: usize, message: String) -> Self {
        Self {
            line,
            severity: Severity::Error,
            message,
        }
    }

    const fn warning(line: usize, message: String) -> Self {
        Self {
            line,
            severity: Severity::Warning,
            message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub mins: u32,
    pub secs: u32,
    pub frac: u32,
    pub frac_digits: u8,
    pub min_digits: u8,
}

impl Timestamp {
    #[must_use]
    pub fn millis(&self) -> Option<u32> {
        let frac_ms = match self.frac_digits {
            0 => 0,
            1 => self.frac.checked_mul(100)?,
            2 => self.frac.checked_mul(10)?,
            3 => self.frac,
            4 => self.frac.checked_div(10)?,
            5 => self.frac.checked_div(100)?,
            _ => self.frac.checked_div(1000)?,
        };
        self.mins
            .checked_mul(60_000)?
            .checked_add(self.secs.checked_mul(1000)?)?
            .checked_add(frac_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line<'a> {
    Blank,
    Comment(&'a str),
    Metadata {
        key: &'a str,
        value: &'a str,
    },
    Timed {
        stamps: Vec<Timestamp>,
        text: &'a str,
    },
    Untimed(&'a str),
    Malformed(&'a str),
}

fn parse_metadata(tag: &str) -> Option<(&str, &str)> {
    let (key, value) = tag.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((key, value.trim()))
}

fn parse_timestamp(tag: &str) -> Option<Timestamp> {
    let (mins_str, rest) = tag.split_once(':')?;
    if mins_str.is_empty() || !mins_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (secs_str, frac_str) = rest
        .split_once('.')
        .map_or((rest, None), |(s, f)| (s, Some(f)));
    if secs_str.is_empty() || !secs_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac_str
        && (f.is_empty() || f.chars().count() > 6 || !f.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }

    let mins: u32 = mins_str.parse().ok()?;
    let secs: u32 = secs_str.parse().ok()?;
    let min_digits = u8::try_from(mins_str.chars().count()).ok()?;
    let (frac, frac_digits) = match frac_str {
        Some(f) => (f.parse().ok()?, u8::try_from(f.chars().count()).ok()?),
        None => (0, 0),
    };

    Some(Timestamp {
        mins,
        secs,
        frac,
        frac_digits,
        min_digits,
    })
}

#[must_use]
#[allow(clippy::string_slice)]
pub fn parse_line(line: &str) -> Line<'_> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Line::Blank;
    }
    if let Some(rest) = trimmed.strip_prefix('#') {
        return Line::Comment(rest.trim_start());
    }
    if !trimmed.starts_with('[') {
        return Line::Untimed(trimmed);
    }

    let mut stamps = Vec::new();
    let mut rest = trimmed;
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some(close) = after_open.find(']') else {
            return Line::Malformed(trimmed);
        };
        let tag = &after_open[..close];

        if let Some(ts) = parse_timestamp(tag) {
            stamps.push(ts);
            rest = &after_open[close.saturating_add(1)..];
            continue;
        }

        if stamps.is_empty()
            && let Some((key, value)) = parse_metadata(tag)
        {
            return Line::Metadata { key, value };
        }

        return Line::Malformed(trimmed);
    }

    Line::Timed { stamps, text: rest }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn lint(contents: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut seen_timed_line = false;
    let mut any_timed_line = false;
    let mut last_millis: Option<u32> = None;
    let mut seen_stamps: HashMap<u32, usize> = HashMap::new();

    for (idx, raw_line) in contents.lines().enumerate() {
        let line_no = idx.saturating_add(1);
        match parse_line(raw_line) {
            Line::Blank | Line::Comment(_) => {}

            Line::Metadata { key, value } => {
                if seen_timed_line {
                    diags.push(Diagnostic::warning(
                        line_no,
                        "metadata tag appears after the first timed line".to_owned(),
                    ));
                }
                if !KNOWN_METADATA_KEYS
                    .iter()
                    .any(|k| key.eq_ignore_ascii_case(k))
                {
                    diags.push(Diagnostic::warning(
                        line_no,
                        format!("unknown metadata key `{key}`"),
                    ));
                }
                if key.eq_ignore_ascii_case("offset") && value.parse::<i64>().is_err() {
                    diags.push(Diagnostic::error(
                        line_no,
                        "`offset` value must be a signed integer".to_owned(),
                    ));
                }
            }

            Line::Untimed(_) => {
                if seen_timed_line {
                    diags.push(Diagnostic::warning(
                        line_no,
                        "untimed text line mixed in among timed lines".to_owned(),
                    ));
                }
            }

            Line::Malformed(_) => {
                diags.push(Diagnostic::error(
                    line_no,
                    "malformed timestamp or metadata tag".to_owned(),
                ));
            }

            Line::Timed { stamps, text } => {
                any_timed_line = true;
                seen_timed_line = true;
                let is_break_entry = text.trim().is_empty();
                let mut first_millis = None;

                for ts in &stamps {
                    if ts.secs >= 60 {
                        diags.push(Diagnostic::error(
                            line_no,
                            "seconds field must be less than 60".to_owned(),
                        ));
                    }
                    if ts.frac_digits != 2 {
                        diags.push(Diagnostic::warning(
                            line_no,
                            "non-canonical timestamp precision, expected [MM:SS.xx]".to_owned(),
                        ));
                    }
                    if ts.min_digits < 2 {
                        diags.push(Diagnostic::warning(
                            line_no,
                            "minutes not zero-padded, expected [MM:SS.xx]".to_owned(),
                        ));
                    }

                    match ts.millis() {
                        Some(millis) => {
                            if first_millis.is_none() {
                                first_millis = Some(millis);
                            }
                            if !is_break_entry {
                                if let Some(&first_seen) = seen_stamps.get(&millis) {
                                    diags.push(Diagnostic::warning(
                                        line_no,
                                        format!(
                                            "duplicate timestamp, first seen on line {first_seen}"
                                        ),
                                    ));
                                } else {
                                    seen_stamps.insert(millis, line_no);
                                }
                            }
                        }
                        None => diags.push(Diagnostic::error(
                            line_no,
                            "timestamp value out of range".to_owned(),
                        )),
                    }
                }

                if let Some(millis) = first_millis {
                    if let Some(prev) = last_millis
                        && millis < prev
                    {
                        diags.push(Diagnostic::error(
                            line_no,
                            "timestamp is earlier than the previous line's".to_owned(),
                        ));
                    }
                    last_millis = Some(millis);
                }
            }
        }
    }

    if !any_timed_line {
        diags.push(Diagnostic::warning(
            0,
            "no timed lines found; file has no sync data".to_owned(),
        ));
    }

    diags
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedLine {
    pub at_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Synced {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub lines: Vec<SyncedLine>,
}

const fn apply_offset(at_ms: u64, offset_ms: i64) -> u64 {
    if offset_ms >= 0 {
        at_ms.saturating_sub(offset_ms.unsigned_abs())
    } else {
        at_ms.saturating_add(offset_ms.unsigned_abs())
    }
}

pub fn parse_synced(contents: &str) -> Result<Synced> {
    let mut title = None;
    let mut artist = None;
    let mut offset_ms: i64 = 0;
    let mut lines = Vec::new();

    for raw_line in contents.lines() {
        match parse_line(raw_line) {
            Line::Metadata { key, value } => {
                if key.eq_ignore_ascii_case("ti") {
                    title = Some(value.to_owned());
                } else if key.eq_ignore_ascii_case("ar") {
                    artist = Some(value.to_owned());
                } else if key.eq_ignore_ascii_case("offset")
                    && let Ok(parsed) = value.parse::<i64>()
                {
                    offset_ms = parsed;
                }
            }
            Line::Timed { stamps, text } => {
                let text = text.trim();
                for stamp in &stamps {
                    if let Some(millis) = stamp.millis() {
                        lines.push(SyncedLine {
                            at_ms: apply_offset(u64::from(millis), offset_ms),
                            text: text.to_owned(),
                        });
                    }
                }
            }
            Line::Blank | Line::Comment(_) | Line::Untimed(_) | Line::Malformed(_) => {}
        }
    }

    if lines.is_empty() {
        bail!("no synced lyrics: file has no timed lines");
    }

    lines.sort_by_key(|line| line.at_ms);
    Ok(Synced {
        title,
        artist,
        lines,
    })
}

fn is_lrc_file(path: &Path) -> bool {
    crate::meta::has_extension(path, &["lrc"])
}

#[must_use]
pub fn resolve_lrc_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = BTreeSet::new();
    let mut skipped = Vec::new();

    for path in paths {
        if path.is_dir() {
            for entry in WalkDir::new(path)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file() && is_lrc_file(entry.path()) {
                    files.insert(entry.path().to_path_buf());
                }
            }
        } else if is_lrc_file(path) {
            files.insert(path.clone());
        } else {
            skipped.push(path.clone());
        }
    }

    (files.into_iter().collect(), skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn severities(diags: &[Diagnostic]) -> Vec<(usize, Severity)> {
        diags.iter().map(|d| (d.line, d.severity)).collect()
    }

    #[test]
    fn parses_blank_comment_and_untimed_lines() {
        assert_eq!(parse_line(""), Line::Blank);
        assert_eq!(parse_line("   "), Line::Blank);
        assert_eq!(
            parse_line("# generated by X"),
            Line::Comment("generated by X")
        );
        assert_eq!(parse_line("just lyrics"), Line::Untimed("just lyrics"));
    }

    #[test]
    fn parses_metadata_tag() {
        assert_eq!(
            parse_line("[ar:Some Artist]"),
            Line::Metadata {
                key: "ar",
                value: "Some Artist"
            }
        );
    }

    #[test]
    fn parses_single_timed_line() {
        let Line::Timed { stamps, text } = parse_line("[00:12.34]Hello") else {
            panic!("expected Timed");
        };
        assert_eq!(stamps.len(), 1);
        assert_eq!(text, "Hello");
        assert_eq!(stamps[0].millis(), Some(12_340));
    }

    #[test]
    fn parses_multi_stamp_line() {
        let Line::Timed { stamps, text } = parse_line("[00:01.00][01:23.45]Same words") else {
            panic!("expected Timed");
        };
        assert_eq!(stamps.len(), 2);
        assert_eq!(text, "Same words");
    }

    #[test]
    fn malformed_bracket_is_reported() {
        assert!(matches!(parse_line("[not a tag"), Line::Malformed(_)));
        assert!(matches!(parse_line("[12x34]oops"), Line::Malformed(_)));
    }

    #[test]
    fn offset_tag_accepts_negative_integers() {
        assert_eq!(
            parse_line("[offset:-500]"),
            Line::Metadata {
                key: "offset",
                value: "-500"
            }
        );
    }

    #[test]
    fn instrumental_marker_lints_clean() {
        let diags = lint("[00:00.00]Instrumental\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn clean_synced_file_lints_clean() {
        let contents = "[ar:Some Artist]\n[ti:Some Title]\n\
                         [00:01.00]First line\n[00:05.50]Second line\n[00:12.00]Third line\n";
        let diags = lint(contents);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn flags_out_of_order_timestamps() {
        let diags = lint("[00:10.00]Later\n[00:05.00]Earlier\n");
        assert_eq!(severities(&diags), vec![(2, Severity::Error)]);
    }

    #[test]
    fn flags_seconds_out_of_range() {
        let diags = lint("[00:75.00]Oops\n");
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error && d.line == 1)
        );
    }

    #[test]
    fn flags_duplicate_timestamps() {
        let diags = lint("[00:01.00]A\n[00:01.00]B\n");
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("duplicate"))
        );
    }

    #[test]
    fn break_entry_sharing_a_timestamp_is_not_a_duplicate() {
        let diags = lint("[00:01.00]A\n[00:01.00]\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn break_entry_does_not_poison_a_later_real_line() {
        let diags = lint("[00:01.00]\n[00:01.00]A\n");
        assert!(
            !diags.iter().any(|d| d.message.contains("duplicate")),
            "{diags:?}"
        );
    }

    #[test]
    fn realistic_lrclib_tail_lints_clean() {
        let contents = "[04:43.70]There's gonna be Hell\n                         [04:51.97]There's gonna be Hell.\n                         [04:51.97]\n";
        let diags = lint(contents);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn flags_non_canonical_precision_and_padding() {
        let diags = lint("[0:01]No fraction, one digit minute\n");
        assert_eq!(
            diags
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count(),
            2
        );
    }

    #[test]
    fn flags_unknown_metadata_key() {
        let diags = lint("[xy:whatever]\n");
        assert!(diags.iter().any(|d| d.message.contains("unknown metadata")));
    }

    #[test]
    fn flags_metadata_after_timed_line() {
        let diags = lint("[00:01.00]Hi\n[ar:Late Artist]\n");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("after the first timed line"))
        );
    }

    #[test]
    fn flags_untimed_line_among_timed_lines() {
        let diags = lint("[00:01.00]Hi\nstray text\n[00:02.00]Bye\n");
        assert!(
            diags
                .iter()
                .any(|d| d.line == 2 && d.message.contains("untimed"))
        );
    }

    #[test]
    fn flags_bad_offset_value() {
        let diags = lint("[offset:not-a-number]\n");
        assert!(diags.iter().any(|d| d.message.contains("signed integer")));
    }

    #[test]
    fn warns_when_no_timed_lines_at_all() {
        let diags = lint("[ar:Artist]\njust some text\n");
        assert!(
            diags
                .iter()
                .any(|d| d.line == 0 && d.message.contains("no timed lines"))
        );
    }

    #[test]
    fn resolve_lrc_paths_walks_dirs_and_flags_non_lrc_files() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.lrc"), "[00:01.00]Hi\n").unwrap();
        fs::write(dir.path().join("notes.txt"), "not lrc\n").unwrap();

        let (files, skipped) = resolve_lrc_paths(&[dir.path().to_path_buf()]);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.lrc"));
        assert!(skipped.is_empty());

        let txt = dir.path().join("notes.txt");
        let (files, skipped) = resolve_lrc_paths(std::slice::from_ref(&txt));
        assert!(files.is_empty());
        assert_eq!(skipped, vec![txt]);
    }

    #[test]
    fn parse_synced_sorts_multi_stamp_lines_into_the_timeline() {
        let synced = parse_synced(
            "[ti:Some Title]\n[ar:Some Artist]\n\
             [00:05.00][01:00.00]Repeated\n[00:01.00]First\n",
        )
        .unwrap();
        assert_eq!(synced.title.as_deref(), Some("Some Title"));
        assert_eq!(synced.artist.as_deref(), Some("Some Artist"));
        let at: Vec<u64> = synced.lines.iter().map(|line| line.at_ms).collect();
        assert_eq!(at, vec![1_000, 5_000, 60_000]);
    }

    #[test]
    fn parse_synced_applies_a_positive_offset_by_pushing_timestamps_later() {
        let synced = parse_synced("[offset:500]\n[00:01.00]Hi\n").unwrap();
        assert_eq!(synced.lines[0].at_ms, 500);
    }

    #[test]
    fn parse_synced_applies_a_negative_offset_by_pulling_timestamps_earlier() {
        let synced = parse_synced("[offset:-500]\n[00:01.00]Hi\n").unwrap();
        assert_eq!(synced.lines[0].at_ms, 1_500);
    }

    #[test]
    fn parse_synced_saturates_offset_at_zero_rather_than_underflowing() {
        let synced = parse_synced("[offset:5000]\n[00:01.00]Hi\n").unwrap();
        assert_eq!(synced.lines[0].at_ms, 0);
    }

    #[test]
    fn parse_synced_keeps_break_entries_as_blank_lines() {
        let synced = parse_synced("[00:01.00]Hi\n[00:02.00]\n").unwrap();
        assert_eq!(synced.lines.len(), 2);
        assert_eq!(synced.lines[1].text, "");
    }

    #[test]
    fn parse_synced_ignores_untimed_and_malformed_lines() {
        let synced = parse_synced("[00:01.00]Hi\nstray text\n[not a tag\n[00:02.00]Bye\n").unwrap();
        assert_eq!(synced.lines.len(), 2);
    }

    #[test]
    fn parse_synced_errors_on_plain_only_lyrics() {
        assert!(parse_synced("just some plain text\nwith no timestamps\n").is_err());
    }

    #[test]
    fn parse_synced_errors_on_empty_input() {
        assert!(parse_synced("").is_err());
    }
}
