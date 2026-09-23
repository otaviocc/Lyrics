// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The binary: parse the command line, run it, set the exit code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use lyrics::cli::{Cli, Command, Options, SharedOptions};
use lyrics::config::{self, Config};
use lyrics::ebook::{self, BookOptions};
use lyrics::http::{Client, ClientConfig, LyricsRecord};
use lyrics::lrc::{self, Severity};
use lyrics::theme;
use lyrics::tui::{self, app::Song};
use lyrics::{runner, stats};

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn resolve_options(raw: &SharedOptions) -> Result<Options> {
    Ok(raw.resolve(&load_config(raw)?))
}

fn load_config(raw: &SharedOptions) -> Result<Config> {
    if raw.no_config {
        return Ok(Config::default());
    }
    match raw.config.as_deref() {
        Some(path) => {
            if !path.exists() {
                anyhow::bail!("config file not found: {}", path.display());
            }
            config::load(path)
        }
        None => {
            config::default_path().map_or_else(|| Ok(Config::default()), |path| config::load(&path))
        }
    }
}

fn client_for(opts: &Options) -> Client {
    Client::new(ClientConfig {
        provider: opts.provider,
        user_agent: opts.user_agent.clone(),
        delay_ms: opts.delay_ms,
        max_retries: opts.max_retries,
        verbosity: opts.verbose,
    })
}

fn fetch_lyrics(
    track: &str,
    artist: &str,
    album: Option<&str>,
    options: &Options,
) -> Result<Option<LyricsRecord>> {
    let mut client = client_for(options);
    runner::lookup_lyrics(&mut client, track, artist, album, options)
}

fn run_lint(paths: &[PathBuf], strict: bool, quiet: bool) -> bool {
    let (files, skipped) = lrc::resolve_lrc_paths(paths);
    for path in &skipped {
        eprintln!("skip      {}: not an LRC file", path.display());
    }

    let mut files_checked: u32 = 0;
    let mut total_errors: u32 = 0;
    let mut total_warnings: u32 = 0;

    for path in &files {
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) => {
                eprintln!("error     {}: {err}", path.display());
                total_errors = total_errors.saturating_add(1);
                continue;
            }
        };
        files_checked = files_checked.saturating_add(1);

        for diag in lrc::lint(&contents) {
            match diag.severity {
                Severity::Error => total_errors = total_errors.saturating_add(1),
                Severity::Warning => total_warnings = total_warnings.saturating_add(1),
            }
            if quiet {
                continue;
            }
            if diag.line == 0 {
                println!(
                    "{}: {}: {}",
                    path.display(),
                    diag.severity.label(),
                    diag.message
                );
            } else {
                println!(
                    "{}:{}: {}: {}",
                    path.display(),
                    diag.line,
                    diag.severity.label(),
                    diag.message
                );
            }
        }
    }

    if !quiet {
        println!("{files_checked} files checked, {total_errors} errors, {total_warnings} warnings");
    }

    total_errors == 0 && !(strict && total_warnings > 0)
}

fn run_scan(dir: &Path, options: &SharedOptions) -> Result<bool> {
    if !dir.is_dir() {
        anyhow::bail!("{} is not a directory", dir.display());
    }
    let options = resolve_options(options)?;
    let mut client = client_for(&options);
    let summary = runner::scan(&mut client, dir, &options)?;
    if !options.quiet {
        println!("{}", summary.line());
    }
    let total_processed = summary
        .synced
        .saturating_add(summary.upgraded)
        .saturating_add(summary.plain)
        .saturating_add(summary.instrumental)
        .saturating_add(summary.skipped)
        .saturating_add(summary.missing)
        .saturating_add(summary.untagged);
    let all_failed = summary.errors > 0 && total_processed == 0;
    Ok(!all_failed)
}

fn run_ebook(
    dir: &Path,
    output: Option<PathBuf>,
    title: Option<String>,
    author: Option<String>,
    verbose: u8,
    quiet: bool,
) -> Result<bool> {
    if !dir.is_dir() {
        anyhow::bail!("{} is not a directory", dir.display());
    }
    let output = output.unwrap_or_else(|| PathBuf::from(ebook::DEFAULT_OUTPUT));
    let options = BookOptions {
        title: title.unwrap_or_else(|| ebook::DEFAULT_TITLE.to_owned()),
        author: author.unwrap_or_else(|| ebook::DEFAULT_AUTHOR.to_owned()),
        verbose,
        quiet,
    };
    let summary = ebook::build(dir, &output, &options)?;
    if !quiet {
        println!("{}", summary.line());
    }
    Ok(true)
}

