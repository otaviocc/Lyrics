// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::provider::ProviderKind;

/// Built-in default for `--duration-tolerance`, used when neither the CLI nor the config
/// file sets it.
pub const DEFAULT_DURATION_TOLERANCE: u32 = 2;
/// Built-in default for `--delay-ms`.
pub const DEFAULT_DELAY_MS: u64 = 300;
/// Built-in default for `--max-retries`.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

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
    /// Survey a directory tree's lyrics coverage. Read-only: makes no network requests.
    Stats {
        /// Root directory to walk recursively.
        dir: PathBuf,

        /// List orphaned sidecar paths instead of just counting them.
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
    /// Check .lrc files for format and sync problems. Read-only: never writes.
    Lint {
        /// One or more .lrc files, or directories to search recursively.
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,

        /// Warn when the gap between consecutive timestamps exceeds this many seconds
        /// (0 disables the check).
        #[arg(long, default_value_t = 60)]
        max_gap: u32,

        /// Print only the final summary line.
        #[arg(short, long)]
        quiet: bool,
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

/// Raw CLI layer of the options shared by the `track`, `scan`, and `show` subcommands.
///
/// Value options (`duration_tolerance`, `provider`, `delay_ms`, `max_retries`, `user_agent`)
/// are `Option<T>` here rather than carrying a `default_value_t`, so `SharedOptions::resolve`
/// can tell "not passed on the CLI" apart from "explicitly set to the built-in default" and
/// layer the config file in between. Boolean flags stay plain `bool`: clap can't distinguish
/// "absent" from "false" for those, so they merge with the config file by OR instead (see
/// `resolve`) — a flag turned on in the config can't be turned back off from the CLI.
#[derive(Args, Debug, Clone, Default)]
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

    /// Max duration delta (in seconds) accepted for a /api/search candidate. [default: 2]
    #[arg(long)]
    pub duration_tolerance: Option<u32>,

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

    /// Disable colored output in `show` (timestamps are dimmed by default).
    #[arg(long)]
    pub no_color: bool,

    // --- Network -----------------------------------------------------------
    /// Lyrics provider to query. [default: lrclib]
    #[arg(long, value_enum)]
    pub provider: Option<ProviderKind>,

    /// Minimum delay between API requests, in milliseconds. [default: 300]
    #[arg(long)]
    pub delay_ms: Option<u64>,

    /// Maximum retries for 429/5xx responses before giving up on a track. [default: 3]
    #[arg(long)]
    pub max_retries: Option<u32>,

    /// Override the User-Agent sent with every request.
    #[arg(long)]
    pub user_agent: Option<String>,

    // --- Config file -------------------------------------------------------
    /// Load config from this path instead of the default location.
    #[arg(long, conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore the config file entirely; use only built-in defaults and CLI flags.
    #[arg(long)]
    pub no_config: bool,
}

impl SharedOptions {
    /// Resolve the raw CLI layer against a loaded `Config`, applying the crate's one
    /// precedence rule: built-in default -> config file -> CLI flag.
    ///
    /// Value options: the first of `self.field`, `config.options.field`, and the built-in
    /// default that's present, in that order. Boolean flags: `self.field || config value`
    /// (see the struct doc for why). `user_agent` additionally checks the config's
    /// provider-specific table (`[lrclib]`/`[lrcmux]`) ahead of `[options].user_agent`, since
    /// a per-provider override is more specific than a blanket one.
    #[must_use]
    pub fn resolve(&self, config: &Config) -> Options {
        let provider = self
            .provider
            .or(config.options.provider)
            .unwrap_or(ProviderKind::Lrclib);

        let provider_user_agent = match provider {
            ProviderKind::Lrclib => config.lrclib.user_agent.as_deref(),
            ProviderKind::Lrcmux => config.lrcmux.user_agent.as_deref(),
        };

        Options {
            force: self.force,
            path_fallback: self.path_fallback || config.options.path_fallback.unwrap_or(false),
            no_search_fallback: self.no_search_fallback
                || config.options.no_search_fallback.unwrap_or(false),
            no_marker_fallback: self.no_marker_fallback
                || config.options.no_marker_fallback.unwrap_or(false),
            duration_tolerance: self
                .duration_tolerance
                .or(config.options.duration_tolerance)
                .unwrap_or(DEFAULT_DURATION_TOLERANCE),
            dry_run: self.dry_run,
            keep_plain: self.keep_plain || config.options.keep_plain.unwrap_or(false),
            verbose: self.verbose,
            quiet: self.quiet,
            no_color: self.no_color || config.options.no_color.unwrap_or(false),
            provider,
            delay_ms: self
                .delay_ms
                .or(config.options.delay_ms)
                .unwrap_or(DEFAULT_DELAY_MS),
            max_retries: self
                .max_retries
                .or(config.options.max_retries)
                .unwrap_or(DEFAULT_MAX_RETRIES),
            user_agent: self
                .user_agent
                .clone()
                .or_else(|| provider_user_agent.map(str::to_owned))
                .or_else(|| config.options.user_agent.clone()),
        }
    }
}

/// Fully resolved options: what `runner` and `http::Client` actually consume.
///
/// Every field is concrete (no `Option`, except `user_agent` which has no built-in default to
/// fall back to). Built by `SharedOptions::resolve`, the single place precedence between the
/// built-in default, the config file, and the CLI is defined.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // Mirrors `SharedOptions`; see its own allow for why.
pub struct Options {
    pub force: bool,
    pub path_fallback: bool,
    pub no_search_fallback: bool,
    pub no_marker_fallback: bool,
    pub duration_tolerance: u32,
    pub dry_run: bool,
    pub keep_plain: bool,
    pub verbose: u8,
    pub quiet: bool,
    pub no_color: bool,
    pub provider: ProviderKind,
    pub delay_ms: u64,
    pub max_retries: u32,
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Options as ConfigOptions, ProviderConfig};

    fn config_with(options: ConfigOptions) -> Config {
        Config {
            options,
            lrclib: ProviderConfig::default(),
            lrcmux: ProviderConfig::default(),
        }
    }

    #[test]
    fn default_wins_when_nothing_is_set() {
        let resolved = SharedOptions::default().resolve(&Config::default());
        assert_eq!(resolved.duration_tolerance, DEFAULT_DURATION_TOLERANCE);
        assert_eq!(resolved.delay_ms, DEFAULT_DELAY_MS);
        assert_eq!(resolved.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(resolved.provider, ProviderKind::Lrclib);
        assert!(!resolved.path_fallback);
        assert!(resolved.user_agent.is_none());
    }

    #[test]
    fn config_beats_default() {
        let config = config_with(ConfigOptions {
            delay_ms: Some(500),
            path_fallback: Some(true),
            ..ConfigOptions::default()
        });
        let resolved = SharedOptions::default().resolve(&config);
        assert_eq!(resolved.delay_ms, 500);
        assert!(resolved.path_fallback);
    }

    #[test]
    fn cli_beats_config() {
        let config = config_with(ConfigOptions {
            delay_ms: Some(500),
            ..ConfigOptions::default()
        });
        let cli = SharedOptions {
            delay_ms: Some(1_000),
            ..SharedOptions::default()
        };
        assert_eq!(cli.resolve(&config).delay_ms, 1_000);
    }

    #[test]
    fn boolean_flag_set_in_config_alone_resolves_true() {
        let config = config_with(ConfigOptions {
            keep_plain: Some(true),
            ..ConfigOptions::default()
        });
        assert!(SharedOptions::default().resolve(&config).keep_plain);
    }

    #[test]
    fn boolean_flag_set_on_the_cli_alone_resolves_true() {
        let cli = SharedOptions {
            keep_plain: true,
            ..SharedOptions::default()
        };
        assert!(cli.resolve(&Config::default()).keep_plain);
    }

    #[test]
    fn provider_specific_user_agent_beats_general_options_user_agent() {
        let mut config = config_with(ConfigOptions {
            user_agent: Some("general".to_owned()),
            ..ConfigOptions::default()
        });
        config.lrclib.user_agent = Some("lrclib-specific".to_owned());

        let resolved = SharedOptions::default().resolve(&config);
        assert_eq!(resolved.user_agent.as_deref(), Some("lrclib-specific"));
    }

    #[test]
    fn cli_user_agent_beats_every_config_source() {
        let mut config = config_with(ConfigOptions {
            user_agent: Some("general".to_owned()),
            ..ConfigOptions::default()
        });
        config.lrclib.user_agent = Some("lrclib-specific".to_owned());

        let cli = SharedOptions {
            user_agent: Some("cli".to_owned()),
            ..SharedOptions::default()
        };
        assert_eq!(cli.resolve(&config).user_agent.as_deref(), Some("cli"));
    }
}
