// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Reading a track's metadata never modifies the audio file.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use lyrics::meta;

#[allow(clippy::expect_used, clippy::unwrap_used)]
fn fixture_copy(tmp: &Path) -> std::path::PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.flac");
    let dst = tmp.join("01 Test Track.flac");
    fs::copy(&src, &dst).expect("copy fixture");
    dst
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[test]
fn reading_tags_never_modifies_the_audio_file() {
    let tmp = tempfile::tempdir().unwrap();
    let audio = fixture_copy(tmp.path());

    let before_len = fs::metadata(&audio).unwrap().len();
    let before_mtime = fs::metadata(&audio).unwrap().modified().unwrap();

    let _ = meta::resolve(&audio, false);
    let _ = meta::resolve(&audio, true);

    let after_len = fs::metadata(&audio).unwrap().len();
    let after_mtime = fs::metadata(&audio).unwrap().modified().unwrap();

    assert_eq!(before_len, after_len, "audio file length changed");
    assert_eq!(before_mtime, after_mtime, "audio file mtime changed");

    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries.len(), 1);

    assert!(before_mtime <= SystemTime::now());
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
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
