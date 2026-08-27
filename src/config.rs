// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Persistent CLI defaults: `~/.config/lyrics/config.toml`.
//!
//! Precedence, defined once in `cli::SharedOptions::resolve`: built-in default -> config file
//! -> CLI flag. This module only loads and parses the file; it has no opinion on how its
//! values get merged with the CLI.
//!
//! `$XDG_CONFIG_HOME/lyrics/config.toml` is honored on every platform (including macOS,
//! rather than `~/Library/Application Support`) so the file lives somewhere a user expects to
//! edit by hand, with no extra platform-detection dependency.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::provider::ProviderKind;

/// Root of `config.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub lrclib: ProviderConfig,
    #[serde(default)]
    pub lrcmux: ProviderConfig,
}

/// The `[options]` table. Every field is optional: an absent key means "use the built-in
/// default, or whatever the CLI says."
///
/// `force` and `dry_run` are deliberately not configurable here: a config that silently
/// forces every run to re-fetch, or silently makes every run a no-op, is a footgun rather
/// than a convenience. `verbose`/`quiet` are omitted too — they conflict with each other in
/// clap and are inherently per-invocation, not persistent preferences.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub provider: Option<ProviderKind>,
    pub delay_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub duration_tolerance: Option<u32>,
    pub user_agent: Option<String>,
    pub path_fallback: Option<bool>,
    pub keep_plain: Option<bool>,
    pub no_search_fallback: Option<bool>,
    pub no_marker_fallback: Option<bool>,
    pub no_color: Option<bool>,
}

/// A `[lrclib]`/`[lrcmux]` table: provider-specific overrides layered on top of `[options]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub user_agent: Option<String>,
}

/// `$XDG_CONFIG_HOME/lyrics/config.toml`, falling back to `$HOME/.config/lyrics/config.toml`.
/// Returns `None` when neither `XDG_CONFIG_HOME` nor `HOME` is set.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("lyrics").join("config.toml"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("lyrics")
            .join("config.toml"),
    )
}

/// Load and parse `path`.
///
/// A missing file yields `Config::default()`, since having no config at all is the common
/// case, not an error; a malformed one is an error, since a typo'd key silently ignored would
/// be worse than a loud failure.
///
/// # Errors
///
/// Returns an error if the file exists but can't be read, or fails to parse as valid TOML
/// matching this shape (an unknown key included).
pub fn load(path: &Path) -> Result<Config> {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempdir().unwrap();
        let config = load(&dir.path().join("does-not-exist.toml")).unwrap();
        assert!(config.options.provider.is_none());
        assert!(config.options.delay_ms.is_none());
    }

    #[test]
    fn parses_the_documented_example() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
            [options]
            provider = "lrclib"
            delay_ms = 500
            path_fallback = true
            keep_plain = true

            [lrclib]
            user_agent = "MyPrivateLyricsBot/1.0"
            "#,
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.options.provider, Some(ProviderKind::Lrclib));
        assert_eq!(config.options.delay_ms, Some(500));
        assert_eq!(config.options.path_fallback, Some(true));
        assert_eq!(config.options.keep_plain, Some(true));
        assert_eq!(
            config.lrclib.user_agent.as_deref(),
            Some("MyPrivateLyricsBot/1.0")
        );
        assert!(config.lrcmux.user_agent.is_none());
    }

    #[test]
    fn unknown_key_is_an_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[options]\nnope = true\n").unwrap();
        assert!(load(&path).is_err());
    }

    /// One test, not two: `std::env::set_var` mutates process-wide state, and `cargo test`
    /// runs tests in the same process concurrently by default, so two tests each poking
    /// `XDG_CONFIG_HOME`/`HOME` independently could race. Exercising both precedence rungs
    /// back-to-back in one test avoids that.
    ///
    /// # Safety
    ///
    /// Per `env::set_var`'s doc caveat: safe here because no other thread in this test binary
    /// reads these two vars, so there's no data race with the sequential mutations below.
    #[test]
    fn default_path_prefers_xdg_config_home_then_falls_back_to_home() {
        unsafe {
            std::env::set_var("HOME", "/home/demo");
            std::env::set_var("XDG_CONFIG_HOME", "/xdg-home");
        }
        assert_eq!(
            default_path(),
            Some(PathBuf::from("/xdg-home/lyrics/config.toml"))
        );

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert_eq!(
            default_path(),
            Some(PathBuf::from("/home/demo/.config/lyrics/config.toml"))
        );
    }
}
