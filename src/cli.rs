// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::provider::ProviderKind;

/// Fetch synced/plain lyrics from LRCLIB or lrcmux and write them as sidecar files next to your
/// music.
///
/// This tool never reads embedded metadata destructively and never writes to audio files.
/// See AGENTS.md for the read-only guarantee.
#[derive(Parser, Debug)]
#[command(name = "lyrics", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Walk a directory tree and process every audio file found.
    Scan {
        /// Root directory to walk recursively.
        dir: PathBuf,

        #[command(flatten)]
        options: SharedOptions,
    },
    /// Process a single audio file.
    Track {
        /// Path to the audio file.
        file: PathBuf,

        #[command(flatten)]
        options: SharedOptions,
    },
    /// Look up lyrics by artist/track name and display them in a pager.
    Show {
        /// Track name to look up.
        track: String,

        /// Artist name.
        #[arg(long)]
        artist: String,

        /// Album name (optional, refines the search).
        #[arg(long)]
        album: Option<String>,

        #[command(flatten)]
        options: SharedOptions,
    },
}

/// Options shared by the `track`, `scan`, and `show` subcommands.
#[derive(Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct SharedOptions {
    // --- Selection -----------------------------------------------------
    /// Re-fetch and overwrite even tracks that already have a synced .lrc.
    #[arg(long)]
    pub force: bool,

    /// For files with missing/empty tags, derive metadata from the path
    /// (Artist/Album/NN Title.ext) instead of skipping them.
    #[arg(long)]
    pub path_fallback: bool,

    /// Don't fall back to /api/search when /api/get returns 404.
    #[arg(long)]
    pub no_search_fallback: bool,

    /// Don't retry with version markers (e.g. "(Acoustic)", "[Live]", "[Bonus Track]") stripped
    /// from the title when the initial lookup finds nothing.
    #[arg(long)]
    pub no_marker_fallback: bool,

    /// Max duration delta (in seconds) accepted for a /api/search candidate.
    #[arg(long, default_value_t = 2)]
    pub duration_tolerance: u32,

    // --- Output ----------------------------------------------------------
    /// Report planned actions; write nothing to disk.
    #[arg(long)]
    pub dry_run: bool,

    /// Keep the old .txt sidecar after a plain -> synced upgrade.
    #[arg(long)]
    pub keep_plain: bool,

    /// Print per-track detail. Repeat (-vv) to also log request URLs and timings.
    #[arg(short, long, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Print only the final summary line.
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    // --- Network -----------------------------------------------------------
    /// Lyrics provider to query.
    #[arg(long, value_enum, default_value_t = ProviderKind::Lrclib)]
    pub provider: ProviderKind,

    /// Minimum delay between API requests, in milliseconds.
    #[arg(long, default_value_t = 300)]
    pub delay_ms: u64,

    /// Maximum retries for 429/5xx responses before giving up on a track.
    #[arg(long, default_value_t = 3)]
    pub max_retries: u32,

    /// Override the User-Agent sent with every request.
    #[arg(long)]
    pub user_agent: Option<String>,
}
