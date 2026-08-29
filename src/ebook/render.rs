// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Render a [`Book`] into the XHTML documents, stylesheet, and images an EPUB is made of.
//!
//! Nothing here touches the filesystem or the ZIP container: it takes a book model and returns
//! bytes, which is what lets the whole layer be unit-tested against exact expected strings.
//! [`super::epub`] packages the result.
//!
//! The one structural decision worth knowing: **every song is its own XHTML document**. A CSS
//! `page-break-before` is a hint that reflowable readers honor inconsistently, but a new
//! document always starts a new page — so "a lyric never shares a page with the previous one"
//! is guaranteed by the file layout rather than by styling.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::ebook::library::{Album, Artist, Book, LyricState, Track};

/// A rendered XHTML document destined for the book.
pub struct Page {
    /// Path inside the EPUB's content directory, e.g. `song-0001.xhtml`.
    pub path: String,
    /// Manifest id, referenced by the spine.
    pub id: String,
    pub content: String,
}

/// A binary asset (album art or the cover collage) destined for the book.
pub struct Image {
    pub path: String,
    pub id: String,
    pub bytes: Vec<u8>,
}

/// Everything needed to write the container.
pub struct Rendered {
    /// Content documents, in spine (reading) order.
    pub pages: Vec<Page>,
    pub images: Vec<Image>,
    pub stylesheet: String,
    pub nav: String,
    pub ncx: String,
    pub opf: String,
    /// Manifest id of the cover image, when there is one.
    pub cover_id: Option<String>,
}

/// Book-level metadata supplied by the caller.
pub struct BookInfo {
    pub title: String,
    pub author: String,
}

/// Escape text for inclusion in XML character data or an attribute value.
///
/// Every user-supplied string — album titles, artist names, lyric lines — passes through here.
/// An unescaped `&` in a band name is the single likeliest way to produce an EPUB that readers
/// reject outright, and library metadata is full of them.
#[must_use]
pub fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// A stable 64-bit FNV-1a hash, used to derive the book's unique identifier from its title.
///
/// EPUB requires a `dc:identifier`. Deriving it from the title rather than generating a UUID
/// keeps the output byte-identical across runs — which is what makes the whole book testable —
/// and avoids a `uuid` dependency for one string.
fn stable_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Timestamp written as the EPUB's `dcterms:modified`, which the spec requires.
///
/// Deliberately a constant rather than the current time: a real clock reading would make two
/// builds of an unchanged library differ, defeating the reproducibility the tests rely on.
const MODIFIED: &str = "2026-01-01T00:00:00Z";

/// XHTML document preamble, shared by every page.
fn page_header(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en">
<head>
<meta charset="utf-8"/>
<title>{}</title>
<link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
"#,
        escape_xml(title)
    )
}

/// Closing tags for every page.
const PAGE_FOOTER: &str = "</body>\n</html>\n";

// --- Layout ------------------------------------------------------------------------------
//
// Filenames are assigned before any page is rendered, because an album's tracklist links to
// the song pages it lists. The layout types below are that first pass.

/// A track paired with the song page it will link to, if it has one.
struct TrackLayout<'a> {
    track: &'a Track,
    /// `None` for instrumental and lyric-less tracks: they appear in the tracklist but get no
    /// page, so there is nothing to link to.
    file: Option<String>,
    id: Option<String>,
    /// 1-based position within the disc, used to number a track whose tag has no number.
    position: u32,
}

struct DiscLayout<'a> {
    number: u32,
    tracks: Vec<TrackLayout<'a>>,
}

struct AlbumLayout<'a> {
    album: &'a Album,
    file: String,
    id: String,
    /// `(path, id)` of the embedded art, when the source file decoded.
    art: Option<(String, String)>,
    discs: Vec<DiscLayout<'a>>,
}

struct ArtistLayout<'a> {
    artist: &'a Artist,
    file: String,
    id: String,
    albums: Vec<AlbumLayout<'a>>,
}

