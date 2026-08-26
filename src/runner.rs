// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Per-track orchestration: decide what to do, do it, report it. Also the `scan` walk.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result};
use walkdir::WalkDir;

use crate::cli::SharedOptions;
use crate::http::{Client, LyricsRecord, pick_best_candidate};
use crate::meta::{self, ResolvedMeta, TrackMeta};
use crate::sidecar::{self, SidecarState};

/// What happened when processing a single audio file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Wrote a synced .lrc where nothing existed before.
    Fetched,
    /// Wrote a synced .lrc, replacing a plain .txt.
    Upgraded,
    /// Wrote a plain .txt.
    Plain,
    /// Wrote (or would have written) the instrumental marker.
    Instrumental,
    /// Already had a synced .lrc; nothing to do.
    Skipped,
    /// LRCLIB has nothing for this track.
    Missing,
    /// Title/artist could not be resolved from tags (and path fallback didn't help, or was
    /// disabled).
    Untagged,
    /// A record was found but carried no new information (e.g. still only plain, and we
    /// already have plain).
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
            Self::Missing => "missing",
            Self::Untagged => "untagged",
            Self::NoChange => "no-change",
        }
    }
}

/// Tallies accumulated across a `scan` run, printed as the final summary line.
#[derive(Debug, Default)]
pub struct Summary {
    pub synced: u32,
    pub upgraded: u32,
    pub plain: u32,
    pub instrumental: u32,
    pub skipped: u32,
    pub missing: u32,
    pub untagged: u32,
    pub errors: u32,
}

impl Summary {
    /// Bump the counter for the given outcome.
    const fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Fetched => self.synced = self.synced.saturating_add(1),
            Outcome::Upgraded => self.upgraded = self.upgraded.saturating_add(1),
            Outcome::Plain => self.plain = self.plain.saturating_add(1),
            Outcome::Instrumental => self.instrumental = self.instrumental.saturating_add(1),
            Outcome::Skipped => self.skipped = self.skipped.saturating_add(1),
            Outcome::Missing => self.missing = self.missing.saturating_add(1),
            Outcome::Untagged => self.untagged = self.untagged.saturating_add(1),
            Outcome::NoChange => {}
        }
    }

    /// Format the summary as a single human-readable line.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "synced: {}  upgraded: {}  plain: {}  instrumental: {}  skipped: {}  missing: {}  untagged: {}  errors: {}",
            self.synced,
            self.upgraded,
            self.plain,
            self.instrumental,
            self.skipped,
            self.missing,
            self.untagged,
            self.errors
        )
    }
}

/// Print a message to stdout unless `--quiet` is set and verbosity level is met.
fn log(opts: &SharedOptions, level: u8, msg: impl AsRef<str>) {
    if opts.quiet {
        return;
    }
    if opts.verbose >= level {
        println!("{}", msg.as_ref());
    }
}

