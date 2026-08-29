// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Validate a generated book against the EPUB specification with `epubcheck`.
//!
//! The check is **optional**: `epubcheck` is a JVM tool that not every machine (or CI runner)
//! has, so when it isn't on `PATH` this test reports that it skipped and passes. When it is
//! installed, a spec violation fails the build. Nothing here touches the network — `epubcheck`
//! runs entirely offline against a local file.
//!
//! Install it with `brew install epubcheck` (or your platform's equivalent).

use std::path::Path;
use std::process::Command;

use lyrics::ebook::{self, BookOptions};

/// A real tagged FLAC; `ebook` groups by tags, so untagged files would produce an empty book.
const FIXTURE_FLAC: &str = "tests/fixtures/sample.flac";

/// Is `epubcheck` available to run?
fn epubcheck_available() -> bool {
    Command::new("epubcheck")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Write a small library that exercises the parts of the renderer most likely to emit invalid
/// markup: XML metacharacters and non-ASCII text in lyrics, an instrumental marker, a track
/// with no sidecar at all, and a real image for the album art and cover collage to consume.
#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
fn build_library(dir: &Path) {
    std::fs::copy(FIXTURE_FLAC, dir.join("01 Track.flac")).expect("fixture flac");
    std::fs::write(
        dir.join("01 Track.lrc"),
        // Every character that has to be escaped to keep the XHTML well-formed, plus text
        // outside ASCII, plus a metadata tag and a break entry that must be dropped.
        "[ar:Test & Artist]\n\
         [00:01.00]Ampersand & angle < brackets >\n\
         [00:02.00]Quotes \"double\" and 'single'\n\
         [00:03.00]Accents: naïve café — em dash, ünïcøde\n\
         [00:04.00]\n\
         [00:05.00]After a stanza break\n",
    )
    .unwrap();

    // An instrumental: renders as a label in the tracklist, with no page of its own.
    std::fs::copy(FIXTURE_FLAC, dir.join("02 Instrumental.flac")).expect("fixture flac");
    std::fs::write(
        dir.join("02 Instrumental.lrc"),
        lyrics::sidecar::INSTRUMENTAL_MARKER,
    )
    .unwrap();

    // No sidecar: listed on the album, but contributes no document.
    std::fs::copy(FIXTURE_FLAC, dir.join("03 Silent.flac")).expect("fixture flac");

    // Album art, so the manifest carries image entries and a cover collage is built.
    let art = image::RgbImage::from_fn(64, 64, |x, y| {
        image::Rgb([
            u8::try_from(x.saturating_mul(4) % 256).unwrap_or(0),
            u8::try_from(y.saturating_mul(4) % 256).unwrap_or(0),
            128,
        ])
    });
    art.save(dir.join("folder.png")).expect("write album art");
}

#[allow(clippy::expect_used, clippy::unwrap_used)] // Test file; panicking on failure is fine.
#[test]
fn the_generated_book_passes_epubcheck() {
    let music = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    build_library(music.path());

    let output = out_dir.path().join("Lyrics.epub");
    let options = BookOptions {
        // An ampersand in the title too: it reaches dc:title, the cover, and every page's
        // <title> element.
        title: "Lyrics & Verses".to_owned(),
        author: "Various Artists".to_owned(),
        verbose: 0,
        quiet: true,
    };
    ebook::build(music.path(), &output, &options).expect("build the book");
    assert!(output.exists());

    if !epubcheck_available() {
        println!(
            "skipping EPUB validation: `epubcheck` is not on PATH (install it with \
             `brew install epubcheck`)"
        );
        return;
    }

    let result = Command::new("epubcheck")
        .arg(&output)
        .output()
        .expect("run epubcheck");

    assert!(
        result.status.success(),
        "epubcheck rejected the generated book\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}
