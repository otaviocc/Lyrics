// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `lyrics ebook`: build an EPUB of the lyrics already sitting beside your music.
//!
//! Read-only and fully offline, in the same class as `stats` and `lint`: this module must never
//! construct an `http::Client` and never call a `sidecar::write_*` function. It reads audio tags
//! and existing sidecars, and writes exactly one file — the book, at the path the user named.
//!
//! On the "never print lyrics to stdout/stderr" invariant (AGENTS.md #3): writing lyrics into a
//! user-named output file is this command's entire purpose and is not what that rule guards
//! against. The standard streams are untouched — logging here stays paths and counts, the same
//! as everywhere else.

pub mod cover;
pub mod epub;
pub mod library;
pub mod lyrics;
pub mod render;

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Result, bail};

use crate::ebook::render::BookInfo;

/// Default book title, used on the cover and as the EPUB's `dc:title`.
pub const DEFAULT_TITLE: &str = "Lyrics";
/// Default `dc:creator`.
pub const DEFAULT_AUTHOR: &str = "Various Artists";
/// Default output path, relative to the working directory.
pub const DEFAULT_OUTPUT: &str = "Lyrics.epub";

/// Resolved options for one `ebook` run.
pub struct BookOptions {
    pub title: String,
    pub author: String,
    pub verbose: u8,
    pub quiet: bool,
}

/// What a run produced, for the closing summary line.
pub struct Summary {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
    /// Tracks listed in a tracklist but carrying no lyrics.
    pub without_lyrics: u32,
    /// Files skipped because their tags had no title or artist.
    pub untagged: u32,
}

impl Summary {
    /// One-line tally, in the shape `runner::Summary::line` uses.
    #[must_use]
    pub fn line(&self) -> String {
        let mut out = format!(
            "{} artists, {} albums, {} songs",
            self.artists, self.albums, self.songs
        );
        if self.without_lyrics > 0 {
            let _ = write!(out, ", {} without lyrics", self.without_lyrics);
        }
        if self.untagged > 0 {
            let _ = write!(out, ", {} untagged", self.untagged);
        }
        out
    }
}

/// Log to stdout at or above verbosity `level`. Paths and counts only, never lyric text.
fn log(opts: &BookOptions, level: u8, message: &str) {
    if !opts.quiet && opts.verbose >= level {
        println!("{message}");
    }
}

/// Build the book for `dir` and write it to `output`.
///
/// # Errors
///
/// Fails when the library yields no lyrics at all (there is no book to write), or when the
/// output file cannot be written.
pub fn build(dir: &Path, output: &Path, opts: &BookOptions) -> Result<Summary> {
    log(opts, 1, &format!("scanning {}", dir.display()));
    let book = library::collect(dir);

    if book.artists.is_empty() {
        bail!(
            "no lyrics found under {} — run `lyrics scan` first to fetch some",
            dir.display()
        );
    }

    for artist in &book.artists {
        for album in &artist.albums {
            log(
                opts,
                1,
                &format!(
                    "album     {} — {} ({}/{} with lyrics)",
                    artist.name,
                    album.title,
                    album.lyric_count(),
                    album.track_count()
                ),
            );
        }
    }

    let info = BookInfo {
        title: opts.title.clone(),
        author: opts.author.clone(),
    };
    let rendered = render::render(
        &book,
        &info,
        |path| cover::thumbnail(path, cover::ART_MAX_EDGE),
        |paths| cover::cover_image(paths, &opts.title),
    );

    log(
        opts,
        1,
        &format!(
            "writing   {} ({} documents, {} images)",
            output.display(),
            rendered.pages.len(),
            rendered.images.len()
        ),
    );
    epub::write(output, &rendered)?;

    Ok(Summary {
        artists: book.artists.len(),
        albums: book.album_count(),
        songs: book.song_count(),
        without_lyrics: book.without_lyrics,
        untagged: book.untagged,
    })
}
