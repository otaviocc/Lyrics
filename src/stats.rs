// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Read-only coverage census for a directory tree: `lyrics stats <dir>`.
//!
//! Walks the tree exactly like `scan` (via `runner::walk_audio_files`) and calls
//! `sidecar::sidecar_detail` per file, but never constructs an `http::Client` and never
//! writes anything. Safe to run as often as you like; it makes no network requests.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::meta;
use crate::sidecar::{self, SidecarDetail};

/// Coverage census of a directory tree, as counted by `collect`.
#[derive(Debug, Default)]
pub struct Stats {
    pub total: u32,
    pub synced: u32,
    pub instrumental: u32,
    pub plain: u32,
    pub missing: u32,
    /// Lowercased audio extension (no dot) -> count, e.g. `"flac" -> 800`.
    pub by_extension: BTreeMap<String, u32>,
    /// `.lrc`/`.txt` sidecars with no same-stem audio file in the same directory.
    pub orphan_paths: Vec<PathBuf>,
}

impl Stats {
    /// Number of orphaned sidecars found.
    #[must_use]
    pub fn orphan_count(&self) -> u32 {
        u32::try_from(self.orphan_paths.len()).unwrap_or(u32::MAX)
    }

    /// Render the census as the human-readable block printed by `lyrics stats`.
    #[must_use]
    #[allow(clippy::missing_panics_doc)] // write! to a String is infallible.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Total tracks: {:>8}", thousands(self.total));
        let _ = writeln!(out);

        for (label, count) in [
            ("Synced", self.synced),
            ("Instrumental", self.instrumental),
            ("Plain", self.plain),
            ("Missing", self.missing),
        ] {
            let _ = writeln!(
                out,
                "{label:<13}{:>8}  ({}%)",
                thousands(count),
                percentage(count, self.total)
            );
        }

        if !self.by_extension.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "By format:");
            let mut entries: Vec<_> = self.by_extension.iter().collect();
            entries.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            for (ext, count) in entries {
                let _ = writeln!(out, "  .{ext:<8}{:>8}", thousands(*count));
            }
        }

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Orphaned sidecars: {:>4}",
            thousands(self.orphan_count())
        );

        out
    }
}

/// Does `path` look like a lyrics sidecar (`.lrc` or `.txt`), case-insensitively?
fn is_sidecar_file(path: &Path) -> bool {
    meta::has_extension(path, &["lrc", "txt"])
}

/// Bump the counter for `key` in `map` by one, saturating rather than overflowing.
fn bump(map: &mut BTreeMap<String, u32>, key: String) {
    map.entry(key)
        .and_modify(|n| *n = n.saturating_add(1))
        .or_insert(1);
}

/// Walk `dir` and build a coverage census. Read-only: never writes, never queries a provider.
///
/// A single pass classifies each file as it's visited (audio vs. sidecar vs. neither), rather
/// than walking the tree once per category, since on a large library the second traversal's
/// I/O would roughly double `stats`'s cost for no benefit. Orphan matching lowercases every
/// stem before comparing, so it stays case-insensitive without assuming the filesystem is.
#[must_use]
pub fn collect(dir: &Path) -> Stats {
    let mut stats = Stats::default();

    let mut audio_stems_by_dir: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut sidecar_candidates: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        if meta::is_audio_file(path) {
            stats.total = stats.total.saturating_add(1);

            match sidecar::sidecar_detail(path) {
                SidecarDetail::Synced => stats.synced = stats.synced.saturating_add(1),
                SidecarDetail::Instrumental => {
                    stats.instrumental = stats.instrumental.saturating_add(1);
                }
                SidecarDetail::Plain => stats.plain = stats.plain.saturating_add(1),
                SidecarDetail::None => stats.missing = stats.missing.saturating_add(1),
            }

            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                bump(&mut stats.by_extension, ext.to_lowercase());
            }

            if let (Some(parent), Some(stem)) =
                (path.parent(), path.file_stem().and_then(|s| s.to_str()))
            {
                audio_stems_by_dir
                    .entry(parent.to_path_buf())
                    .or_default()
                    .insert(stem.to_lowercase());
            }
        } else if is_sidecar_file(path) {
            sidecar_candidates.push(path.to_path_buf());
        }
    }

    for path in sidecar_candidates {
        let has_audio_sibling = path.parent().is_some_and(|parent| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| {
                    audio_stems_by_dir
                        .get(parent)
                        .is_some_and(|stems| stems.contains(&stem.to_lowercase()))
                })
        });
        if !has_audio_sibling {
            stats.orphan_paths.push(path);
        }
    }
    stats.orphan_paths.sort();

    stats
}