/// Assign every page and image a filename and manifest id, and decode album art.
///
/// `thumbnail` is injected rather than called directly so the layout can be tested without
/// touching real image files.
fn lay_out<F>(book: &Book, mut thumbnail: F) -> (Vec<ArtistLayout<'_>>, Vec<Image>)
where
    F: FnMut(&PathBuf) -> Option<Vec<u8>>,
{
    let mut images = Vec::new();
    let mut song_number: u32 = 0;
    let mut album_number: u32 = 0;

    let artists = book
        .artists
        .iter()
        .enumerate()
        .map(|(artist_index, artist)| {
            let number = artist_index.saturating_add(1);
            let albums = artist
                .albums
                .iter()
                .map(|album| {
                    album_number = album_number.saturating_add(1);
                    let art = album.art.as_ref().and_then(|source| {
                        let bytes = thumbnail(source)?;
                        let path = format!("images/album-{album_number:04}.jpg");
                        let id = format!("img-album-{album_number:04}");
                        images.push(Image {
                            path: path.clone(),
                            id: id.clone(),
                            bytes,
                        });
                        Some((path, id))
                    });

                    let discs = album
                        .discs
                        .iter()
                        .map(|disc| {
                            let tracks = disc
                                .tracks
                                .iter()
                                .enumerate()
                                .map(|(track_index, track)| {
                                    let (file, id) = if track.state.has_lyrics() {
                                        song_number = song_number.saturating_add(1);
                                        (
                                            Some(format!("song-{song_number:04}.xhtml")),
                                            Some(format!("song-{song_number:04}")),
                                        )
                                    } else {
                                        (None, None)
                                    };
                                    TrackLayout {
                                        track,
                                        file,
                                        id,
                                        position: u32::try_from(track_index)
                                            .unwrap_or(0)
                                            .saturating_add(1),
                                    }
                                })
                                .collect();
                            DiscLayout {
                                number: disc.number,
                                tracks,
                            }
                        })
                        .collect();

                    AlbumLayout {
                        album,
                        file: format!("album-{album_number:04}.xhtml"),
                        id: format!("album-{album_number:04}"),
                        art,
                        discs,
                    }
                })
                .collect();

            ArtistLayout {
                artist,
                file: format!("artist-{number:03}.xhtml"),
                id: format!("artist-{number:03}"),
                albums,
            }
        })
        .collect();

    (artists, images)
}

// --- Pages -------------------------------------------------------------------------------

/// The cover: one full-bleed image, and nothing else.
///
/// Title included — `cover::cover_image` rasterizes it into the JPEG. Nothing about the cover is
/// markup, because reading systems re-theme markup: Apple Books in night mode discards a
/// `background-color` and substitutes its own text color, which left an HTML title illegible on
/// the artwork. No reader re-themes an image. Don't reintroduce a CSS overlay here.
///
/// Only called when there is an image; `render` omits the cover page entirely otherwise.
fn cover_page(info: &BookInfo) -> String {
    let mut out = page_header(&info.title);
    let _ = writeln!(
        out,
        "<div class=\"cover\"><img class=\"cover-art\" src=\"images/cover.jpg\" alt=\"{}\"/></div>",
        escape_xml(&info.title)
    );
    out.push_str(PAGE_FOOTER);
    out
}

/// The book's table of contents: one entry per artist.
///
/// Artists only, deliberately. Each artist chapter already opens with an index of its own
/// albums, so listing every album here as well would repeat the whole book's structure on a
/// page nobody reads twice.
///
/// This is a real page in the spine, distinct from `nav.xhtml`: the navigation document drives
/// the reader's own TOC menu but is never paged into, so without this the book has no contents
/// you can simply turn to.
fn contents_page(artists: &[ArtistLayout], info: &BookInfo) -> String {
    let mut out = page_header(&info.title);
    out.push_str("<section epub:type=\"toc\">\n<h1 class=\"contents\">Contents</h1>\n<ul class=\"contents\">\n");
    for artist in artists {
        let _ = writeln!(
            out,
            "<li><a href=\"{}\">{}</a></li>",
            escape_xml(&artist.file),
            escape_xml(&artist.artist.name)
        );
    }
    out.push_str("</ul>\n</section>\n");
    out.push_str(PAGE_FOOTER);
    out
}

/// An artist chapter: a title page listing the artist's albums.
fn artist_page(layout: &ArtistLayout) -> String {
    let name = &layout.artist.name;
    let mut out = page_header(name);
    let _ = writeln!(
        out,
        "<section epub:type=\"chapter\">\n<h1 class=\"artist\">{}</h1>\n<ul class=\"album-index\">",
        escape_xml(name)
    );
    for album in &layout.albums {
        let _ = writeln!(
            out,
            "<li><a href=\"{}\">{}</a>{}</li>",
            escape_xml(&album.file),
            escape_xml(&album.album.title),
            album
                .album
                .year
                .map_or_else(String::new, |y| format!(" <span class=\"year\">{y}</span>"))
        );
    }
    out.push_str("</ul>\n</section>\n");
    out.push_str(PAGE_FOOTER);
    out
}

/// Render one track's line in a tracklist.
fn tracklist_entry(layout: &TrackLayout, album_artist: &str) -> String {
    let track = layout.track;
    let number = track.number.unwrap_or(layout.position);
    let title = escape_xml(&track.title);

    // A linked title is the signal that a track has lyrics; an unlinked one that it doesn't.
    // Labelling every lyric-less line "no lyrics" would be noise on a sparsely covered album.
    let title = layout.file.as_ref().map_or_else(
        || format!("<span class=\"title\">{title}</span>"),
        |file| format!("<a href=\"{}\">{title}</a>", escape_xml(file)),
    );

    let instrumental = if track.state == LyricState::Instrumental {
        " <span class=\"tag\">instrumental</span>"
    } else {
        ""
    };

    // On a compilation each track has its own artist; show it only where it differs from the
    // album's, so a normal single-artist album isn't cluttered with a repeated name.
    let artist = if track.artist == album_artist {
        String::new()
    } else {
        format!(
            "<span class=\"track-artist\">{}</span>",
            escape_xml(&track.artist)
        )
    };

    let class = if layout.file.is_some() {
        "track"
    } else {
        "track no-lyrics"
    };
    format!(
        "<li class=\"{class}\"><span class=\"num\">{number}</span>{title}{instrumental}{artist}</li>\n"
    )
}

/// An album subchapter: cover art, then the album's full tracklist.
fn album_page(layout: &AlbumLayout) -> String {
    let album = layout.album;
    let mut out = page_header(&album.title);
    let _ = writeln!(
        out,
        "<section epub:type=\"chapter\">\n<h1 class=\"album\">{}</h1>",
        escape_xml(&album.title)
    );

    if let Some((path, _)) = &layout.art {
        let _ = writeln!(
            out,
            "<div class=\"album-art\"><img src=\"{}\" alt=\"{}\"/></div>",
            escape_xml(path),
            escape_xml(&album.title)
        );
    }

    let mut meta_parts: Vec<String> = Vec::new();
    if let Some(year) = album.year {
        meta_parts.push(year.to_string());
    }
    meta_parts.push(format!(
        "{} of {} tracks with lyrics",
        album.lyric_count(),
        album.track_count()
    ));
    let _ = writeln!(
        out,
        "<p class=\"album-meta\">{}</p>",
        escape_xml(&meta_parts.join(" · "))
    );

    for disc in &layout.discs {
        // Only a genuinely multi-disc album gets "CD n" headings; a single-disc release would
        // just carry a redundant "CD 1" above its only list.
        if album.is_multi_disc() {
            let _ = writeln!(out, "<h2 class=\"disc\">CD {}</h2>", disc.number);
        }
        out.push_str("<ul class=\"tracklist\">\n");
        for track in &disc.tracks {
            out.push_str(&tracklist_entry(track, &album.artist));
        }
        out.push_str("</ul>\n");
    }

    out.push_str("</section>\n");
    out.push_str(PAGE_FOOTER);
    out
}

/// One song's lyrics, on a page of its own.
fn song_page(track: &Track, album_artist: &str) -> String {
    let mut out = page_header(&track.title);
    let _ = writeln!(
        out,
        "<section epub:type=\"chapter\">\n<h1 class=\"song\">{}</h1>",
        escape_xml(&track.title)
    );
    if track.artist != album_artist {
        let _ = writeln!(
            out,
            "<p class=\"song-artist\">{}</p>",
            escape_xml(&track.artist)
        );
    }
    for stanza in &track.stanzas {
        out.push_str("<div class=\"stanza\">\n");
        for line in stanza {
            let _ = writeln!(out, "<p>{}</p>", escape_xml(line));
        }
        out.push_str("</div>\n");
    }
    out.push_str("</section>\n");
    out.push_str(PAGE_FOOTER);
    out
}

/// The closing page: what the book contains and what it left out.
fn colophon_page(book: &Book) -> String {
    let mut out = page_header("About this book");
    out.push_str(
        "<section epub:type=\"colophon\">\n<h1>About this book</h1>\n<ul class=\"colophon\">\n",
    );
    let _ = writeln!(
        out,
        "<li>{} artists</li>\n<li>{} albums</li>\n<li>{} songs with lyrics</li>",
        book.artists.len(),
        book.album_count(),
        book.song_count()
    );
    if book.without_lyrics > 0 {
        let _ = writeln!(
            out,
            "<li>{} tracks listed without lyrics</li>",
            book.without_lyrics
        );
    }
    if book.untagged > 0 {
        let _ = writeln!(
            out,
            "<li>{} files skipped for missing tags</li>",
            book.untagged
        );
    }
    out.push_str(
        "</ul>\n<p class=\"credit\">Generated by <code>lyrics ebook</code>.</p>\n</section>\n",
    );
    out.push_str(PAGE_FOOTER);
    out
}

// --- Navigation --------------------------------------------------------------------------

/// The EPUB 3 navigation document: artists as sections, their albums nested beneath.
fn nav_document(artists: &[ArtistLayout], info: &BookInfo) -> String {
    let mut out = page_header(&info.title);
    out.push_str("<nav epub:type=\"toc\" id=\"toc\">\n<h1>Contents</h1>\n<ol>\n");
    for artist in artists {
        let _ = writeln!(
            out,
            "<li><a href=\"{}\">{}</a>\n<ol>",
            escape_xml(&artist.file),
            escape_xml(&artist.artist.name)
        );
        for album in &artist.albums {
            let _ = writeln!(
                out,
                "<li><a href=\"{}\">{}</a></li>",
                escape_xml(&album.file),
                escape_xml(&album.album.title)
            );
        }
        out.push_str("</ol>\n</li>\n");
    }
    out.push_str("</ol>\n</nav>\n");
    out.push_str(PAGE_FOOTER);
    out
}

/// The EPUB 2 navigation map. Superseded by `nav.xhtml`, but still what older readers look for,
/// and cheap to emit alongside it.
fn ncx_document(artists: &[ArtistLayout], info: &BookInfo, uid: &str) -> String {
    let mut out = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<head>
<meta name="dtb:uid" content="{}"/>
<meta name="dtb:depth" content="2"/>
<meta name="dtb:totalPageCount" content="0"/>
<meta name="dtb:maxPageNumber" content="0"/>
</head>
<docTitle><text>{}</text></docTitle>
<navMap>
"#,
        escape_xml(uid),
        escape_xml(&info.title)
    );

    let mut order: u32 = 0;
    for artist in artists {
        order = order.saturating_add(1);
        let _ = writeln!(
            out,
            "<navPoint id=\"nav-{}\" playOrder=\"{order}\">\n<navLabel><text>{}</text></navLabel>\n<content src=\"{}\"/>",
            escape_xml(&artist.id),
            escape_xml(&artist.artist.name),
            escape_xml(&artist.file)
        );
        for album in &artist.albums {
            order = order.saturating_add(1);
            let _ = writeln!(
                out,
                "<navPoint id=\"nav-{}\" playOrder=\"{order}\">\n<navLabel><text>{}</text></navLabel>\n<content src=\"{}\"/>\n</navPoint>",
                escape_xml(&album.id),
                escape_xml(&album.album.title),
                escape_xml(&album.file)
            );
        }
        out.push_str("</navPoint>\n");
    }

    out.push_str("</navMap>\n</ncx>\n");
    out
}

