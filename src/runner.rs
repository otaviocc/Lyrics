// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Per-track decisions, the scan walk, and the audio-file walk `stats` shares.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

use anyhow::{Context as _, Result};
use walkdir::WalkDir;

use crate::cli::Options;
use crate::http::{Client, LyricsRecord, pick_best_candidate};
use crate::meta::{self, ResolvedMeta, TrackMeta};
use crate::missing::MissingCache;
use crate::progress::Progress;
use crate::sidecar::{self, SidecarState};
use crate::status::{self, Tone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Fetched,
    Upgraded,
    Plain,
    Instrumental,
    Skipped,
    Deferred,
    Missing,
    Untagged,
    NoChange,
}

impl Outcome {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::Upgraded => "upgraded",
            Self::Plain => "plain",
            Self::Instrumental => "instrumental",
            Self::Skipped => "skipped",
            Self::Deferred => "deferred",
            Self::Missing => "missing",
            Self::Untagged => "untagged",
            Self::NoChange => "no-change",
        }
    }

    #[must_use]
    pub const fn verbosity(self) -> u8 {
        match self {
            Self::Fetched | Self::Upgraded | Self::Plain | Self::Instrumental => 0,
            Self::Skipped | Self::Deferred | Self::Missing | Self::Untagged | Self::NoChange => 1,
        }
    }
}

#[must_use]
pub fn report_line(outcome: Outcome, path: &Path, color: bool) -> String {
    status::line(
        outcome.label(),
        &path.display().to_string(),
        Tone::Outcome,
        color,
    )
}

pub fn report(outcome: Outcome, path: &Path, opts: &Options) {
    log(
        opts,
        outcome.verbosity(),
        report_line(
            outcome,
            path,
            status::color_for(opts.no_color, &std::io::stdout()),
        ),
    );
}

pub fn report_error(path: &Path, err: &anyhow::Error, opts: &Options) {
    eprintln!(
        "{}",
        status::line(
            "error",
            &format!("{}: {err:#}", path.display()),
            Tone::Error,
            status::color_for(opts.no_color, &std::io::stderr()),
        )
    );
}

#[derive(Debug, Default)]
pub struct Summary {
    pub synced: u32,
    pub upgraded: u32,
    pub plain: u32,
    pub instrumental: u32,
    pub skipped: u32,
    pub deferred: u32,
    pub missing: u32,
    pub untagged: u32,
    pub errors: u32,
}

impl Summary {
    const fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Fetched => self.synced = self.synced.saturating_add(1),
            Outcome::Upgraded => self.upgraded = self.upgraded.saturating_add(1),
            Outcome::Plain => self.plain = self.plain.saturating_add(1),
            Outcome::Instrumental => self.instrumental = self.instrumental.saturating_add(1),
            Outcome::Skipped => self.skipped = self.skipped.saturating_add(1),
            Outcome::Deferred => self.deferred = self.deferred.saturating_add(1),
            Outcome::Missing => self.missing = self.missing.saturating_add(1),
            Outcome::Untagged => self.untagged = self.untagged.saturating_add(1),
            Outcome::NoChange => {}
        }
    }

    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "synced: {}  upgraded: {}  plain: {}  instrumental: {}  skipped: {}  deferred: {}  missing: {}  untagged: {}  errors: {}",
            self.synced,
            self.upgraded,
            self.plain,
            self.instrumental,
            self.skipped,
            self.deferred,
            self.missing,
            self.untagged,
            self.errors
        )
    }
}

fn log(opts: &Options, level: u8, msg: impl AsRef<str>) {
    if opts.quiet {
        return;
    }
    if opts.verbose >= level {
        println!("{}", msg.as_ref());
    }
}

fn lookup(client: &mut Client, meta: &TrackMeta, opts: &Options) -> Result<Option<LyricsRecord>> {
    let mut record = client.get(meta)?;
    if record.is_none() && !opts.no_search_fallback {
        let candidates = client.search(meta)?;
        record = pick_best_candidate(
            &candidates,
            meta.duration,
            meta.album.as_deref(),
            opts.duration_tolerance,
        );
    }
    Ok(record)
}

