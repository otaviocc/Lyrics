// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Track metadata: embedded tags first, optional path-derived fallback.
//!
//! Metadata comes from `lofty`, a pure-Rust tag library, so no subprocess is ever spawned. The
//! only lofty API called anywhere in this crate is `lofty::read_from_path`; do not add a call
//! to any tag-writing API (read-only guarantee, see AGENTS.md). Directory layout is not a
//! requirement; it is only consulted under `--path-fallback`.

use std::path::{Path, PathBuf};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;

/// Audio file extensions this tool will consider during a `scan`.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "m4b", "mp4", "ogg", "opus", "wav", "aiff", "aif", "wma",
];

/// Returns `true` if `path`'s extension case-insensitively matches one of `extensions`.
///
/// Shared by every extension check in the crate (`is_audio_file` here, plus the `.lrc`/`.txt`
/// checks in `stats` and `lrc`), so a future change to how extensions are compared only needs
/// to happen once.
#[must_use]
pub fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// Returns `true` if `path` has an extension matching a known audio format (case-insensitive).
#[must_use]
pub fn is_audio_file(path: &Path) -> bool {
    has_extension(path, AUDIO_EXTENSIONS)
}

/// Resolved metadata for a single audio file, ready for a lyrics lookup.
///
/// Title and artist are always present (resolution fails without them). Album and duration
/// are optional: not every file carries them, and the path fallback never guesses duration.
#[derive(Debug, Clone)]
pub struct TrackMeta {
    /// Kept for callers/tests that need the source path alongside the resolved metadata;
    /// `process_track` already has it separately and doesn't read this field.
    #[allow(dead_code)]
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Duration in whole seconds, when readable from the file's audio properties.
    /// Never derived from the path.
    pub duration: Option<u32>,
    /// Fields that were filled in via `--path-fallback` rather than an embedded tag,
    /// reported back to the caller for logging.
    pub guessed: Vec<GuessedField>,
}

/// A metadata field that was filled in from the file path rather than an embedded tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessedField {
    Title,
    Artist,
    Album,
}

impl GuessedField {
    /// Human-readable name of the field, used in log output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
        }
    }
}

/// Raw tag values read directly from the audio file, before any path-based guessing.
struct RawTags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u32>,
}

/// Trim and return `None` for empty strings, so callers never deal with whitespace-only tags.
fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Read title, artist, album, and duration straight from the audio file's tags via lofty.
///
/// Returns all-`None` fields when the file is unreadable or carries no recognized tags.
fn read_raw_tags(path: &Path) -> RawTags {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return RawTags {
            title: None,
            artist: None,
            album: None,
            duration: None,
        };
    };

    let duration = u32::try_from(tagged.properties().duration().as_secs())
        .ok()
        .filter(|&d| d > 0);

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let (title, artist, album) = tag.map_or((None, None, None), |tag| {
        (
            non_empty(tag.title().map(|s| s.to_string())),
            non_empty(tag.artist().map(|s| s.to_string())),
            non_empty(tag.album().map(|s| s.to_string())),
        )
    });

    RawTags {
        title,
        artist,
        album,
        duration,
    }
}

