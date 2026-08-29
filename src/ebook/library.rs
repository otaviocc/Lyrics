// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Turn a directory tree into the book's model: artists holding albums holding discs holding
//! tracks.
//!
//! Offline and read-only. The walk is [`crate::runner::walk_audio_files`] and the per-track
//! metadata is [`crate::meta::resolve`], both shared with `scan`, so the two commands can never
//! disagree about which files count or what they're tagged with.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ebook::lyrics::{self, Stanza};
use crate::meta::{self, ResolvedMeta, TrackMeta};
use crate::runner;
use crate::sidecar::{self, SidecarDetail};

/// Filenames accepted as album art, in preference order. Matched case-insensitively against the
/// file stem, paired with each extension in [`ART_EXTENSIONS`].
const ART_STEMS: &[&str] = &["folder", "cover"];
/// Image extensions accepted as album art. Kept to what `image` is built with here.
const ART_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];
/// Album title used when a track carries no album tag.
const UNKNOWN_ALBUM: &str = "Unknown Album";

/// What a track's sidecar turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricState {
    /// A timestamped `.lrc`.
    Synced,
    /// A `.txt`, or an `.lrc` with no timestamps in it.
    Plain,
    /// The `[00:00.00]Instrumental` marker `scan` writes.
    Instrumental,
    /// No sidecar at all.
    Missing,
}

impl LyricState {
    /// Does this state carry words to put on a page of their own?
    #[must_use]
    pub const fn has_lyrics(self) -> bool {
        matches!(self, Self::Synced | Self::Plain)
    }
}

/// One track, whether or not it has lyrics: every track on an album appears in its tracklist.
#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    /// The track's own artist. Shown under the title only where it differs from the album
    /// artist, which is how a compilation reads correctly.
    pub artist: String,
    /// Track number within its disc, when tagged.
    pub number: Option<u32>,
    pub state: LyricState,
    /// Empty unless `state.has_lyrics()`.
    pub stanzas: Vec<Stanza>,
}

/// One disc of an album. Single-disc albums have exactly one, numbered 1.
#[derive(Debug, Clone)]
pub struct Disc {
    pub number: u32,
    pub tracks: Vec<Track>,
}

/// One album: a subchapter of the book.
#[derive(Debug, Clone)]
pub struct Album {
    pub title: String,
    /// The album artist this album was grouped under.
    pub artist: String,
    pub year: Option<u32>,
    /// Path to `folder.jpg`/`cover.png`/etc., when one sits beside the tracks.
    pub art: Option<PathBuf>,
    pub discs: Vec<Disc>,
}

impl Album {
    /// Total tracks across every disc.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.discs.iter().map(|d| d.tracks.len()).sum()
    }

    /// Tracks that have words to show.
    #[must_use]
    pub fn lyric_count(&self) -> usize {
        self.discs
            .iter()
            .flat_map(|d| &d.tracks)
            .filter(|t| t.state.has_lyrics())
            .count()
    }

    /// Does this album span more than one disc? Drives whether `CD n` headings are rendered.
    #[must_use]
    pub const fn is_multi_disc(&self) -> bool {
        self.discs.len() > 1
    }
}

/// One artist: a chapter of the book.
#[derive(Debug, Clone)]
pub struct Artist {
    pub name: String,
    pub albums: Vec<Album>,
}

/// The whole book's content, ready to render.
#[derive(Debug, Clone, Default)]
pub struct Book {
    pub artists: Vec<Artist>,
    /// Audio files skipped because title or artist could not be read from their tags.
    pub untagged: u32,
    /// Tracks that were included in a tracklist but had no lyrics to show.
    pub without_lyrics: u32,
}

impl Book {
    /// Every album in the book, in reading order. Used to pick cover-collage art.
    pub fn albums(&self) -> impl Iterator<Item = &Album> {
        self.artists.iter().flat_map(|a| &a.albums)
    }

    /// Total tracks that contribute a lyric page.
    #[must_use]
    pub fn song_count(&self) -> usize {
        self.albums().map(Album::lyric_count).sum()
    }

    /// Total albums across every artist.
    #[must_use]
    pub fn album_count(&self) -> usize {
        self.artists.iter().map(|a| a.albums.len()).sum()
    }
}