pub fn process_track(
    client: &mut Client,
    path: &Path,
    opts: &Options,
    cache: &mut MissingCache,
) -> Result<Outcome> {
    let resolved = meta::resolve(path, opts.path_fallback);
    let meta = match resolved {
        ResolvedMeta::Untagged => return Ok(Outcome::Untagged),
        ResolvedMeta::Ok(m) => m,
    };

    for field in &meta.guessed {
        log(
            opts,
            1,
            format!(
                "  guessed {} from path: {:?}",
                field.label(),
                match field {
                    crate::meta::GuessedField::Title => &meta.title,
                    crate::meta::GuessedField::Artist => &meta.artist,
                    crate::meta::GuessedField::Album => meta.album.as_ref().unwrap_or(&meta.title),
                }
            ),
        );
    }

    let state = sidecar::sidecar_state(path);
    if state == SidecarState::Synced && !opts.force {
        return Ok(Outcome::Skipped);
    }

    let provider = opts.provider.spec().name;
    if !opts.force
        && cache.is_fresh(
            path,
            provider,
            opts.retry_missing_window(),
            SystemTime::now(),
        )
    {
        return Ok(Outcome::Deferred);
    }

    let mut record = lookup(client, &meta, opts)?;

    if record.is_none()
        && !opts.no_marker_fallback
        && let Some(stripped_title) = meta::strip_trailing_markers(&meta.title)
    {
        log(
            opts,
            1,
            format!("  retrying without title markers: {stripped_title:?}"),
        );
        let alt_meta = TrackMeta {
            title: stripped_title,
            ..meta.clone()
        };
        record = lookup(client, &alt_meta, opts)?;
    }

    let Some(record) = record else {
        cache.record_miss(path, provider, SystemTime::now());
        return Ok(Outcome::Missing);
    };
    cache.forget(path);

    if let Some(synced) = record.synced_lyrics.filter(|s| !s.trim().is_empty()) {
        let outcome = if state == SidecarState::Plain {
            Outcome::Upgraded
        } else {
            Outcome::Fetched
        };
        if !opts.dry_run {
            sidecar::write_synced(path, &synced, opts.keep_plain)?;
        }
        return Ok(outcome);
    }

    if let Some(plain) = record.plain_lyrics.filter(|s| !s.trim().is_empty()) {
        if state == SidecarState::Plain && !opts.force {
            return Ok(Outcome::NoChange);
        }
        if !opts.dry_run {
            sidecar::write_plain(path, &plain)?;
        }
        return Ok(Outcome::Plain);
    }

    if record.instrumental {
        if state == SidecarState::None {
            if !opts.dry_run {
                sidecar::write_instrumental_marker_if_absent(path)?;
            }
            return Ok(Outcome::Instrumental);
        }
        return Ok(Outcome::NoChange);
    }

    cache.record_miss(path, provider, SystemTime::now());
    Ok(Outcome::Missing)
}

#[must_use]
pub fn walk_audio_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file() && meta::is_audio_file(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect();
    paths.sort();
    paths
}

pub fn scan(client: &mut Client, dir: &Path, opts: &Options) -> Result<Summary> {
    let mut summary = Summary::default();
    let paths = walk_audio_files(dir);
    let mut progress = Progress::for_scan(opts, paths.len());
    let mut cache = MissingCache::load(opts.missing_cache.clone());

    for path in paths {
        match process_track(client, &path, opts, &mut cache) {
            Ok(outcome) => {
                summary.record(outcome);
                progress.clear();
                report(outcome, &path, opts);
            }
            Err(err) => {
                summary.errors = summary.errors.saturating_add(1);
                progress.clear();
                report_error(&path, &err, opts);
            }
        }
        progress.tick(&summary, Instant::now());
    }
    progress.finish();
    if !opts.dry_run {
        cache.save()?;
    }

    Ok(summary)
}

