// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The cache of tracks a provider had nothing for, so a re-scan can leave them alone for a while.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    mtime_secs: u64,
    checked_at_secs: u64,
    provider: String,
}

#[derive(Debug, Default)]
pub struct MissingCache {
    path: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
    dirty: bool,
}

impl MissingCache {
    #[must_use]
    pub fn load(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let entries = fs::read_to_string(&path)
            .ok()
            .map_or_else(BTreeMap::new, |contents| parse(&contents, &path));
        Self {
            path: Some(path),
            entries,
            dirty: false,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn is_fresh(
        &self,
        audio: &Path,
        provider: &str,
        retry_after: Duration,
        now: SystemTime,
    ) -> bool {
        let Some(entry) = self.entries.get(&key(audio)) else {
            return false;
        };
        if entry.provider != provider || Some(entry.mtime_secs) != mtime_secs(audio) {
            return false;
        }
        let age = secs_since_epoch(now).saturating_sub(entry.checked_at_secs);
        age < retry_after.as_secs()
    }

    pub fn record_miss(&mut self, audio: &Path, provider: &str, now: SystemTime) {
        let Some(mtime_secs) = mtime_secs(audio) else {
            return;
        };
        let entry = Entry {
            mtime_secs,
            checked_at_secs: secs_since_epoch(now),
            provider: provider.to_owned(),
        };
        self.entries.insert(key(audio), entry);
        self.dirty = true;
    }

    pub fn forget(&mut self, audio: &Path) {
        if self.entries.remove(&key(audio)).is_some() {
            self.dirty = true;
        }
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = self.path.as_deref().filter(|_| self.dirty) else {
            return Ok(());
        };
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("missing.json");
        let tmp_path = dir.join(format!(".{file_name}.tmp"));
        let contents = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&tmp_path, contents)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("failed to replace {}", path.display()))?;
        Ok(())
    }
}

fn parse(contents: &str, path: &Path) -> BTreeMap<String, Entry> {
    serde_json::from_str(contents).unwrap_or_else(|_| {
        eprintln!("lyrics: ignoring unreadable miss cache {}", path.display());
        BTreeMap::new()
    })
}

fn key(audio: &Path) -> String {
    fs::canonicalize(audio)
        .unwrap_or_else(|_| audio.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn mtime_secs(audio: &Path) -> Option<u64> {
    let modified = fs::metadata(audio).ok()?.modified().ok()?;
    Some(secs_since_epoch(modified))
}

fn secs_since_epoch(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[must_use]
pub fn default_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("lyrics").join("missing.json"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("lyrics")
            .join("missing.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const A_DAY: Duration = Duration::from_secs(86_400);

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "audio").unwrap();
        path
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH.checked_add(Duration::from_secs(secs)).unwrap()
    }

    #[test]
    fn a_recorded_miss_is_fresh_until_the_retry_window_passes() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let mut cache = MissingCache::load(None);

        assert!(!cache.is_fresh(&audio, "lrclib", A_DAY, at(1_000)));
        cache.record_miss(&audio, "lrclib", at(1_000));
        assert!(cache.is_fresh(&audio, "lrclib", A_DAY, at(1_000 + 86_399)));
        assert!(!cache.is_fresh(&audio, "lrclib", A_DAY, at(1_000 + 86_400)));
    }

    #[test]
    fn a_miss_from_another_provider_does_not_count() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let mut cache = MissingCache::load(None);
        cache.record_miss(&audio, "lrclib", at(1_000));
        assert!(!cache.is_fresh(&audio, "lrcmux", A_DAY, at(1_000)));
    }

    #[test]
    fn retagging_the_file_invalidates_its_miss() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let mut cache = MissingCache::load(None);
        cache.record_miss(&audio, "lrclib", at(1_000));

        let later = at(2_000_000_000);
        fs::File::options()
            .write(true)
            .open(&audio)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(!cache.is_fresh(&audio, "lrclib", A_DAY, at(1_000)));
    }

    #[test]
    fn forgetting_a_track_makes_it_eligible_again() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let mut cache = MissingCache::load(None);
        cache.record_miss(&audio, "lrclib", at(1_000));
        cache.forget(&audio);
        assert!(!cache.is_fresh(&audio, "lrclib", A_DAY, at(1_000)));
        assert!(cache.is_empty());
    }

    #[test]
    fn a_missing_audio_file_is_never_recorded() {
        let dir = tempdir().unwrap();
        let mut cache = MissingCache::load(None);
        cache.record_miss(&dir.path().join("gone.flac"), "lrclib", at(1_000));
        assert!(cache.is_empty());
    }

    #[test]
    fn the_cache_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let store = dir.path().join("cache").join("missing.json");

        let mut cache = MissingCache::load(Some(store.clone()));
        cache.record_miss(&audio, "lrclib", at(1_000));
        cache.save().unwrap();
        assert!(store.exists());
        assert!(!dir.path().join("cache").join(".missing.json.tmp").exists());

        let reloaded = MissingCache::load(Some(store));
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.is_fresh(&audio, "lrclib", A_DAY, at(1_000)));
    }

    #[test]
    fn saving_an_untouched_cache_writes_nothing() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("missing.json");
        MissingCache::load(Some(store.clone())).save().unwrap();
        assert!(!store.exists());
    }

    #[test]
    fn an_in_memory_cache_never_touches_disk() {
        let dir = tempdir().unwrap();
        let audio = touch(dir.path(), "a.flac");
        let mut cache = MissingCache::load(None);
        cache.record_miss(&audio, "lrclib", at(1_000));
        cache.save().unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_corrupt_cache_file_loads_as_empty_rather_than_failing() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("missing.json");
        fs::write(&store, "{ not json").unwrap();
        let cache = MissingCache::load(Some(store));
        assert!(cache.is_empty());
    }

    #[test]
    fn default_path_prefers_xdg_cache_home_then_falls_back_to_home() {
        unsafe {
            std::env::set_var("HOME", "/home/demo");
            std::env::set_var("XDG_CACHE_HOME", "/xdg-cache");
        }
        assert_eq!(
            default_path(),
            Some(PathBuf::from("/xdg-cache/lyrics/missing.json"))
        );

        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
        }
        assert_eq!(
            default_path(),
            Some(PathBuf::from("/home/demo/.cache/lyrics/missing.json"))
        );
    }
}