/// The package document: metadata, the manifest of every file, and the spine's reading order.
fn opf_document(
    info: &BookInfo,
    uid: &str,
    pages: &[Page],
    images: &[Image],
    cover_id: Option<&str>,
) -> String {
    let mut out = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="book-id">{}</dc:identifier>
<dc:title>{}</dc:title>
<dc:creator>{}</dc:creator>
<dc:language>en</dc:language>
<meta property="dcterms:modified">{MODIFIED}</meta>
"#,
        escape_xml(uid),
        escape_xml(&info.title),
        escape_xml(&info.author)
    );
    if let Some(id) = cover_id {
        // The `name="cover"` form is EPUB 2, kept because several readers still use it to find
        // the thumbnail for a library shelf.
        let _ = writeln!(out, "<meta name=\"cover\" content=\"{}\"/>", escape_xml(id));
    }
    out.push_str("</metadata>\n<manifest>\n");
    out.push_str("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    out.push_str("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
    out.push_str("<item id=\"style\" href=\"style.css\" media-type=\"text/css\"/>\n");
    for page in pages {
        let _ = writeln!(
            out,
            "<item id=\"{}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>",
            escape_xml(&page.id),
            escape_xml(&page.path)
        );
    }
    for image in images {
        let properties = if Some(image.id.as_str()) == cover_id {
            " properties=\"cover-image\""
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "<item id=\"{}\" href=\"{}\" media-type=\"image/jpeg\"{properties}/>",
            escape_xml(&image.id),
            escape_xml(&image.path)
        );
    }
    out.push_str("</manifest>\n<spine toc=\"ncx\">\n");
    for page in pages {
        let _ = writeln!(out, "<itemref idref=\"{}\"/>", escape_xml(&page.id));
    }
    out.push_str("</spine>\n</package>\n");
    out
}