/// Render `n` with thousands separators, e.g. `1240 -> "1,240"`.
#[allow(clippy::arithmetic_side_effects)] // Index math over a short digit string; can't overflow.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// `count` as an integer percentage of `total`, rounded to the nearest whole number.
/// Returns 0 when `total` is 0 rather than dividing by zero.
#[allow(clippy::arithmetic_side_effects)] // `total` is checked nonzero before the divide.
fn percentage(count: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    let count = u64::from(count);
    let total = u64::from(total);
    let scaled = count.saturating_mul(100).saturating_add(total / 2);
    u32::try_from(scaled / total).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn thousands_groups_correctly() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(9), "9");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_240), "1,240");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn percentage_rounds_and_avoids_division_by_zero() {
        assert_eq!(percentage(0, 0), 0);
        assert_eq!(percentage(980, 1_240), 79);
        assert_eq!(percentage(1, 3), 33);
        assert_eq!(percentage(2, 3), 67);
        assert_eq!(percentage(10, 10), 100);
    }

    #[test]
    fn empty_dir_has_zero_total_and_no_panic() {
        let dir = tempdir().unwrap();
        let stats = collect(dir.path());
        assert_eq!(stats.total, 0);
        assert!(stats.render().contains("Total tracks:"));
    }

    #[test]
    fn counts_synced_plain_instrumental_and_missing() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("01 Synced.flac"), "audio");
        write(
            &dir.path().join("01 Synced.lrc"),
            "[00:01.00]Hello\n[00:02.00]World\n",
        );

        write(&dir.path().join("02 Plain.flac"), "audio");
        write(&dir.path().join("02 Plain.txt"), "Hello\nWorld\n");

        write(&dir.path().join("03 NoTimestamps.flac"), "audio");
        write(
            &dir.path().join("03 NoTimestamps.lrc"),
            "[ar:Some Artist]\nHello\n",
        );

        write(&dir.path().join("04 Instrumental.flac"), "audio");
        write(
            &dir.path().join("04 Instrumental.lrc"),
            sidecar::INSTRUMENTAL_MARKER,
        );

        write(&dir.path().join("05 Missing.mp3"), "audio");

        let stats = collect(dir.path());
        assert_eq!(stats.total, 5);
        assert_eq!(stats.synced, 1);
        assert_eq!(stats.instrumental, 1);
        assert_eq!(stats.plain, 2); // .txt + the timestamp-less .lrc
        assert_eq!(stats.missing, 1);
        assert_eq!(stats.by_extension.get("flac"), Some(&4));
        assert_eq!(stats.by_extension.get("mp3"), Some(&1));
        assert!(stats.orphan_paths.is_empty());
    }

    #[test]
    fn detects_orphaned_sidecar() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("01 Track.flac"), "audio");
        write(&dir.path().join("01 Track.lrc"), "[00:01.00]Hi\n");
        write(&dir.path().join("02 Deleted.lrc"), "[00:01.00]Hi\n"); // no audio sibling

        let stats = collect(dir.path());
        assert_eq!(stats.orphan_count(), 1);
        assert_eq!(
            stats.orphan_paths.first().and_then(|p| p.file_name()),
            Some(std::ffi::OsStr::new("02 Deleted.lrc"))
        );
    }

    #[test]
    fn orphan_check_is_case_insensitive_on_extension_and_stem() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("Track.FLAC"), "audio");
        write(&dir.path().join("track.lrc"), "[00:01.00]Hi\n");

        let stats = collect(dir.path());
        assert!(stats.orphan_paths.is_empty());
    }
}