/// Sort key for an artist name: case-insensitive, ignoring a leading "The".
///
/// Without this "The Beatles" files under T, away from every other B artist, which is not how
/// anyone looks for a band on a shelf.
fn artist_sort_key(name: &str) -> String {
    let lower = name.to_lowercase();
    lower
        .strip_prefix("the ")
        .unwrap_or(&lower)
        .trim()
        .to_owned()
}

/// Find album art sitting beside the tracks, e.g. `folder.jpg`.
///
/// Matches [`ART_STEMS`] × [`ART_EXTENSIONS`] case-insensitively, in preference order, by
/// reading the directory once rather than probing each of the six candidate names.
fn find_art(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && meta::has_extension(p, ART_EXTENSIONS))
        .collect();

    ART_STEMS.iter().find_map(|stem| {
        entries
            .iter()
            .find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(stem))
            })
            .cloned()
    })
}

/// Read a track's sidecar and classify it.
///
/// Uses `sidecar_detail` rather than `sidecar_state`: the latter is documented as a lossy view
/// that folds the instrumental marker into "synced", and the tracklist needs to tell those
/// apart. A sidecar that exists but can't be read degrades to `Missing` — one unreadable file
/// must not fail the whole book.
fn read_lyrics(audio: &Path) -> (LyricState, Vec<Stanza>) {
    let state = match sidecar::sidecar_detail(audio) {
        SidecarDetail::Synced => LyricState::Synced,
        SidecarDetail::Plain => LyricState::Plain,
        SidecarDetail::Instrumental => return (LyricState::Instrumental, Vec::new()),
        SidecarDetail::None => return (LyricState::Missing, Vec::new()),
    };

    // `.lrc` wins when both exist, matching how `sidecar_detail` decides.
    let lrc = sidecar::sidecar_path(audio, "lrc");
    let path = if lrc.exists() {
        lrc
    } else {
        sidecar::sidecar_path(audio, "txt")
    };

    let Ok(contents) = fs::read_to_string(&path) else {
        return (LyricState::Missing, Vec::new());
    };

    let stanzas = lyrics::to_stanzas(&contents);
    if stanzas.is_empty() {
        // A sidecar with no words in it is no better than none for a book.
        return (LyricState::Missing, Vec::new());
    }
    (state, stanzas)
}

/// A track plus the grouping/ordering facts taken from its tags, before grouping happens.
struct Entry {
    meta: TrackMeta,
    path: PathBuf,
    state: LyricState,
    stanzas: Vec<Stanza>,
}

impl Entry {
    /// The artist this track's album files under: album artist, falling back to the track
    /// artist. A compilation tags every track with a different `artist` but one shared
    /// `album_artist`, which is exactly the chapter it belongs in.
    fn album_artist(&self) -> &str {
        self.meta
            .album_artist
            .as_deref()
            .unwrap_or(&self.meta.artist)
    }

    /// Album title, or a placeholder when the tag is absent.
    fn album_title(&self) -> &str {
        self.meta.album.as_deref().unwrap_or(UNKNOWN_ALBUM)
    }
}

/// Collect `dir` into a book model.
///
/// Makes no network requests and writes nothing: every path here is a read.
#[must_use]
pub fn collect(dir: &Path) -> Book {
    let mut book = Book::default();
    let mut entries: Vec<Entry> = Vec::new();

    for path in runner::walk_audio_files(dir) {
        // Tags only: the book never guesses an artist from a directory name, so a mis-shaped
        // tree can't invent chapters.
        let ResolvedMeta::Ok(meta) = meta::resolve(&path, false) else {
            book.untagged = book.untagged.saturating_add(1);
            continue;
        };
        let (state, stanzas) = read_lyrics(&path);
        entries.push(Entry {
            meta,
            path,
            state,
            stanzas,
        });
    }

    let grouped = group(entries, &mut book);
    book.artists = grouped;
    book
}

/// Key an album is grouped by. Sorted by the `BTreeMap` holding it, which is why grouping is
/// deterministic before any explicit sort runs.
type AlbumKey = (String, String);

