// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `stats` and `lint` are read-only: they never construct an `http::Client` and never write
//! to disk. This test drives both against a small fixture tree and asserts nothing changed,
//! following the same pattern as `read_only_guarantee.rs` (link the `lyrics` lib crate,
//! `tempfile::tempdir()`, snapshot before/after).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use lyrics::{lrc, stats};

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
fn snapshot(dir: &Path) -> BTreeMap<std::path::PathBuf, (u64, SystemTime)> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let meta = entry.metadata().expect("metadata");
        out.insert(
            entry.path().to_path_buf(),
            (meta.len(), meta.modified().expect("mtime")),
        );
    }
    out
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
fn build_fixture_tree(dir: &Path) {
    fs::write(dir.join("01 Synced.flac"), b"not really audio").unwrap();
    fs::write(
        dir.join("01 Synced.lrc"),
        "[00:01.00]Hello\n[00:02.00]World\n",
    )
    .unwrap();

    fs::write(dir.join("02 Plain.mp3"), b"not really audio").unwrap();
    fs::write(dir.join("02 Plain.txt"), "Hello\nWorld\n").unwrap();

    fs::write(dir.join("03 Missing.flac"), b"not really audio").unwrap();

    fs::write(dir.join("04 Orphan.lrc"), "[00:01.00]No audio sibling\n").unwrap();

    fs::write(
        dir.join("05 Broken.lrc"),
        "[00:05.00]Later\n[00:01.00]Earlier\n[not a valid tag\n",
    )
    .unwrap();
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn stats_and_lint_never_modify_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    build_fixture_tree(tmp.path());

    let before = snapshot(tmp.path());
    let before_entry_count = before.len();

    let census = stats::collect(tmp.path());
    assert_eq!(census.total, 3, "stats::collect should see 3 audio files");
    assert_eq!(
        census.orphan_count(),
        2,
        "04 Orphan.lrc and 05 Broken.lrc both lack an audio sibling"
    );

    let (lrc_files, skipped) = lrc::resolve_lrc_paths(&[tmp.path().to_path_buf()]);
    assert_eq!(lrc_files.len(), 3, "expect 01/04/05's .lrc files");
    assert!(skipped.is_empty(), "a directory walk reports no skips");

    for path in &lrc_files {
        let contents = fs::read_to_string(path).unwrap();
        let _ = lrc::lint(&contents, 60);
    }

    let after = snapshot(tmp.path());
    assert_eq!(before, after, "no file's length or mtime changed");
    assert_eq!(
        after.len(),
        before_entry_count,
        "no file was created or deleted"
    );
}
