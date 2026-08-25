// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Per-track orchestration: decide what to do, do it, report it. Also the `scan` walk.

use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::cli::SharedOptions;
use crate::http::{Client, LyricsRecord, pick_best_candidate};
use crate::meta::{self, ResolvedMeta, TrackMeta};
use crate::sidecar::{self, SidecarState};

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
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Fetched => "fetched",
            Outcome::Upgraded => "upgraded",
            Outcome::Plain => "plain",
            Outcome::Instrumental => "instrumental",
            Outcome::Skipped => "skipped",
            Outcome::Missing => "missing",
            Outcome::Untagged => "untagged",
            Outcome::NoChange => "no-change",
        }
    }
}

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
    fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Fetched => self.synced += 1,
            Outcome::Upgraded => self.upgraded += 1,
            Outcome::Plain => self.plain += 1,
            Outcome::Instrumental => self.instrumental += 1,
            Outcome::Skipped => self.skipped += 1,
            Outcome::Missing => self.missing += 1,
            Outcome::Untagged => self.untagged += 1,
            Outcome::NoChange => {}
        }
    }

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

fn log(opts: &SharedOptions, level: u8, msg: impl AsRef<str>) {
    if opts.quiet {
        return;
    }
    if opts.verbose >= level {
        println!("{}", msg.as_ref());
    }
}

/// `/api/get`, falling back to `/api/search` (unless disabled) when it 404s.
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

    // A version marker like "(Acoustic)"/"[Live]"/"[Bonus Track]" tacked onto the title makes
    // both /api/get and /api/search fail to find an otherwise-identical record (verified
    // against the live API). Retry once with any trailing bracketed group(s) stripped.
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

    // Synced first (always), then plain, then the instrumental flag, so a record that carries
    // both a flag and real lyrics still yields lyrics (see runner design, plan section 4).
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
pub fn scan(client: &mut Client, dir: &Path, opts: &SharedOptions) -> Result<Summary> {
    let mut summary = Summary::default();

    let mut entries: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && meta::is_audio_file(e.path()))
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in entries {
        match process_track(client, entry.path(), opts) {
            Ok(outcome) => summary.record(outcome),
            Err(err) => {
                summary.errors += 1;
                eprintln!("error     {}: {err:#}", entry.path().display());
            }
        }
    }

    Ok(summary)
}