/// Group entries into artists → albums → discs → tracks, sorted for reading.
fn group(entries: Vec<Entry>, book: &mut Book) -> Vec<Artist> {
    let mut albums: BTreeMap<AlbumKey, Vec<Entry>> = BTreeMap::new();
    for entry in entries {
        let key = (
            entry.album_artist().to_owned(),
            entry.album_title().to_owned(),
        );
        albums.entry(key).or_default().push(entry);
    }

    let mut by_artist: BTreeMap<String, Vec<Album>> = BTreeMap::new();
    for ((artist, title), group) in albums {
        // An album nobody has any lyrics for would render as a cover and a dead tracklist, so
        // it's dropped whole — along with any artist left with nothing.
        if !group.iter().any(|e| e.state.has_lyrics()) {
            continue;
        }
        let album = build_album(artist.clone(), title, group, book);
        by_artist.entry(artist).or_default().push(album);
    }

    let mut artists: Vec<Artist> = by_artist
        .into_iter()
        .map(|(name, mut albums)| {
            // Chronological within an artist, with the title as a stable tiebreak for untagged
            // years (which sort last, since `None` would otherwise lead).
            albums.sort_by(|a, b| {
                a.year
                    .unwrap_or(u32::MAX)
                    .cmp(&b.year.unwrap_or(u32::MAX))
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            });
            Artist { name, albums }
        })
        .collect();

    artists.sort_by(|a, b| {
        artist_sort_key(&a.name)
            .cmp(&artist_sort_key(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
    artists
}

/// Assemble one album from its tracks, splitting them across discs.
fn build_album(artist: String, title: String, entries: Vec<Entry>, book: &mut Book) -> Album {
    let art = entries
        .first()
        .and_then(|e| e.path.parent().and_then(find_art));
    // The earliest tagged year wins: remasters and reissues often re-tag individual tracks, and
    // the album belongs where the record was released.
    let year = entries.iter().filter_map(|e| e.meta.year).min();

    let mut discs: BTreeMap<u32, Vec<Entry>> = BTreeMap::new();
    for entry in entries {
        // An untagged disc number means a single-disc release.
        let disc = entry.meta.disc_number.unwrap_or(1);
        discs.entry(disc).or_default().push(entry);
    }

    let discs = discs
        .into_iter()
        .map(|(number, mut entries)| {
            // Untagged track numbers sort last, then by path, so ordering stays deterministic
            // for a badly tagged album instead of following filesystem order.
            entries.sort_by(|a, b| {
                a.meta
                    .track_number
                    .unwrap_or(u32::MAX)
                    .cmp(&b.meta.track_number.unwrap_or(u32::MAX))
                    .then_with(|| a.path.cmp(&b.path))
            });
            let tracks = entries
                .into_iter()
                .map(|e| {
                    if !e.state.has_lyrics() {
                        book.without_lyrics = book.without_lyrics.saturating_add(1);
                    }
                    Track {
                        title: e.meta.title,
                        artist: e.meta.artist,
                        number: e.meta.track_number,
                        state: e.state,
                        stanzas: e.stanzas,
                    }
                })
                .collect();
            Disc { number, tracks }
        })
        .collect();

    Album {
        title,
        artist,
        year,
        art,
        discs,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Build an entry directly, bypassing the filesystem, so grouping and ordering can be
    /// tested without fixture audio files.
    #[allow(clippy::too_many_arguments)] // A test fixture builder mirroring every tag field.
    fn entry(
        title: &str,
        artist: &str,
        album_artist: Option<&str>,
        album: &str,
        track: Option<u32>,
        disc: Option<u32>,
        year: Option<u32>,
        state: LyricState,
    ) -> Entry {
        let path = PathBuf::from(format!("/music/{artist}/{album}/{title}.flac"));
        Entry {
            meta: TrackMeta {
                path: path.clone(),
                title: title.to_owned(),
                artist: artist.to_owned(),
                album: Some(album.to_owned()),
                duration: None,
                album_artist: album_artist.map(str::to_owned),
                track_number: track,
                disc_number: disc,
                year,
                guessed: vec![],
            },
            path,
            state,
            stanzas: if state.has_lyrics() {
                vec![vec![title.to_owned()]]
            } else {
                vec![]
            },
        }
    }

    fn synced(title: &str, artist: &str, album: &str, track: u32) -> Entry {
        entry(
            title,
            artist,
            None,
            album,
            Some(track),
            None,
            Some(1990),
            LyricState::Synced,
        )
    }

    fn group_only(entries: Vec<Entry>) -> Vec<Artist> {
        let mut book = Book::default();
        group(entries, &mut book)
    }

    #[test]
    fn groups_albums_under_their_artist() {
        let artists = group_only(vec![
            synced("A", "Metallica", "Ride the Lightning", 1),
            synced("B", "Metallica", "Ride the Lightning", 2),
        ]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Metallica");
        assert_eq!(artists[0].albums.len(), 1);
        assert_eq!(artists[0].albums[0].track_count(), 2);
    }

    #[test]
    fn album_artist_wins_over_track_artist() {
        // A compilation: every track a different artist, one shared album artist.
        let artists = group_only(vec![
            entry(
                "A",
                "Artist One",
                Some("Various Artists"),
                "Comp",
                Some(1),
                None,
                Some(2000),
                LyricState::Synced,
            ),
            entry(
                "B",
                "Artist Two",
                Some("Various Artists"),
                "Comp",
                Some(2),
                None,
                Some(2000),
                LyricState::Synced,
            ),
        ]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Various Artists");
        assert_eq!(artists[0].albums[0].track_count(), 2);
        // The per-track artists survive, so the tracklist can show them.
        let titles: Vec<&str> = artists[0].albums[0].discs[0]
            .tracks
            .iter()
            .map(|t| t.artist.as_str())
            .collect();
        assert_eq!(titles, vec!["Artist One", "Artist Two"]);
    }

    #[test]
    fn falls_back_to_track_artist_without_an_album_artist_tag() {
        let artists = group_only(vec![synced("A", "Slayer", "Reign in Blood", 1)]);
        assert_eq!(artists[0].name, "Slayer");
    }

    #[test]
    fn leading_the_is_ignored_when_sorting_artists() {
        let artists = group_only(vec![
            synced("A", "The Beatles", "Revolver", 1),
            synced("B", "Cream", "Disraeli Gears", 1),
            synced("C", "ABBA", "Arrival", 1),
        ]);
        let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
        // "The Beatles" files under B, between ABBA and Cream — not under T.
        assert_eq!(names, vec!["ABBA", "The Beatles", "Cream"]);
    }

    #[test]
    fn albums_sort_by_year_then_title() {
        let artists = group_only(vec![
            entry(
                "A",
                "X",
                None,
                "Later",
                Some(1),
                None,
                Some(1995),
                LyricState::Synced,
            ),
            entry(
                "B",
                "X",
                None,
                "Earlier",
                Some(1),
                None,
                Some(1980),
                LyricState::Synced,
            ),
            entry(
                "C",
                "X",
                None,
                "Undated",
                Some(1),
                None,
                None,
                LyricState::Synced,
            ),
        ]);
        let titles: Vec<&str> = artists[0].albums.iter().map(|a| a.title.as_str()).collect();
        // Undated albums sort last rather than leading.
        assert_eq!(titles, vec!["Earlier", "Later", "Undated"]);
    }

    #[test]
    fn tracks_sort_by_disc_then_number() {
        let artists = group_only(vec![
            entry(
                "Two",
                "X",
                None,
                "A",
                Some(2),
                Some(1),
                Some(1990),
                LyricState::Synced,
            ),
            entry(
                "Four",
                "X",
                None,
                "A",
                Some(2),
                Some(2),
                Some(1990),
                LyricState::Synced,
            ),
            entry(
                "One",
                "X",
                None,
                "A",
                Some(1),
                Some(1),
                Some(1990),
                LyricState::Synced,
            ),
            entry(
                "Three",
                "X",
                None,
                "A",
                Some(1),
                Some(2),
                Some(1990),
                LyricState::Synced,
            ),
        ]);
        let album = &artists[0].albums[0];
        assert!(album.is_multi_disc());
        assert_eq!(album.discs.len(), 2);
        assert_eq!(album.discs[0].number, 1);
        let disc_one: Vec<&str> = album.discs[0]
            .tracks
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(disc_one, vec!["One", "Two"]);
        let disc_two: Vec<&str> = album.discs[1]
            .tracks
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(disc_two, vec!["Three", "Four"]);
    }

    #[test]
    fn an_untagged_disc_number_lands_on_disc_one() {
        let artists = group_only(vec![synced("A", "X", "Album", 1)]);
        let album = &artists[0].albums[0];
        assert_eq!(album.discs.len(), 1);
        assert_eq!(album.discs[0].number, 1);
        assert!(!album.is_multi_disc());
    }

    #[test]
    fn untracked_titles_sort_last_by_path() {
        let artists = group_only(vec![
            entry(
                "Zeta",
                "X",
                None,
                "A",
                None,
                None,
                Some(1990),
                LyricState::Synced,
            ),
            entry(
                "Alpha",
                "X",
                None,
                "A",
                None,
                None,
                Some(1990),
                LyricState::Synced,
            ),
            entry(
                "Numbered",
                "X",
                None,
                "A",
                Some(1),
                None,
                Some(1990),
                LyricState::Synced,
            ),
        ]);
        let titles: Vec<&str> = artists[0].albums[0].discs[0]
            .tracks
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Numbered", "Alpha", "Zeta"]);
    }

    #[test]
    fn lyricless_and_instrumental_tracks_stay_on_the_album() {
        // They get no page of their own, but the tracklist must still show them.
        let artists = group_only(vec![
            synced("Has Lyrics", "X", "A", 1),
            entry(
                "Silent",
                "X",
                None,
                "A",
                Some(2),
                None,
                Some(1990),
                LyricState::Missing,
            ),
            entry(
                "Interlude",
                "X",
                None,
                "A",
                Some(3),
                None,
                Some(1990),
                LyricState::Instrumental,
            ),
        ]);
        let album = &artists[0].albums[0];
        assert_eq!(album.track_count(), 3);
        assert_eq!(album.lyric_count(), 1);
    }

    #[test]
    fn an_album_with_no_lyrics_at_all_is_dropped() {
        let artists = group_only(vec![
            entry(
                "A",
                "X",
                None,
                "Empty",
                Some(1),
                None,
                Some(1990),
                LyricState::Missing,
            ),
            entry(
                "B",
                "X",
                None,
                "Empty",
                Some(2),
                None,
                Some(1990),
                LyricState::Instrumental,
            ),
        ]);
        // Nothing to read, so neither the album nor the artist appears in the book.
        assert!(artists.is_empty());
    }

    #[test]
    fn an_artist_keeps_only_the_albums_that_have_lyrics() {
        let artists = group_only(vec![
            synced("A", "X", "Good", 1),
            entry(
                "B",
                "X",
                None,
                "Empty",
                Some(1),
                None,
                Some(1990),
                LyricState::Missing,
            ),
        ]);
        assert_eq!(artists.len(), 1);
        let titles: Vec<&str> = artists[0].albums.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["Good"]);
    }

    #[test]
    fn counts_tracks_listed_without_lyrics() {
        let mut book = Book::default();
        let artists = group(
            vec![
                synced("A", "X", "Album", 1),
                entry(
                    "B",
                    "X",
                    None,
                    "Album",
                    Some(2),
                    None,
                    Some(1990),
                    LyricState::Missing,
                ),
                entry(
                    "C",
                    "X",
                    None,
                    "Album",
                    Some(3),
                    None,
                    Some(1990),
                    LyricState::Instrumental,
                ),
            ],
            &mut book,
        );
        book.artists = artists;
        assert_eq!(book.without_lyrics, 2);
        assert_eq!(book.song_count(), 1);
        assert_eq!(book.album_count(), 1);
    }

    #[test]
    fn artist_sort_key_strips_a_leading_the() {
        assert_eq!(artist_sort_key("The Beatles"), "beatles");
        assert_eq!(artist_sort_key("Theatre of Tragedy"), "theatre of tragedy");
        assert_eq!(artist_sort_key("ABBA"), "abba");
    }

    #[test]
    fn missing_album_tag_becomes_a_placeholder_title() {
        let mut e = synced("A", "X", "Ignored", 1);
        e.meta.album = None;
        assert_eq!(e.album_title(), UNKNOWN_ALBUM);
    }
}
