// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use lyrics::cli::{Cli, Command, SharedOptions};
use lyrics::http::{Client, ClientConfig};
use lyrics::runner;

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

/// Build an HTTP client from the shared CLI options.
fn client_for(opts: &SharedOptions) -> Client {
    Client::new(ClientConfig {
        provider: opts.provider,
        user_agent: opts.user_agent.clone(),
        delay_ms: opts.delay_ms,
        max_retries: opts.max_retries,
        verbosity: opts.verbose,
    })
}

/// Returns `Ok(true)` on overall success, `Ok(false)` if the run completed but every track
/// errored (see plan §4 exit-code rule).
fn run(cli: Cli) -> Result<bool> {
    match cli.command {
        Command::Track { file, options } => {
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
            let mut client = client_for(&options);
            let summary = runner::scan(&mut client, &dir, &options)?;
            if !options.quiet {
                println!("{}", summary.line());
            }
            let total_processed = summary.synced
                + summary.upgraded
                + summary.plain
                + summary.instrumental
                + summary.skipped
                + summary.missing
                + summary.untagged;
            let all_failed = summary.errors > 0 && total_processed == 0;
            Ok(!all_failed)
        }
    }
}
