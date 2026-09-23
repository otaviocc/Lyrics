// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `lyrics ebook`: collect the library, render it, write one EPUB.

pub mod cover;
pub mod epub;
pub mod library;
pub mod lyrics;
pub mod render;

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Result, bail};

use crate::ebook::render::BookInfo;

pub const DEFAULT_TITLE: &str = "Lyrics";
pub const DEFAULT_AUTHOR: &str = "Various Artists";
pub const DEFAULT_OUTPUT: &str = "Lyrics.epub";

pub struct BookOptions {
    pub title: String,
    pub author: String,
    pub verbose: u8,
    pub quiet: bool,
}

pub struct Summary {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
    pub without_lyrics: u32,
    pub untagged: u32,
}

impl Summary {
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

fn log(opts: &BookOptions, level: u8, message: &str) {
    if !opts.quiet && opts.verbose >= level {
        println!("{message}");
    }
}

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
