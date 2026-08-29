// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Turn a sidecar's raw contents into display-ready stanzas.
//!
//! Reading lyrics is not parsing them a second time: this is a fold over the existing
//! [`crate::lrc::parse_line`], so `ebook` and `lint` can never disagree about what a line of an
//! `.lrc` file means. The book shows words only — timestamps, `[ar:]`-style metadata tags, and
//! comments are all dropped here rather than in the renderer.

use crate::lrc::{self, Line};

/// A stanza: consecutive lyric lines with no blank line between them.
pub type Stanza = Vec<String>;

/// Split `contents` into stanzas of plain text, discarding timestamps and metadata.
///
/// Handles both sidecar flavors with one pass. A synced `.lrc` yields `Line::Timed`, whose text
/// is kept and whose timestamps are dropped; a plain `.txt` yields `Line::Untimed`, kept as-is.
/// Blank lines — and the *break entries* a synced file uses to mark "stop showing the previous
/// line", which parse as `Timed` with empty text — both end the current stanza. Runs of them
/// collapse into a single break, so a file padded with blank lines doesn't render as a page of
/// whitespace.
#[must_use]
pub fn to_stanzas(contents: &str) -> Vec<Stanza> {
    let mut stanzas: Vec<Stanza> = Vec::new();
    let mut current: Stanza = Vec::new();

    for raw in contents.lines() {
        match lrc::parse_line(raw) {
            Line::Timed { text, .. } | Line::Untimed(text) => {
                push_or_break(text, &mut current, &mut stanzas);
            }
            Line::Blank => end_stanza(&mut current, &mut stanzas),
            // Not lyrics: `[ar:]`/`[ti:]`/`[by:]` tags, `#` comments, and tag-like lines the
            // parser couldn't make sense of. A malformed line is dropped rather than shown,
            // since rendering a half-parsed `[00:1` into a book helps nobody.
            Line::Metadata { .. } | Line::Comment(_) | Line::Malformed(_) => {}
        }
    }

    end_stanza(&mut current, &mut stanzas);
    stanzas
}

/// Append `text` to the open stanza, or end that stanza when `text` is blank.
fn push_or_break(text: &str, current: &mut Stanza, stanzas: &mut Vec<Stanza>) {
    let text = text.trim();
    if text.is_empty() {
        end_stanza(current, stanzas);
    } else {
        current.push(text.to_owned());
    }
}

/// Close the open stanza, if it has any lines. A no-op otherwise, which is what collapses
/// consecutive blank lines into a single stanza break.
fn end_stanza(current: &mut Stanza, stanzas: &mut Vec<Stanza>) {
    if !current.is_empty() {
        stanzas.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_timestamps_from_synced_lyrics() {
        let lrc = "[00:12.00]First line\n[00:15.50]Second line\n";
        assert_eq!(
            to_stanzas(lrc),
            vec![vec!["First line".to_owned(), "Second line".to_owned()]]
        );
    }

    #[test]
    fn drops_metadata_and_comment_lines() {
        let lrc = "[ar:Some Artist]\n[ti:Some Title]\n[by:someone]\n# a comment\n[00:01.00]Words\n";
        assert_eq!(to_stanzas(lrc), vec![vec!["Words".to_owned()]]);
    }

    #[test]
    fn plain_text_sidecar_passes_through() {
        let txt = "First line\nSecond line\n\nThird line\n";
        assert_eq!(
            to_stanzas(txt),
            vec![
                vec!["First line".to_owned(), "Second line".to_owned()],
                vec!["Third line".to_owned()],
            ]
        );
    }

    #[test]
    fn blank_line_starts_a_new_stanza() {
        let lrc = "[00:01.00]One\n\n[00:05.00]Two\n";
        assert_eq!(
            to_stanzas(lrc),
            vec![vec!["One".to_owned()], vec!["Two".to_owned()]]
        );
    }

    #[test]
    fn break_entry_ends_a_stanza_like_a_blank_line() {
        // A timestamp with no text is a break entry, not a lyric line.
        let lrc = "[00:01.00]One\n[00:04.00]\n[00:05.00]Two\n";
        assert_eq!(
            to_stanzas(lrc),
            vec![vec!["One".to_owned()], vec!["Two".to_owned()]]
        );
    }

    #[test]
    fn consecutive_breaks_collapse_into_one() {
        let lrc = "[00:01.00]One\n\n\n\n[00:05.00]Two\n";
        assert_eq!(
            to_stanzas(lrc),
            vec![vec!["One".to_owned()], vec!["Two".to_owned()]]
        );
    }

    #[test]
    fn trailing_break_does_not_emit_an_empty_stanza() {
        // LRCLIB ends most synced records with a break entry; it must not become a blank stanza.
        let lrc = "[00:01.00]One\n[00:09.00]\n";
        assert_eq!(to_stanzas(lrc), vec![vec!["One".to_owned()]]);
    }

    #[test]
    fn empty_input_yields_no_stanzas() {
        assert!(to_stanzas("").is_empty());
        assert!(to_stanzas("\n\n\n").is_empty());
    }
}
