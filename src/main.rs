// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use lyrics::cli::{Cli, Command, Options, SharedOptions};
use lyrics::config::{self, Config};
use lyrics::http::{Client, ClientConfig};
use lyrics::lrc::{self, Severity};
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

/// Load the config file `raw` points at (or the default location, unless `--no-config`) and
/// resolve it against the CLI layer. The single place `Track`/`Scan`/`Show` go from raw CLI
/// args to the concrete `Options` the rest of the crate consumes.
///
/// # Errors
///
/// Propagates a config file read/parse error. A missing file at the *default* location is not
/// an error (see `config::load`) — having no config at all is the common case. A missing file
/// at an *explicit* `--config <path>` is an error: the user named that path on purpose, so
/// silently falling back to defaults would mask a typo instead of reporting it.
fn resolve_options(raw: &SharedOptions) -> Result<Options> {
    let config = if raw.no_config {
        Config::default()
    } else {
        match raw.config.as_deref() {
            Some(path) => {
                if !path.exists() {
                    anyhow::bail!("config file not found: {}", path.display());
                }
                config::load(path)?
            }
            None => match config::default_path() {
                Some(path) => config::load(&path)?,
                None => Config::default(),
            },
        }
    };
    Ok(raw.resolve(&config))
}

/// Build an HTTP client from the resolved options.
fn client_for(opts: &Options) -> Client {
    Client::new(ClientConfig {
        provider: opts.provider,
        user_agent: opts.user_agent.clone(),
        delay_ms: opts.delay_ms,
        max_retries: opts.max_retries,
        verbosity: opts.verbose,
    })
}

/// Check every resolved `.lrc` file and print diagnostics. Returns `Ok(false)` (exit 1) when
/// any error was found, or any warning was found under `--strict`.
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

/// Returns `Ok(true)` on overall success, `Ok(false)` if the run completed but every track
/// errored (see plan §4 exit-code rule).
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
        Command::Scan { dir, options } => {
            if !dir.is_dir() {
                anyhow::bail!("{} is not a directory", dir.display());
            }
            let options = resolve_options(&options)?;
            let mut client = client_for(&options);
            let summary = runner::scan(&mut client, &dir, &options)?;
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
        Command::Show {
            track,
            artist,
            album,
            options,
        } => {
            let options = resolve_options(&options)?;
            let mut client = client_for(&options);
            let record =
                runner::lookup_lyrics(&mut client, &track, &artist, album.as_deref(), &options)?;
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
    }
}
