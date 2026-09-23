// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The countdown's block-glyph digits.

const GLYPH_HEIGHT: usize = 5;
const GLYPH_WIDTH: usize = 5;

const fn glyph(c: char) -> Option<[&'static str; GLYPH_HEIGHT]> {
    Some(match c {
        '1' => [" ##  ", "  #  ", "  #  ", "  #  ", " ### "],
        '2' => [" ### ", "#   #", "   # ", "  #  ", "#####"],
        '3' => ["#### ", "    #", " ### ", "    #", "#### "],
        'P' => ["#### ", "#   #", "#### ", "#    ", "#    "],
        'L' => ["#    ", "#    ", "#    ", "#    ", "#####"],
        'A' => [" ### ", "#   #", "#####", "#   #", "#   #"],
        'Y' => ["#   #", " # # ", "  #  ", "  #  ", "  #  "],
        _ => return None,
    })
}

#[must_use]
pub fn render(text: &str) -> Option<[String; GLYPH_HEIGHT]> {
    let glyphs: Vec<[&'static str; GLYPH_HEIGHT]> =
        text.to_uppercase().chars().filter_map(glyph).collect();
    if glyphs.is_empty() {
        return None;
    }

    let mut rows: [String; GLYPH_HEIGHT] = Default::default();
    for (row_index, row) in rows.iter_mut().enumerate() {
        let mut pieces = Vec::with_capacity(glyphs.len());
        for glyph in &glyphs {
            if let Some(piece) = glyph.get(row_index) {
                pieces.push(*piece);
            }
        }
        *row = pieces.join(" ");
    }
    Some(rows)
}

#[must_use]
pub const fn width(glyph_count: usize) -> usize {
    if glyph_count == 0 {
        return 0;
    }
    glyph_count
        .saturating_mul(GLYPH_WIDTH)
        .saturating_add(glyph_count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_countdown_glyph_is_defined() {
        for c in ['1', '2', '3', 'P', 'L', 'A', 'Y'] {
            assert!(glyph(c).is_some(), "{c} has no glyph");
        }
    }

    #[test]
    fn every_glyph_is_the_same_height_and_width() {
        for c in ['1', '2', '3', 'P', 'L', 'A', 'Y'] {
            let rows = glyph(c).unwrap();
            assert_eq!(rows.len(), GLYPH_HEIGHT);
            for row in rows {
                assert_eq!(
                    row.chars().count(),
                    GLYPH_WIDTH,
                    "{c} row {row:?} is not {GLYPH_WIDTH} wide"
                );
            }
        }
    }

    #[test]
    fn render_play_has_four_glyphs_worth_of_width() {
        let rows = render("PLAY").unwrap();
        assert_eq!(rows.len(), GLYPH_HEIGHT);
        let expected = width(4);
        for row in &rows {
            assert_eq!(row.chars().count(), expected, "{row:?}");
        }
    }

    #[test]
    fn render_is_case_insensitive() {
        assert_eq!(render("play"), render("PLAY"));
    }

    #[test]
    fn render_drops_characters_with_no_glyph_rather_than_failing() {
        let rows = render("P!AY").unwrap();
        assert_eq!(
            rows[0].chars().count(),
            width(3),
            "the unglyphable character was not dropped"
        );
    }

    #[test]
    fn render_of_an_empty_or_all_unglyphable_string_is_none() {
        assert!(render("").is_none());
        assert!(render("!!!").is_none());
    }

    #[test]
    fn width_of_zero_glyphs_is_zero() {
        assert_eq!(width(0), 0);
    }
}
