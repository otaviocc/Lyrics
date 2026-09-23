// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Track metadata: tags read with lofty, and the optional path fallback.

use std::path::{Path, PathBuf};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey, Tag};

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "m4b", "mp4", "ogg", "opus", "wav", "aiff", "aif", "wma",
];

#[must_use]
pub fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

#[must_use]
pub fn is_audio_file(path: &Path) -> bool {
    has_extension(path, AUDIO_EXTENSIONS)
}

#[derive(Debug, Clone)]
pub struct TrackMeta {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<u32>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub guessed: Vec<GuessedField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessedField {
    Title,
    Artist,
    Album,
}

impl GuessedField {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
        }
    }
}

struct RawTags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u32>,
    album_artist: Option<String>,
    track_number: Option<u32>,
    disc_number: Option<u32>,
    year: Option<u32>,
}

impl RawTags {
    const fn empty() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            duration: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
            year: None,
        }
    }
}

fn album_artist(tag: &Tag) -> Option<String> {
    non_empty(tag.get_string(ItemKey::AlbumArtist).map(str::to_owned))
}

fn year(tag: &Tag) -> Option<u32> {
    tag.get_string(ItemKey::RecordingDate)
        .or_else(|| tag.get_string(ItemKey::Year))
        .and_then(parse_year)
}

fn parse_year(raw: &str) -> Option<u32> {
    let digits: String = raw
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() != 4 {
        return None;
    }
    digits.parse().ok()
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_raw_tags(path: &Path) -> RawTags {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return RawTags::empty();
    };

    let duration = u32::try_from(tagged.properties().duration().as_secs())
        .ok()
        .filter(|&d| d > 0);

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let Some(tag) = tag else {
        return RawTags {
            duration,
            ..RawTags::empty()
        };
    };

    RawTags {
        title: non_empty(tag.title().map(|s| s.to_string())),
        artist: non_empty(tag.artist().map(|s| s.to_string())),
        album: non_empty(tag.album().map(|s| s.to_string())),
        duration,
        album_artist: album_artist(tag),
        track_number: tag.track(),
        disc_number: tag.disk(),
        year: year(tag),
    }
}

#[allow(clippy::string_slice)]
#[allow(clippy::arithmetic_side_effects)]
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

#[allow(clippy::string_slice)]
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

struct PathGuess {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

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

pub enum ResolvedMeta {
    Ok(TrackMeta),
    Untagged,
}

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
            album_artist: raw.album_artist,
            track_number: raw.track_number,
            disc_number: raw.disc_number,
            year: raw.year,
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
        assert_eq!(strip_trailing_markers("(Interlude)"), None);
    }

    #[test]
    fn leaves_non_trailing_brackets_alone() {
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
    fn parses_a_bare_year() {
        assert_eq!(parse_year("1991"), Some(1991));
        assert_eq!(parse_year("  1991 "), Some(1991));
    }

    #[test]
    fn parses_the_year_out_of_a_full_date() {
        assert_eq!(parse_year("1991-08-12"), Some(1991));
        assert_eq!(parse_year("1991-08-12T00:00:00Z"), Some(1991));
        assert_eq!(parse_year("2003/05/01"), Some(2003));
    }

    #[test]
    fn rejects_values_that_are_not_a_four_digit_year() {
        assert_eq!(parse_year(""), None);
        assert_eq!(parse_year("91"), None);
        assert_eq!(parse_year("199"), None);
        assert_eq!(parse_year("unknown"), None);
        assert_eq!(parse_year("19910"), None);
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
