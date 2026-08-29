// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `stats`, `lint`, and `ebook` are read-only: they never construct an `http::Client` and never
//! write into the music tree. These tests drive all three against small fixture trees and assert
//! nothing changed, following the same pattern as `read_only_guarantee.rs` (link the `lyrics`
//! lib crate, `tempfile::tempdir()`, snapshot before/after).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use lyrics::ebook::{self, BookOptions};
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
        let _ = lrc::lint(&contents);
    }

    let after = snapshot(tmp.path());
    assert_eq!(before, after, "no file's length or mtime changed");
    assert_eq!(
        after.len(),
        before_entry_count,
        "no file was created or deleted"
    );
}

/// A real tagged FLAC, needed because `ebook` groups by tags and skips anything untagged.
const FIXTURE_FLAC: &str = "tests/fixtures/sample.flac";

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn ebook_never_modifies_the_music_tree() {
    let music = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();

    // A tagged track with a synced sidecar, so the book has something to put on a page.
    let audio = music.path().join("01 Sample.flac");
    fs::copy(FIXTURE_FLAC, &audio).expect("fixture flac");
    fs::write(
        music.path().join("01 Sample.lrc"),
        "[ar:Test Artist]\n[00:01.00]Hello\n[00:02.00]World\n",
    )
    .unwrap();
    // A second track with no sidecar: it must appear in the tracklist without being written to.
    fs::copy(FIXTURE_FLAC, music.path().join("02 Silent.flac")).expect("fixture flac");

    let before = snapshot(music.path());
    let before_entry_count = before.len();

    // The output deliberately lands outside the scanned tree: the book is the only file this
    // command may create, and it must never appear beside the music.
    let output = out_dir.path().join("Lyrics.epub");
    let options = BookOptions {
        title: "Lyrics".to_owned(),
        author: "Various Artists".to_owned(),
        verbose: 0,
        quiet: true,
    };
    let summary = ebook::build(music.path(), &output, &options).expect("build the book");

    assert_eq!(summary.artists, 1);
    assert_eq!(summary.albums, 1);
    assert_eq!(
        summary.songs, 1,
        "only the track with a sidecar gets a page"
    );
    assert_eq!(
        summary.without_lyrics, 1,
        "the sidecar-less track is still listed on the album"
    );
    assert!(output.exists(), "the book was written");

    let after = snapshot(music.path());
    assert_eq!(before, after, "no file's length or mtime changed");
    assert_eq!(
        after.len(),
        before_entry_count,
        "no file was created or deleted in the music tree"
    );
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn ebook_reports_an_error_when_there_is_nothing_to_put_in_a_book() {
    let music = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    // Tagged audio, but no sidecar anywhere: there is no book to write.
    fs::copy(FIXTURE_FLAC, music.path().join("01 Sample.flac")).expect("fixture flac");

    let output = out_dir.path().join("Lyrics.epub");
    let options = BookOptions {
        title: "Lyrics".to_owned(),
        author: "Various Artists".to_owned(),
        verbose: 0,
        quiet: true,
    };
    assert!(ebook::build(music.path(), &output, &options).is_err());
    assert!(!output.exists(), "no file is created when the build fails");
}
