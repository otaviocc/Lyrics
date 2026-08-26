// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Verification step 3 (plan): audio files must never be modified. This test drives the real
//! tag-reading path (`meta::resolve`, which is what `process_track` calls before ever touching
//! the network) against a real tagged fixture file and asserts the file's length and mtime are
//! byte-identical afterwards, and that nothing but a `.lrc`/`.txt` sidecar can appear next to
//! it.
//!
//! This deliberately does not invoke the network client. It isolates the read path, which is
//! the one capable of touching the audio file at all. Sidecar-writing is covered independently
//! by the write_* tests in src/sidecar.rs, all of which write only to `.lrc`/`.txt` paths.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use lyrics::meta;

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
fn fixture_copy(tmp: &Path) -> std::path::PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.flac");
    let dst = tmp.join("01 Test Track.flac");
    fs::copy(&src, &dst).expect("copy fixture");
    dst
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn reading_tags_never_modifies_the_audio_file() {
    let tmp = tempfile::tempdir().unwrap();
    let audio = fixture_copy(tmp.path());

    let before_len = fs::metadata(&audio).unwrap().len();
    let before_mtime = fs::metadata(&audio).unwrap().modified().unwrap();

    // Exercise the real resolution path twice (default, then with path fallback), same as a
    // `scan` would for a track it's already seen once and is re-processing on a later run.
    let _ = meta::resolve(&audio, false);
    let _ = meta::resolve(&audio, true);

    let after_len = fs::metadata(&audio).unwrap().len();
    let after_mtime = fs::metadata(&audio).unwrap().modified().unwrap();

    assert_eq!(before_len, after_len, "audio file length changed");
    assert_eq!(before_mtime, after_mtime, "audio file mtime changed");

    // The directory must contain only the audio file: resolving metadata writes nothing.
    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries.len(), 1);

    // Sanity: mtime really is a meaningful signal on this filesystem.
    assert!(before_mtime <= SystemTime::now());
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn resolved_metadata_matches_embedded_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let audio = fixture_copy(tmp.path());

    let resolved = meta::resolve(&audio, false);
    match resolved {
        meta::ResolvedMeta::Ok(m) => {
            assert_eq!(m.title, "Test Track");
            assert_eq!(m.artist, "Test Artist");
            assert_eq!(m.album.as_deref(), Some("Test Album"));
            assert_eq!(m.duration, Some(2));
        }
        meta::ResolvedMeta::Untagged => panic!("expected tags to resolve from the fixture"),
    }
}