/// The book's stylesheet.
///
/// Ragged right rather than justified: justification stretches word spacing to fill a line, and
/// lyric lines are short enough that it opens rivers of whitespace across the page. The hanging
/// indent matters for the same reason — without it, a long line that wraps is indistinguishable
/// from the next line of the song.
const STYLESHEET: &str = r#"@charset "utf-8";

/* No margin on the body: the cover is full-bleed, and every other page indents itself via
   `section` below. A margin here would letterbox the collage. */
body {
  font-family: Georgia, "Iowan Old Style", "Palatino Linotype", serif;
  line-height: 1.6;
  margin: 0;
  text-align: left;
  -epub-hyphens: none;
  hyphens: none;
}

section { margin: 0 5%; }

h1, h2 { font-weight: normal; line-height: 1.25; }

h1.artist {
  font-size: 2em;
  margin: 3em 0 1.5em;
  text-align: center;
  letter-spacing: 0.02em;
}

h1.album { font-size: 1.5em; margin: 1.5em 0 1em; text-align: center; }
h1.song { font-size: 1.4em; margin: 1.5em 0 0.25em; }

p.song-artist {
  font-size: 0.85em;
  font-style: italic;
  color: #555;
  margin: 0 0 1.5em;
}

h1.song + div.stanza { margin-top: 1.5em; }

div.album-art { margin: 0 auto 1em; text-align: center; }
div.album-art img { max-width: 65%; max-height: 45vh; }

p.album-meta {
  text-align: center;
  font-size: 0.85em;
  color: #555;
  margin: 0 0 2em;
}

h2.disc {
  font-size: 0.8em;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: #666;
  margin: 2em 0 0.5em;
  border-bottom: 1px solid #ddd;
  padding-bottom: 0.3em;
}

ul.tracklist, ul.album-index, ul.contents, ul.colophon {
  list-style: none;
  padding: 0;
  margin: 0;
}

ul.tracklist li {
  margin: 0.45em 0;
  padding-left: 2.6em;
  text-indent: -2.6em;
}