/// Try `/api/get` first, then fall back to `/api/search` unless disabled.
fn lookup(
    client: &mut Client,
    meta: &TrackMeta,
    opts: &SharedOptions,
) -> Result<Option<LyricsRecord>> {
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

/// Process a single audio file. Returns the outcome; logs per-track detail per `opts`.
///
/// # Errors
///
/// Propagates I/O errors from sidecar writes and network errors from the lyrics provider.
pub fn process_track(client: &mut Client, path: &Path, opts: &SharedOptions) -> Result<Outcome> {
    let resolved = meta::resolve(path, opts.path_fallback);
    let meta = match resolved {
        ResolvedMeta::Untagged => {
            log(opts, 1, format!("untagged  {}", path.display()));
            return Ok(Outcome::Untagged);
        }
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
        log(opts, 1, format!("skipped   {}", path.display()));
        return Ok(Outcome::Skipped);
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
        log(opts, 1, format!("missing   {}", path.display()));
        return Ok(Outcome::Missing);
    };

    if let Some(synced) = record.synced_lyrics.filter(|s| !s.trim().is_empty()) {
        let outcome = if state == SidecarState::Plain {
            Outcome::Upgraded
        } else {
            Outcome::Fetched
        };
        if !opts.dry_run {
            sidecar::write_synced(path, &synced, opts.keep_plain)?;
        }
        log(
            opts,
            0,
            format!("{:<9} {}", outcome.label(), path.display()),
        );
        return Ok(outcome);
    }

    if let Some(plain) = record.plain_lyrics.filter(|s| !s.trim().is_empty()) {
        if state == SidecarState::Plain && !opts.force {
            log(opts, 1, format!("no-change {}", path.display()));
            return Ok(Outcome::NoChange);
        }
        if !opts.dry_run {
            sidecar::write_plain(path, &plain)?;
        }
        log(opts, 0, format!("plain     {}", path.display()));
        return Ok(Outcome::Plain);
    }

    if record.instrumental {
        if state == SidecarState::None {
            if !opts.dry_run {
                sidecar::write_instrumental_marker_if_absent(path)?;
            }
            log(opts, 0, format!("instrumental {}", path.display()));
            return Ok(Outcome::Instrumental);
        }
        return Ok(Outcome::NoChange);
    }

    log(opts, 1, format!("missing   {}", path.display()));
    Ok(Outcome::Missing)
}

/// Walk `dir` recursively, processing every recognized audio file in deterministic
/// (name-sorted) order, strictly sequentially.
///
/// # Errors
///
/// Propagates the first network or I/O error that cannot be recovered from; per-track errors
/// are caught and counted in the returned [`Summary`].
pub fn scan(client: &mut Client, dir: &Path, opts: &SharedOptions) -> Result<Summary> {
    let mut summary = Summary::default();

    let mut entries: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file() && meta::is_audio_file(e.path()))
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in entries {
        match process_track(client, entry.path(), opts) {
            Ok(outcome) => summary.record(outcome),
            Err(err) => {
                summary.errors = summary.errors.saturating_add(1);
                eprintln!("error     {}: {err:#}", entry.path().display());
            }
        }
    }

    Ok(summary)
}

/// Look up lyrics by artist/track name without an audio file.
///
/// Constructs a synthetic [`TrackMeta`] and delegates to the standard lookup pipeline.
///
/// # Errors
///
/// Propagates network errors from the lyrics provider.
pub fn lookup_lyrics(
    client: &mut Client,
    track: &str,
    artist: &str,
    album: Option<&str>,
    opts: &SharedOptions,
) -> Result<Option<LyricsRecord>> {
    let meta = TrackMeta {
        path: PathBuf::from("<show>"),
        title: track.to_owned(),
        artist: artist.to_owned(),
        album: album.map(str::to_owned),
        duration: None,
        guessed: vec![],
    };
    lookup(client, &meta, opts)
}

/// Display `text` in a terminal pager ($PAGER, or `less`).
///
/// When `color` is `true`, LRC timestamps like `[00:17.12]` are rendered in dark gray so the
/// lyrics text stands out. Respects the `NO_COLOR` environment variable.
///
/// # Errors
///
/// Returns an error if the pager cannot be spawned or exits with a non-zero status, or if
/// writing to the pager's stdin fails.
pub fn print_lyrics(text: &str, color: bool) -> Result<()> {
    let use_color = color && std::env::var_os("NO_COLOR").is_none();

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

/// Wrap LRC timestamps (`[MM:SS.xx]`) in a dark-gray ANSI color.
#[allow(clippy::string_slice)] // All slices are derived from `find(']')` and `trim_start`, valid boundaries.
fn colorize_timestamps(text: &str) -> String {
    const GRAY: &str = "\x1b[38;5;240m";
    const RESET: &str = "\x1b[0m";

    let mut out = String::with_capacity(text.len().saturating_add(text.len() / 4));
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[')
            && let Some(close) = trimmed.find(']')
        {
            let prefix_len = line.len().saturating_sub(trimmed.len());
            // Prefix (leading whitespace) + dimmed timestamp + reset + rest of line
            out.push_str(&line[..prefix_len]);
            out.push_str(GRAY);
            out.push_str(&trimmed[..=close]);
            out.push_str(RESET);
            out.push_str(&trimmed[close.saturating_add(1)..]);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
