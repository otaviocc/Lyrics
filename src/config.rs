// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Loading `~/.config/lyrics/config.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::provider::ProviderKind;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub lrclib: ProviderConfig,
    #[serde(default)]
    pub lrcmux: ProviderConfig,
    #[serde(default)]
    pub tui: TuiConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuiConfig {
    pub theme: Option<String>,
}

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

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub user_agent: Option<String>,
}

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

#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    default_path()?.parent().map(Path::to_path_buf)
}

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
