// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The command line, as clap sees it, and its merge with the config file.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::provider::ProviderKind;

pub const DEFAULT_DURATION_TOLERANCE: u32 = 2;
pub const DEFAULT_DELAY_MS: u64 = 300;
pub const DEFAULT_MAX_RETRIES: u32 = 3;

#[derive(Parser, Debug)]
#[command(name = "lyrics", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Walk a directory tree and process every audio file found")]
    Scan {
        #[arg(help = "Root directory to walk recursively")]
        dir: PathBuf,

        #[command(flatten)]
        options: SharedOptions,
    },
    #[command(about = "Process a single audio file")]
    Track {
        #[arg(help = "Path to the audio file")]
        file: PathBuf,

        #[command(flatten)]
        options: SharedOptions,
    },
    #[command(
        about = "Survey a directory tree's lyrics coverage. Read-only: makes no network requests"
    )]
    Stats {
        #[arg(help = "Root directory to walk recursively")]
        dir: PathBuf,

        #[arg(short, long, action = clap::ArgAction::Count, help = "List orphaned sidecar paths instead of just counting them")]
        verbose: u8,
    },
    #[command(about = "Check .lrc files for format and sync problems. Read-only: never writes")]
    Lint {
        #[arg(
            required = true,
            help = "One or more .lrc files, or directories to search recursively"
        )]
        paths: Vec<PathBuf>,

        #[arg(long, help = "Treat warnings as errors")]
        strict: bool,

        #[arg(short, long, help = "Print only the final summary line")]
        quiet: bool,
    },
    #[command(
        about = "Build an EPUB of your library's lyrics. Read-only: makes no network requests"
    )]
    Ebook {
        #[arg(help = "Root directory to walk recursively")]
        dir: PathBuf,

        #[arg(
            short,
            long,
            help = "Destination path for the generated book. [default: ./Lyrics.epub]"
        )]
        output: Option<PathBuf>,

        #[arg(long, help = "Book title, shown on the cover. [default: Lyrics]")]
        title: Option<String>,

        #[arg(
            long,
            help = "Book author, written to the EPUB's metadata. [default: Various Artists]"
        )]
        author: Option<String>,

        #[arg(short, long, action = clap::ArgAction::Count, conflicts_with = "quiet", help = "Print per-album detail")]
        verbose: u8,

        #[arg(
            short,
            long,
            conflicts_with = "verbose",
            help = "Print only the final summary line"
        )]
        quiet: bool,
    },
    #[command(about = "Look up lyrics by artist/track name and display them in a pager")]
    Show {
        #[arg(help = "Track name to look up")]
        track: String,

        #[arg(long, help = "Artist name")]
        artist: String,

        #[arg(long, help = "Album name (optional, refines the search)")]
        album: Option<String>,

        #[command(flatten)]
        options: SharedOptions,
    },
    #[command(
        about = "Follow along with synced lyrics: a full-screen teleprompter, current line centered"
    )]
    Tui {
        #[arg(
            required_unless_present_any = ["file", "list_themes"],
            conflicts_with = "file",
            requires = "artist",
            help = "Track name to look up (omit this with --file or --list-themes)"
        )]
        track: Option<String>,

        #[arg(long, help = "Artist name. Required alongside a track name")]
        artist: Option<String>,

        #[arg(long, help = "Album name (optional, refines the search)")]
        album: Option<String>,

        #[arg(long, conflicts_with_all = ["track", "artist", "album"], help = "Read lyrics from a local .lrc (or .txt-in-LRC-syntax) file instead of fetching them. Stays offline: no provider is queried")]
        file: Option<PathBuf>,

        #[arg(
            long,
            help = "Show a 3, 2, 1, PLAY countdown before the clock starts running"
        )]
        counter: bool,

        #[arg(
            long,
            help = "Theme to use: a bundled name, or one from ~/.config/lyrics/themes/. [default: stage]"
        )]
        theme: Option<String>,

        #[arg(long, help = "List built-in and user themes, then exit")]
        list_themes: bool,

        #[command(flatten)]
        options: SharedOptions,
    },
}

#[derive(Args, Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SharedOptions {
    #[arg(
        long,
        help = "Re-fetch and overwrite even tracks that already have a synced .lrc"
    )]
    pub force: bool,

    #[arg(
        long,
        help = "For files with missing/empty tags, derive metadata from the path (Artist/Album/NN Title.ext) instead of skipping them"
    )]
    pub path_fallback: bool,

    #[arg(
        long,
        help = "Don't fall back to /api/search when /api/get returns 404"
    )]
    pub no_search_fallback: bool,

    #[arg(
        long,
        help = "Don't retry with version markers (e.g. \"(Acoustic)\", \"[Live]\", \"[Bonus Track]\") stripped from the title when the initial lookup finds nothing"
    )]
    pub no_marker_fallback: bool,

    #[arg(
        long,
        help = "Max duration delta (in seconds) accepted for a /api/search candidate. [default: 2]"
    )]
    pub duration_tolerance: Option<u32>,

    #[arg(long, help = "Report planned actions; write nothing to disk")]
    pub dry_run: bool,

    #[arg(
        long,
        help = "Keep the old .txt sidecar after a plain -> synced upgrade"
    )]
    pub keep_plain: bool,

    #[arg(short, long, action = clap::ArgAction::Count, conflicts_with = "quiet", help = "Print per-track detail. Repeat (-vv) to also log request URLs and timings")]
    pub verbose: u8,

    #[arg(
        short,
        long,
        conflicts_with = "verbose",
        help = "Print only the final summary line"
    )]
    pub quiet: bool,

    #[arg(
        long,
        help = "Disable colored output in `show` (timestamps are dimmed by default)"
    )]
    pub no_color: bool,

    #[arg(long, value_enum, help = "Lyrics provider to query. [default: lrclib]")]
    pub provider: Option<ProviderKind>,

    #[arg(
        long,
        help = "Minimum delay between API requests, in milliseconds. [default: 300]"
    )]
    pub delay_ms: Option<u64>,

    #[arg(
        long,
        help = "Maximum retries for 429/5xx responses before giving up on a track. [default: 3]"
    )]
    pub max_retries: Option<u32>,

    #[arg(long, help = "Override the User-Agent sent with every request")]
    pub user_agent: Option<String>,

    #[arg(
        long,
        conflicts_with = "no_config",
        help = "Load config from this path instead of the default location"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        help = "Ignore the config file entirely; use only built-in defaults and CLI flags"
    )]
    pub no_config: bool,
}

impl SharedOptions {
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

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
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
    use crate::config::{Options as ConfigOptions, ProviderConfig, TuiConfig};

    fn config_with(options: ConfigOptions) -> Config {
        Config {
            options,
            lrclib: ProviderConfig::default(),
            lrcmux: ProviderConfig::default(),
            tui: TuiConfig::default(),
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
