// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! LRC parsing and linting: `lyrics lint <path>...`.
//!
//! This is the crate's first real LRC parser; previously the only LRC-syntax awareness was
//! `sidecar::is_timestamp_line`, a shallow prefix check used purely to decide synced-vs-plain.
//! `parse_line` here is a superset of that check (it also classifies metadata tags, comments,
//! and multi-stamp lines), but `sidecar::is_timestamp_line` is left as-is: it is a proven,
//! narrowly-scoped function backed by its own tests, and this module has no need to touch it.
//!
//! Read-only, like `stats`: `lint` never writes a file and never queries a provider.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// Metadata keys recognized by the LRC format (case-insensitive). `offset` gets its own
/// value-format check in `lint`; the rest are free-form.
const KNOWN_METADATA_KEYS: &[&str] = &[
    "ti", "ar", "al", "au", "by", "length", "offset", "re", "ve", "tool",
];

/// How serious a `Diagnostic` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// Human-readable label used in diagnostic output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// A single problem found in an `.lrc` file. Never carries lyric text (invariant 3, see
/// `AGENTS.md`) — only positions, tags, and structural descriptions.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// 1-indexed source line. `0` for a file-level diagnostic not tied to one line.
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

/// A parsed LRC timestamp tag, e.g. `[01:23.45]`, kept in its as-written form (digit counts)
/// as well as its numeric value, so `lint` can flag non-canonical formatting separately from
/// out-of-range values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub mins: u32,
    pub secs: u32,
    /// Numeric value of the fractional part, e.g. `45` for `.45`. `0` when absent.
    pub frac: u32,
    /// Number of digits in the fractional part, `0` when absent.
    pub frac_digits: u8,
    /// Number of digits used to write the minutes field.
    pub min_digits: u8,
}

impl Timestamp {
    /// Total milliseconds since the start of the track, computed straight from the parsed
    /// digits. Deliberately does not clamp `secs` to the canonical 0..60 range — flagging an
    /// out-of-range value is `lint`'s job, not this function's job to silently paper over.
    /// Returns `None` on overflow (an absurdly large minutes field).
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

/// One classified line of an `.lrc` file, as returned by `parse_line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line<'a> {
    Blank,
    /// A `# ...` comment line.
    Comment(&'a str),
    /// A single `[key:value]` tag, e.g. `[ar:Some Artist]`.
    Metadata {
        key: &'a str,
        value: &'a str,
    },
    /// One or more leading timestamp tags followed by the lyric text.
    Timed {
        stamps: Vec<Timestamp>,
        /// Everything after the last timestamp tag. Blank (or whitespace-only) makes this a
        /// *break entry* rather than a lyric: a marker telling a player when to stop
        /// displaying the previous line. LRCLIB ends most of its synced records with one.
        text: &'a str,
    },
    /// Non-blank text with no leading `[` at all.
    Untimed(&'a str),
    /// Starts with `[` but isn't a valid timestamp or metadata tag.
    Malformed(&'a str),
}

/// Parse a `key:value` tag body into a metadata tag, if `key` looks like a metadata key
/// (letters only) rather than the digit run a timestamp would start with.
fn parse_metadata(tag: &str) -> Option<(&str, &str)> {
    let (key, value) = tag.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((key, value.trim()))
}

/// Parse a bracket tag body as a timestamp: `MM:SS`, `MM:SS.x`, `MM:SS.xx`, or `MM:SS.xxx`
/// (and looser digit counts on every field — `lint` flags non-canonical forms, this function
/// only rejects what isn't a timestamp shape at all).
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

/// Classify a single line of an `.lrc` file.
#[must_use]
#[allow(clippy::string_slice)] // `close` comes from `find(']')` on ASCII brackets: always a char boundary.
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

/// Check `contents` (an `.lrc` file's text) and return every problem found.
///
/// Diagnostics come back in line order, most-relevant first within each line.
///
/// Break entries (see [`Line::Timed`]'s `text`) are exempt from the duplicate-timestamp
/// check, and are not recorded as seen either: sharing a timestamp with the line they
/// terminate is exactly what they are for, so neither direction is a duplicate.
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

/// Does `path` have an `.lrc` extension (case-insensitive)?
fn is_lrc_file(path: &Path) -> bool {
    crate::meta::has_extension(path, &["lrc"])
}

/// Resolve `lyrics lint`'s path arguments into a sorted, deduplicated list of `.lrc` files.
///
/// A directory is walked recursively for `.lrc` files; a file argument is included directly
/// if it has an `.lrc` extension, otherwise it's returned in the second list so the caller
/// can report it as skipped rather than silently ignoring it.
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

    /// A timed line with blank text is an LRC break entry, not a lyric: it marks when the
    /// previous line should stop being displayed. Sharing a timestamp with a real line is
    /// legitimate, so it must not be reported as a duplicate.
    #[test]
    fn break_entry_sharing_a_timestamp_is_not_a_duplicate() {
        let diags = lint("[00:01.00]A\n[00:01.00]\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// The other half of the fix: a break entry must not be *recorded* either, or the real
    /// lyric line that follows it at the same timestamp gets flagged instead.
    #[test]
    fn break_entry_does_not_poison_a_later_real_line() {
        let diags = lint("[00:01.00]\n[00:01.00]A\n");
        assert!(
            !diags.iter().any(|d| d.message.contains("duplicate")),
            "{diags:?}"
        );
    }

    /// The shape LRCLIB actually returns, and what prompted the fix: lyric lines followed by
    /// a trailing break entry at the same timestamp as the last one.
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
        assert!(skipped.is_empty()); // directory walk silently skips non-.lrc, no report needed

        let txt = dir.path().join("notes.txt");
        let (files, skipped) = resolve_lrc_paths(std::slice::from_ref(&txt));
        assert!(files.is_empty());
        assert_eq!(skipped, vec![txt]);
    }
}