ul.tracklist span.num {
  display: inline-block;
  width: 2.2em;
  color: #999;
  font-size: 0.85em;
  text-indent: 0;
}

ul.tracklist li.no-lyrics, ul.tracklist li.no-lyrics span.title { color: #999; }

span.tag {
  font-size: 0.7em;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #999;
  margin-left: 0.5em;
}

span.track-artist {
  display: block;
  font-size: 0.8em;
  font-style: italic;
  color: #777;
  margin-left: 2.2em;
  text-indent: 0;
}

h1.contents {
  font-size: 1.6em;
  margin: 2.5em 0 1.5em;
  text-align: center;
  letter-spacing: 0.08em;
}

ul.contents li {
  margin: 0.8em 0;
  text-align: center;
  font-size: 1.1em;
}

ul.album-index li { margin: 0.6em 0; text-align: center; }
ul.album-index span.year { color: #999; font-size: 0.85em; }

/* Stanzas are spaced by the block's own margin rather than by empty paragraphs, so a reader
   that collapses empty elements still shows the song's shape. */
div.stanza { margin: 0 0 1.4em; }

div.stanza p {
  margin: 0;
  padding-left: 1.2em;
  text-indent: -1.2em;
  orphans: 2;
  widows: 2;
}

/* Links carry no underline or color of their own. In a tracklist a rule under every title is
   visual noise on a 40-track page, and the distinction that matters is already there: a track
   with lyrics is set in the body color, one without is greyed by `li.no-lyrics`. */
a { color: inherit; text-decoration: none; }

div.cover { margin: 0; padding: 0; text-align: center; }
img.cover-art { width: 100%; height: auto; display: block; }

ul.colophon li { margin: 0.4em 0; }
p.credit { margin-top: 2em; font-size: 0.85em; color: #777; }
"#;

/// Render `book` into every file the container needs, in reading order.
///
/// `thumbnail` decodes and downscales one album-art file, returning `None` if it can't;
/// `cover` builds the collage from the art paths it is handed. Both are injected so this
/// function stays free of image decoding and therefore testable on its own.
pub fn render<F, C>(book: &Book, info: &BookInfo, thumbnail: F, cover: C) -> Rendered
where
    F: FnMut(&PathBuf) -> Option<Vec<u8>>,
    C: FnOnce(&[PathBuf]) -> Option<Vec<u8>>,
{
    let (artists, mut images) = lay_out(book, thumbnail);

    let art_paths: Vec<PathBuf> = book.albums().filter_map(|a| a.art.clone()).collect();
    let cover_id = cover(&art_paths).map(|bytes| {
        let id = "cover-image".to_owned();
        images.push(Image {
            path: "images/cover.jpg".to_owned(),
            id: id.clone(),
            bytes,
        });
        id
    });

    // No image means the encoder failed; a cover page with nothing on it is worse than none.
    let mut pages: Vec<Page> = cover_id
        .iter()
        .map(|_| Page {
            path: "cover.xhtml".to_owned(),
            id: "cover".to_owned(),
            content: cover_page(info),
        })
        .collect();
    pages.push(Page {
        path: "contents.xhtml".to_owned(),
        id: "contents".to_owned(),
        content: contents_page(&artists, info),
    });

    for artist in &artists {
        pages.push(Page {
            path: artist.file.clone(),
            id: artist.id.clone(),
            content: artist_page(artist),
        });
        for album in &artist.albums {
            pages.push(Page {
                path: album.file.clone(),
                id: album.id.clone(),
                content: album_page(album),
            });
            for track in album.discs.iter().flat_map(|d| &d.tracks) {
                let (Some(file), Some(id)) = (&track.file, &track.id) else {
                    continue;
                };
                pages.push(Page {
                    path: file.clone(),
                    id: id.clone(),
                    content: song_page(track.track, &album.album.artist),
                });
            }
        }
    }

    pages.push(Page {
        path: "colophon.xhtml".to_owned(),
        id: "colophon".to_owned(),
        content: colophon_page(book),
    });

    let uid = format!("urn:lyrics:{:016x}", stable_hash(&info.title));
    let nav = nav_document(&artists, info);
    let ncx = ncx_document(&artists, info, &uid);
    let opf = opf_document(info, &uid, &pages, &images, cover_id.as_deref());

    Rendered {
        pages,
        images,
        stylesheet: STYLESHEET.to_owned(),
        nav,
        ncx,
        opf,
        cover_id,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ebook::library::{Disc, LyricState};

    fn track(title: &str, number: u32, state: LyricState) -> Track {
        Track {
            title: title.to_owned(),
            artist: "Test Artist".to_owned(),
            number: Some(number),
            state,
            stanzas: if state.has_lyrics() {
                vec![vec!["A line".to_owned(), "Another line".to_owned()]]
            } else {
                vec![]
            },
        }
    }

    fn album(title: &str, discs: Vec<Disc>) -> Album {
        Album {
            title: title.to_owned(),
            artist: "Test Artist".to_owned(),
            year: Some(1991),
            art: None,
            discs,
        }
    }

    fn book_with(discs: Vec<Disc>) -> Book {
        Book {
            artists: vec![Artist {
                name: "Test Artist".to_owned(),
                albums: vec![album("Test Album", discs)],
            }],
            untagged: 0,
            without_lyrics: 0,
        }
    }

    fn info() -> BookInfo {
        BookInfo {
            title: "Lyrics".to_owned(),
            author: "Various Artists".to_owned(),
        }
    }

    /// Render with image work stubbed out: no art decodes, no cover collage.
    fn render_bare(book: &Book) -> Rendered {
        render(book, &info(), |_| None, |_| None)
    }

    #[test]
    fn escapes_every_xml_metacharacter() {
        assert_eq!(escape_xml("Simon & Garfunkel"), "Simon &amp; Garfunkel");
        assert_eq!(escape_xml("<b>"), "&lt;b&gt;");
        assert_eq!(escape_xml(r#"say "it""#), "say &quot;it&quot;");
        assert_eq!(escape_xml("don't"), "don&apos;t");
        assert_eq!(escape_xml("plain"), "plain");
    }

    #[test]
    fn ampersand_in_an_album_title_reaches_the_page_escaped() {
        let book = Book {
            artists: vec![Artist {
                name: "AC/DC".to_owned(),
                albums: vec![album(
                    "Rock & Roll",
                    vec![Disc {
                        number: 1,
                        tracks: vec![track("Song", 1, LyricState::Synced)],
                    }],
                )],
            }],
            untagged: 0,
            without_lyrics: 0,
        };
        let rendered = render_bare(&book);
        let page = rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap();
        assert!(page.content.contains("Rock &amp; Roll"));
        assert!(!page.content.contains("Rock & Roll"));
    }

    #[test]
    fn every_song_gets_its_own_document() {
        // The page-break guarantee: three songs, three separate XHTML files.
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("One", 1, LyricState::Synced),
                track("Two", 2, LyricState::Plain),
                track("Three", 3, LyricState::Synced),
            ],
        }]);
        let rendered = render_bare(&book);
        let songs: Vec<&str> = rendered
            .pages
            .iter()
            .filter(|p| p.path.starts_with("song-"))
            .map(|p| p.path.as_str())
            .collect();
        assert_eq!(
            songs,
            vec!["song-0001.xhtml", "song-0002.xhtml", "song-0003.xhtml"]
        );
    }

    #[test]
    fn lyricless_tracks_get_no_page_but_stay_in_the_tracklist() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("Has Lyrics", 1, LyricState::Synced),
                track("Silent", 2, LyricState::Missing),
                track("Interlude", 3, LyricState::Instrumental),
            ],
        }]);
        let rendered = render_bare(&book);
        assert_eq!(
            rendered
                .pages
                .iter()
                .filter(|p| p.path.starts_with("song-"))
                .count(),
            1
        );

        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert!(page.contains("Silent"));
        assert!(page.contains("Interlude"));
        // The linked title is the "has lyrics" signal; the other two are unlinked.
        assert!(page.contains(r#"<a href="song-0001.xhtml">Has Lyrics</a>"#));
        assert!(page.contains(r#"<span class="title">Silent</span>"#));
        assert!(page.contains(r#"<span class="tag">instrumental</span>"#));
        assert!(page.contains("1 of 3 tracks with lyrics"));
    }

    #[test]
    fn a_single_disc_album_gets_no_cd_heading() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert!(!page.contains("CD 1"));
        assert!(!page.contains("class=\"disc\""));
    }

    #[test]
    fn a_multi_disc_album_gets_a_heading_per_disc() {
        let book = book_with(vec![
            Disc {
                number: 1,
                tracks: vec![track("One", 1, LyricState::Synced)],
            },
            Disc {
                number: 2,
                tracks: vec![track("Two", 1, LyricState::Synced)],
            },
        ]);
        let rendered = render_bare(&book);
        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert!(page.contains(r#"<h2 class="disc">CD 1</h2>"#));
        assert!(page.contains(r#"<h2 class="disc">CD 2</h2>"#));
        // One list per disc.
        assert_eq!(page.matches("<ul class=\"tracklist\">").count(), 2);
    }

    #[test]
    fn track_artist_is_shown_only_when_it_differs_from_the_album_artist() {
        let mut book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("Same", 1, LyricState::Synced),
                track("Different", 2, LyricState::Synced),
            ],
        }]);
        book.artists[0].albums[0].discs[0].tracks[1].artist = "Guest Artist".to_owned();

        let rendered = render_bare(&book);
        let album_page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert_eq!(album_page.matches("track-artist").count(), 1);
        assert!(album_page.contains("Guest Artist"));

        let song = &rendered
            .pages
            .iter()
            .find(|p| p.path == "song-0002.xhtml")
            .unwrap()
            .content;
        assert!(song.contains(r#"<p class="song-artist">Guest Artist</p>"#));

        let same = &rendered
            .pages
            .iter()
            .find(|p| p.path == "song-0001.xhtml")
            .unwrap()
            .content;
        assert!(!same.contains("song-artist"));
    }

    #[test]
    fn a_track_with_no_number_falls_back_to_its_position() {
        let mut book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("First", 9, LyricState::Synced),
                track("Second", 9, LyricState::Synced),
            ],
        }]);
        book.artists[0].albums[0].discs[0].tracks[1].number = None;
        let rendered = render_bare(&book);
        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        // The tagged number is used where present, the 1-based position where it isn't.
        assert!(page.contains(r#"<span class="num">9</span>"#));
        assert!(page.contains(r#"<span class="num">2</span>"#));
    }

    #[test]
    fn stanzas_render_one_paragraph_per_line() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("Song", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        let song = &rendered
            .pages
            .iter()
            .find(|p| p.path == "song-0001.xhtml")
            .unwrap()
            .content;
        assert!(
            song.contains("<div class=\"stanza\">\n<p>A line</p>\n<p>Another line</p>\n</div>")
        );
        // Timestamps never reach the page — they were dropped upstream in `lyrics::to_stanzas`.
        assert!(!song.contains('['));
    }

    #[test]
    fn the_manifest_lists_exactly_the_files_that_are_written() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("One", 1, LyricState::Synced),
                track("Two", 2, LyricState::Missing),
            ],
        }]);
        let rendered = render_bare(&book);
        for page in &rendered.pages {
            assert!(
                rendered.opf.contains(&format!("href=\"{}\"", page.path)),
                "{} is missing from the manifest",
                page.path
            );
            assert!(
                rendered.opf.contains(&format!("idref=\"{}\"", page.id)),
                "{} is missing from the spine",
                page.id
            );
        }
        // A track with no lyrics contributes no document, so none may be listed.
        assert!(!rendered.opf.contains("song-0002.xhtml"));
    }

    #[test]
    fn pages_are_in_reading_order() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        let order: Vec<&str> = rendered.pages.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "contents.xhtml",
                "artist-001.xhtml",
                "album-0001.xhtml",
                "song-0001.xhtml",
                "colophon.xhtml",
            ]
        );
    }

    #[test]
    fn every_list_class_used_in_a_page_is_covered_by_the_list_reset() {
        // Adding a `<ul class="...">` without adding it to the reset rule leaves the browser's
        // default bullets and indent on it, which is how the contents page first shipped wrong.
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        let reset = rendered
            .stylesheet
            .split("list-style: none;")
            .next()
            .unwrap_or_default()
            .to_owned();

        for page in &rendered.pages {
            for fragment in page.content.split("<ul class=\"").skip(1) {
                let class = fragment.split('"').next().unwrap_or_default();
                assert!(
                    reset.contains(&format!("ul.{class}")),
                    "ul.{class} is missing from the list reset rule"
                );
            }
        }
    }

    #[test]
    fn a_contents_page_follows_the_cover() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render(&book, &info(), |_| None, |_| Some(vec![0xff, 0xd8]));
        let contents = &rendered.pages[1];
        assert_eq!(rendered.pages[0].path, "cover.xhtml");
        assert_eq!(contents.path, "contents.xhtml");
        assert!(
            contents
                .content
                .contains(r#"<h1 class="contents">Contents</h1>"#)
        );
        assert!(
            contents
                .content
                .contains(r#"<li><a href="artist-001.xhtml">Test Artist</a></li>"#)
        );
    }

    #[test]
    fn the_contents_page_lists_every_artist_and_no_albums() {
        // Albums are deliberately absent: each artist chapter indexes its own.
        let book = Book {
            artists: vec![
                Artist {
                    name: "First Artist".to_owned(),
                    albums: vec![album(
                        "Album A",
                        vec![Disc {
                            number: 1,
                            tracks: vec![track("One", 1, LyricState::Synced)],
                        }],
                    )],
                },
                Artist {
                    name: "Second Artist".to_owned(),
                    albums: vec![album(
                        "Album B",
                        vec![Disc {
                            number: 1,
                            tracks: vec![track("Two", 1, LyricState::Synced)],
                        }],
                    )],
                },
            ],
            untagged: 0,
            without_lyrics: 0,
        };
        let rendered = render(&book, &info(), |_| None, |_| Some(vec![0xff, 0xd8]));
        let contents = &rendered.pages[1].content;

        assert!(contents.contains(r#"<a href="artist-001.xhtml">First Artist</a>"#));
        assert!(contents.contains(r#"<a href="artist-002.xhtml">Second Artist</a>"#));
        assert_eq!(contents.matches("<li>").count(), 2);
        assert!(!contents.contains("album-"));
        assert!(!contents.contains("Album A"));
    }

    #[test]
    fn the_contents_page_is_in_the_manifest_and_the_spine() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        assert!(rendered.opf.contains(r#"href="contents.xhtml""#));
        assert!(rendered.opf.contains(r#"idref="contents""#));
    }

    #[test]
    fn artist_chapters_keep_their_own_album_index() {
        // The contents page replaces nothing: the per-artist index stays.
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        let artist = &rendered
            .pages
            .iter()
            .find(|p| p.path == "artist-001.xhtml")
            .unwrap()
            .content;
        assert!(artist.contains(r#"<a href="album-0001.xhtml">Test Album</a>"#));
        assert!(artist.contains("album-index"));
    }

    #[test]
    fn nav_nests_albums_under_artists() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        // The album's entry sits in a list nested inside the artist's, not beside it.
        assert!(rendered.nav.contains(concat!(
            "<li><a href=\"artist-001.xhtml\">Test Artist</a>\n",
            "<ol>\n",
            "<li><a href=\"album-0001.xhtml\">Test Album</a></li>\n",
            "</ol>\n",
            "</li>\n",
        )));
    }

    #[test]
    fn ncx_play_order_increments_across_artists_and_albums() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        assert!(rendered.ncx.contains(r#"playOrder="1""#));
        assert!(rendered.ncx.contains(r#"playOrder="2""#));
    }

    #[test]
    fn without_a_cover_image_nothing_claims_to_be_one() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render_bare(&book);
        assert!(rendered.cover_id.is_none());
        assert!(!rendered.opf.contains("cover-image"));
        assert!(rendered.images.is_empty());
        // No image means no cover page at all, rather than an empty one.
        assert!(rendered.pages.iter().all(|p| p.path != "cover.xhtml"));
        assert_eq!(
            rendered.pages.first().map(|p| p.path.as_str()),
            Some("contents.xhtml")
        );
    }

    #[test]
    fn the_cover_page_is_an_image_and_nothing_else() {
        // The title is baked into the JPEG, so no markup on this page may carry it.
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render(&book, &info(), |_| None, |_| Some(vec![0xff, 0xd8]));
        let cover = &rendered.pages.first().unwrap().content;
        assert!(cover.contains(r#"<img class="cover-art" src="images/cover.jpg""#));
        assert!(!cover.contains("<h1"));
        assert!(!cover.contains("cover-plate"));
        assert!(!rendered.stylesheet.contains("cover-title"));
        assert!(!rendered.stylesheet.contains("cover-plate"));
    }

    #[test]
    fn a_cover_image_is_manifested_with_the_cover_image_property() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        let rendered = render(&book, &info(), |_| None, |_| Some(vec![0xff, 0xd8]));
        assert_eq!(rendered.cover_id.as_deref(), Some("cover-image"));
        assert!(rendered.opf.contains(
            r#"href="images/cover.jpg" media-type="image/jpeg" properties="cover-image""#
        ));
        assert!(
            rendered
                .opf
                .contains(r#"<meta name="cover" content="cover-image"/>"#)
        );
    }

    #[test]
    fn album_art_is_embedded_and_referenced_when_it_decodes() {
        let mut book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        book.artists[0].albums[0].art = Some(PathBuf::from("/music/folder.jpg"));
        let rendered = render(&book, &info(), |_| Some(vec![0xff, 0xd8]), |_| None);

        assert_eq!(rendered.images.len(), 1);
        assert_eq!(rendered.images[0].path, "images/album-0001.jpg");
        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert!(page.contains(r#"<img src="images/album-0001.jpg""#));
    }

    #[test]
    fn art_that_fails_to_decode_leaves_the_album_without_an_image() {
        let mut book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        book.artists[0].albums[0].art = Some(PathBuf::from("/music/corrupt.jpg"));
        let rendered = render_bare(&book);
        assert!(rendered.images.is_empty());
        let page = &rendered
            .pages
            .iter()
            .find(|p| p.path == "album-0001.xhtml")
            .unwrap()
            .content;
        assert!(!page.contains("album-art"));
    }

    #[test]
    fn colophon_reports_what_was_left_out() {
        let mut book = book_with(vec![Disc {
            number: 1,
            tracks: vec![track("One", 1, LyricState::Synced)],
        }]);
        book.untagged = 4;
        book.without_lyrics = 7;
        let rendered = render_bare(&book);
        let colophon = &rendered.pages.last().unwrap().content;
        assert!(colophon.contains("<li>1 artists</li>"));
        assert!(colophon.contains("<li>7 tracks listed without lyrics</li>"));
        assert!(colophon.contains("<li>4 files skipped for missing tags</li>"));
    }

    #[test]
    fn rendering_is_deterministic() {
        let book = book_with(vec![Disc {
            number: 1,
            tracks: vec![
                track("One", 1, LyricState::Synced),
                track("Two", 2, LyricState::Synced),
            ],
        }]);
        let first = render_bare(&book);
        let second = render_bare(&book);
        assert_eq!(first.opf, second.opf);
        assert_eq!(first.nav, second.nav);
        assert_eq!(first.ncx, second.ncx);
        let firsts: Vec<&String> = first.pages.iter().map(|p| &p.content).collect();
        let seconds: Vec<&String> = second.pages.iter().map(|p| &p.content).collect();
        assert_eq!(firsts, seconds);
    }

    #[test]
    fn the_identifier_is_stable_for_a_title() {
        assert_eq!(stable_hash("Lyrics"), stable_hash("Lyrics"));
        assert_ne!(stable_hash("Lyrics"), stable_hash("Other"));
    }
}