/// Strip a leading track-number prefix like "01 ", "01. ", "01_", "01-" from a filename stem.
///
/// Digits without a following separator are not stripped (e.g. "2001 A Space Odyssey" yields
/// "A Space Odyssey" but "21" stays "21").
#[allow(clippy::string_slice)] // Offsets are computed from `char_indices()`, always at boundaries.
#[allow(clippy::arithmetic_side_effects)] // Index arithmetic on known-valid char boundaries.
fn strip_track_number(stem: &str) -> &str {
    let mut chars = stem.char_indices().peekable();
    let mut digit_end = 0;
    while let Some(&(i, c)) = chars.peek() {
        if c.is_ascii_digit() {
            digit_end = i + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if digit_end == 0 {
        return stem;
    }
    let mut sep_end = digit_end;
    for (i, c) in stem[digit_end..].char_indices() {
        if c == ' ' || c == '.' || c == '_' || c == '-' {
            sep_end = digit_end + i + c.len_utf8();
        } else {
            break;
        }
    }
    if sep_end == digit_end {
        return stem;
    }
    stem[sep_end..].trim_start()
}

/// Strip trailing bracketed marker groups from a title, e.g.
/// `"Machine Gun Man (Acoustic) [Bonus Track]"` -> `"Machine Gun Man"`.
///
/// LRCLIB stores lyrics under the "base" track title; a version marker like `(Live)`,
/// `[Bonus Track]`, or `(Acoustic)` tacked onto a locally-tagged title makes both `/api/get`
/// and `/api/search` fail to find an otherwise-identical record (verified against the live
/// API). Any trailing `(...)`/`[...]` group is stripped, generically rather than against a
/// fixed keyword list, since the markers people use vary widely.
///
/// Returns `None` when nothing was stripped, so callers only retry when the title actually
/// changed.
#[must_use]
pub fn strip_trailing_markers(title: &str) -> Option<String> {
    let mut current = title.trim_end();
    let mut changed = false;

    while let Some(stripped) = strip_one_trailing_group(current) {
        let stripped = stripped.trim_end();
        if stripped.is_empty() {
            break;
        }
        current = stripped;
        changed = true;
    }

    changed.then(|| current.to_string())
}

/// Strip exactly one trailing `(...)` or `[...]` group from `s`, if `s` ends with one.
#[allow(clippy::string_slice)] // `open_idx` comes from `char_indices()`, always at a boundary.
fn strip_one_trailing_group(s: &str) -> Option<&str> {
    let (close, open) = match s.chars().next_back()? {
        ')' => (')', '('),
        ']' => (']', '['),
        _ => return None,
    };

    let mut depth = 0i32;
    let mut open_idx = None;
    for (i, c) in s.char_indices().rev() {
        if c == close {
            depth = depth.saturating_add(1);
        } else if c == open {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                open_idx = Some(i);
                break;
            }
        }
    }

    Some(&s[..open_idx?])
}

/// Metadata guessed from the file's position in the directory tree (Artist/Album/Title).
struct PathGuess {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

/// Derive title, album, and artist from a path shaped like `Artist/Album/01 Title.ext`.
///
/// The track-number prefix is stripped from the filename. Returns `None` for any field the
/// path doesn't contain enough segments to fill.
fn guess_from_path(path: &Path) -> PathGuess {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(strip_track_number)
        .map(std::string::ToString::to_string)
        .filter(|s| !s.is_empty());

    let album = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
        .filter(|s| !s.is_empty());

    let artist = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
        .filter(|s| !s.is_empty());

    PathGuess {
        title,
        artist,
        album,
    }
}

/// Outcome of resolving a track's metadata.
pub enum ResolvedMeta {
    Ok(TrackMeta),
    /// Title and/or artist could not be resolved (tags missing and either path fallback is
    /// disabled or the path itself didn't yield enough information).
    Untagged,
}

/// Resolve a track's metadata from embedded tags, optionally falling back to path-based
/// guessing when `path_fallback` is enabled.
///
/// Returns `Untagged` when title or artist could not be determined from either source.
#[must_use]
pub fn resolve(path: &Path, path_fallback: bool) -> ResolvedMeta {
    let raw = read_raw_tags(path);
    let mut guessed = Vec::new();

    let (title, artist, album) = if path_fallback {
        let guess = guess_from_path(path);

        let title = raw.title.clone().or_else(|| {
            guess.title.clone().inspect(|_| {
                guessed.push(GuessedField::Title);
            })
        });
        let artist = raw.artist.clone().or_else(|| {
            guess.artist.clone().inspect(|_| {
                guessed.push(GuessedField::Artist);
            })
        });
        let album = raw.album.clone().or_else(|| {
            guess.album.clone().inspect(|_| {
                guessed.push(GuessedField::Album);
            })
        });
        (title, artist, album)
    } else {
        (raw.title.clone(), raw.artist.clone(), raw.album.clone())
    };

    match (title, artist) {
        (Some(title), Some(artist)) => ResolvedMeta::Ok(TrackMeta {
            path: path.to_path_buf(),
            title,
            artist,
            album,
            duration: raw.duration,
            guessed,
        }),
        _ => ResolvedMeta::Untagged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_common_track_number_prefixes() {
        assert_eq!(strip_track_number("01 Track"), "Track");
        assert_eq!(strip_track_number("01. Track"), "Track");
        assert_eq!(strip_track_number("01_Track"), "Track");
        assert_eq!(strip_track_number("01-Track"), "Track");
        assert_eq!(strip_track_number("1 Track"), "Track");
        assert_eq!(strip_track_number("Track"), "Track");
    }

    #[test]
    fn does_not_strip_digits_with_no_separator() {
        // "2001" style titles should survive untouched.
        assert_eq!(
            strip_track_number("2001 A Space Odyssey"),
            "A Space Odyssey"
        );
        assert_eq!(strip_track_number("21"), "21");
    }

    #[test]
    fn strips_single_trailing_marker() {
        assert_eq!(
            strip_trailing_markers("Mother Mary [Bonus Track]"),
            Some("Mother Mary".to_string())
        );
        assert_eq!(
            strip_trailing_markers("The Wizard [Live]"),
            Some("The Wizard".to_string())
        );
        assert_eq!(
            strip_trailing_markers("Tom's Diner (Acoustic)"),
            Some("Tom's Diner".to_string())
        );
    }

    #[test]
    fn strips_multiple_trailing_markers() {
        assert_eq!(
            strip_trailing_markers("Machine Gun Man (Acoustic) [Bonus Track]"),
            Some("Machine Gun Man".to_string())
        );
    }

    #[test]
    fn no_markers_returns_none() {
        assert_eq!(strip_trailing_markers("Eye In The Sky"), None);
    }

    #[test]
    fn does_not_strip_down_to_an_empty_title() {
        // The whole title is one bracketed group, so stripping it would leave nothing useful.
        // Bail out instead of returning Some("").
        assert_eq!(strip_trailing_markers("(Interlude)"), None);
    }

    #[test]
    fn leaves_non_trailing_brackets_alone() {
        // A parenthetical in the middle of the title, not at the end, isn't a version marker.
        assert_eq!(
            strip_trailing_markers("Say My Name (feat. Someone) Reprise"),
            None
        );
    }

    #[test]
    fn guesses_title_album_artist_from_path() {
        let path = Path::new("/music/Artist Name/Album Name/01 Track Title.flac");
        let guess = guess_from_path(path);
        assert_eq!(guess.title.as_deref(), Some("Track Title"));
        assert_eq!(guess.album.as_deref(), Some("Album Name"));
        assert_eq!(guess.artist.as_deref(), Some("Artist Name"));
    }

    #[test]
    fn is_audio_file_matches_known_extensions_case_insensitively() {
        assert!(is_audio_file(Path::new("track.MP3")));
        assert!(is_audio_file(Path::new("track.flac")));
        assert!(!is_audio_file(Path::new("track.lrc")));
        assert!(!is_audio_file(Path::new("track.txt")));
        assert!(!is_audio_file(Path::new("cover.jpg")));
    }
}