pub fn lookup_lyrics(
    client: &mut Client,
    track: &str,
    artist: &str,
    album: Option<&str>,
    opts: &Options,
) -> Result<Option<LyricsRecord>> {
    let meta = TrackMeta {
        path: PathBuf::from("<show>"),
        title: track.to_owned(),
        artist: artist.to_owned(),
        album: album.map(str::to_owned),
        duration: None,
        album_artist: None,
        track_number: None,
        disc_number: None,
        year: None,
        guessed: vec![],
    };
    lookup(client, &meta, opts)
}

pub fn print_lyrics(text: &str, color: bool) -> Result<()> {
    let use_color = color && status::color_allowed();

    let output;
    let rendered = if use_color {
        output = colorize_timestamps(text);
        output.as_str()
    } else {
        text
    };

    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".to_owned());
    let mut child = Command::new(&pager)
        .env("LESS", "FRX")
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start pager `{pager}`"))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(rendered.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("pager `{pager}` exited with {status}");
    }
    Ok(())
}

#[allow(clippy::string_slice)]
fn colorize_timestamps(text: &str) -> String {
    const GRAY: &str = "\x1b[38;5;240m";

    let mut out = String::with_capacity(text.len().saturating_add(text.len() / 4));
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[')
            && let Some(close) = trimmed.find(']')
        {
            let lyric = trimmed[close.saturating_add(1)..].trim_start();
            let prefix_len = line.len().saturating_sub(trimmed.len());
            out.push_str(&line[..prefix_len]);
            out.push_str(GRAY);
            out.push_str(&trimmed[..=close]);
            out.push_str(status::RESET);
            if !lyric.is_empty() {
                out.push(' ');
            }
            out.push_str(lyric);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Outcome, colorize_timestamps, report_line};
    use crate::status::RESET;
    use std::path::Path;

    const GRAY: &str = "\x1b[38;5;240m";

    #[test]
    fn report_lines_right_align_the_label_before_the_path() {
        let path = Path::new("a/b.flac");
        assert_eq!(
            report_line(Outcome::Fetched, path, false),
            "     fetched a/b.flac"
        );
        assert_eq!(
            report_line(Outcome::NoChange, path, false),
            "   no-change a/b.flac"
        );
        assert_eq!(
            report_line(Outcome::Instrumental, path, false),
            "instrumental a/b.flac"
        );
    }

    #[test]
    fn only_outcomes_that_wrote_something_print_at_default_verbosity() {
        for outcome in [
            Outcome::Fetched,
            Outcome::Upgraded,
            Outcome::Plain,
            Outcome::Instrumental,
        ] {
            assert_eq!(outcome.verbosity(), 0, "{outcome:?}");
        }
        for outcome in [
            Outcome::Skipped,
            Outcome::Deferred,
            Outcome::Missing,
            Outcome::Untagged,
            Outcome::NoChange,
        ] {
            assert_eq!(outcome.verbosity(), 1, "{outcome:?}");
        }
    }

    #[test]
    fn colorize_inserts_a_space_between_timestamp_and_lyric() {
        let rendered = colorize_timestamps("[00:17.12]Hello world");
        assert_eq!(rendered, format!("{GRAY}[00:17.12]{RESET} Hello world\n"));
    }

    #[test]
    fn colorize_normalizes_leading_whitespace_to_one_space() {
        let rendered = colorize_timestamps("[00:17.12]   Hello world");
        assert_eq!(rendered, format!("{GRAY}[00:17.12]{RESET} Hello world\n"));
    }

    #[test]
    fn colorize_preserves_lines_without_timestamps() {
        let rendered = colorize_timestamps("plain line\n[00:17.12]lyric");
        assert_eq!(
            rendered,
            format!("plain line\n{GRAY}[00:17.12]{RESET} lyric\n")
        );
    }

    #[test]
    fn colorize_blank_lyric_line_gets_no_trailing_space() {
        let rendered = colorize_timestamps("[00:17.12]");
        assert_eq!(rendered, format!("{GRAY}[00:17.12]{RESET}\n"));
    }
}