fn run_show(
    track: &str,
    artist: &str,
    album: Option<&str>,
    options: &SharedOptions,
) -> Result<bool> {
    let options = resolve_options(options)?;
    let record = fetch_lyrics(track, artist, album, &options)?;
    match record {
        Some(rec) => {
            let text = rec
                .synced_lyrics
                .as_deref()
                .or(rec.plain_lyrics.as_deref())
                .unwrap_or("");
            runner::print_lyrics(text, !options.no_color)?;
        }
        None => {
            eprintln!("No lyrics found for \"{track}\" by {artist}");
        }
    }
    Ok(true)
}

fn run(cli: Cli) -> Result<bool> {
    match cli.command {
        Command::Track { file, options } => {
            let options = resolve_options(&options)?;
            let mut client = client_for(&options);
            let outcome = runner::process_track(&mut client, &file, &options)?;
            if !options.quiet {
                println!("{}", outcome.label());
            }
            Ok(true)
        }
        Command::Scan { dir, options } => run_scan(&dir, &options),
        Command::Stats { dir, verbose } => {
            if !dir.is_dir() {
                anyhow::bail!("{} is not a directory", dir.display());
            }
            let census = stats::collect(&dir);
            print!("{}", census.render());
            if verbose > 0 && !census.orphan_paths.is_empty() {
                println!("\nOrphaned sidecar paths:");
                for path in &census.orphan_paths {
                    println!("  {}", path.display());
                }
            }
            Ok(true)
        }
        Command::Lint {
            paths,
            strict,
            quiet,
        } => Ok(run_lint(&paths, strict, quiet)),
        Command::Ebook {
            dir,
            output,
            title,
            author,
            verbose,
            quiet,
        } => run_ebook(&dir, output, title, author, verbose, quiet),
        Command::Show {
            track,
            artist,
            album,
            options,
        } => run_show(&track, &artist, album.as_deref(), &options),
        Command::Tui {
            track,
            artist,
            album,
            file,
            counter,
            theme,
            list_themes,
            options,
        } => run_tui(&TuiArgs {
            track,
            artist,
            album,
            file,
            counter,
            theme,
            list_themes,
            options,
        }),
    }
}

struct TuiArgs {
    track: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    file: Option<PathBuf>,
    counter: bool,
    theme: Option<String>,
    list_themes: bool,
    options: SharedOptions,
}

fn run_tui(args: &TuiArgs) -> Result<bool> {
    let config = load_config(&args.options)?;
    let config_dir = config::config_dir();
    let theme_name = args.theme.as_deref().or(config.tui.theme.as_deref());

    if args.list_themes {
        print!("{}", theme::loader::list(config_dir.as_deref()));
        return Ok(true);
    }

    let loaded = theme::loader::load(None, theme_name, config_dir.as_deref())?;
    for warning in &loaded.warnings {
        eprintln!("lyrics: {warning}");
    }

    let song = if let Some(path) = &args.file {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let lrc::Synced {
            title,
            artist,
            lines,
        } = lrc::parse_synced(&contents)?;
        let title = title.unwrap_or_else(|| {
            path.file_stem().map_or_else(
                || path.display().to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            )
        });
        Song {
            title,
            artist,
            lines,
        }
    } else {
        let Some(track) = args.track.clone() else {
            anyhow::bail!("a track name is required unless --file or --list-themes is given");
        };
        let Some(artist) = args.artist.clone() else {
            anyhow::bail!("--artist is required alongside a track name");
        };
        let options = args.options.resolve(&config);
        let record = fetch_lyrics(&track, &artist, args.album.as_deref(), &options)?;
        let Some(record) = record else {
            anyhow::bail!("no lyrics found for \"{track}\" by {artist}");
        };
        let Some(text) = record.synced_lyrics.filter(|_| !record.instrumental) else {
            anyhow::bail!("no synced lyrics for \"{track}\" by {artist}");
        };
        let synced = lrc::parse_synced(&text)?;
        Song {
            title: track,
            artist: Some(artist),
            lines: synced.lines,
        }
    };

    tui::run(song, loaded.theme, args.counter)?;
    Ok(true)
}
