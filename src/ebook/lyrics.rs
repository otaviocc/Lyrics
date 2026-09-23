// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Turn a sidecar's raw contents into display-ready stanzas.

use crate::lrc::{self, Line};

pub type Stanza = Vec<String>;

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
            Line::Metadata { .. } | Line::Comment(_) | Line::Malformed(_) => {}
        }
    }

    end_stanza(&mut current, &mut stanzas);
    stanzas
}

fn push_or_break(text: &str, current: &mut Stanza, stanzas: &mut Vec<Stanza>) {
    let text = text.trim();
    if text.is_empty() {
        end_stanza(current, stanzas);
    } else {
        current.push(text.to_owned());
    }
}

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
        let lrc = "[00:01.00]One\n[00:09.00]\n";
        assert_eq!(to_stanzas(lrc), vec![vec!["One".to_owned()]]);
    }

    #[test]
    fn empty_input_yields_no_stanzas() {
        assert!(to_stanzas("").is_empty());
        assert!(to_stanzas("\n\n\n").is_empty());
    }
}
