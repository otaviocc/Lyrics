// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Package rendered documents into an EPUB 3 file.

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

use anyhow::{Context, Result};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::ebook::render::Rendered;

const CONTENT_DIR: &str = "OEBPS";

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles>
<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles>
</container>
"#;

fn fixed_time() -> DateTime {
    DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap_or_default()
}

pub fn write(path: &Path, rendered: &Rendered) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("could not create {}", path.display()))?;
    let mut zip = ZipWriter::new(BufWriter::new(file));

    write_archive(&mut zip, rendered)
        .with_context(|| format!("could not write {}", path.display()))?;

    zip.finish()
        .with_context(|| format!("could not finalize {}", path.display()))?;
    Ok(())
}

fn write_archive<W: Write + Seek>(zip: &mut ZipWriter<W>, rendered: &Rendered) -> Result<()> {
    let time = fixed_time();
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(time);
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(time);

    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;

    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(CONTAINER_XML.as_bytes())?;

    let mut text = |name: &str, contents: &str| -> Result<()> {
        zip.start_file(format!("{CONTENT_DIR}/{name}"), deflated)?;
        zip.write_all(contents.as_bytes())?;
        Ok(())
    };

    text("content.opf", &rendered.opf)?;
    text("nav.xhtml", &rendered.nav)?;
    text("toc.ncx", &rendered.ncx)?;
    text("style.css", &rendered.stylesheet)?;
    for page in &rendered.pages {
        text(&page.path, &page.content)?;
    }

    for image in &rendered.images {
        zip.start_file(format!("{CONTENT_DIR}/{}", image.path), stored)?;
        zip.write_all(&image.bytes)?;
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::tempdir;
    use zip::ZipArchive;

    use super::*;
    use crate::ebook::library::{Album, Artist, Book, Disc, LyricState, Track};
    use crate::ebook::render::{self, BookInfo};

    fn sample_book() -> Book {
        Book {
            artists: vec![Artist {
                name: "Metallica".to_owned(),
                albums: vec![Album {
                    title: "...And Justice for All".to_owned(),
                    artist: "Metallica".to_owned(),
                    year: Some(1988),
                    art: None,
                    discs: vec![Disc {
                        number: 1,
                        tracks: vec![Track {
                            title: "One".to_owned(),
                            artist: "Metallica".to_owned(),
                            number: Some(4),
                            state: LyricState::Synced,
                            stanzas: vec![vec!["I can't remember anything".to_owned()]],
                        }],
                    }],
                }],
            }],
            untagged: 0,
            without_lyrics: 0,
        }
    }

    fn rendered() -> render::Rendered {
        let info = BookInfo {
            title: "Lyrics".to_owned(),
            author: "Various Artists".to_owned(),
        };
        render::render(
            &sample_book(),
            &info,
            |_| None,
            |_| Some(vec![0xff, 0xd8, 0xff]),
        )
    }

    #[test]
    fn mimetype_is_the_first_entry_and_is_stored_uncompressed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("book.epub");
        write(&path, &rendered()).unwrap();

        let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);

        let mut entry = archive.by_name("mimetype").unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
        assert_eq!(contents, "application/epub+zip");
    }

    #[test]
    fn the_archive_holds_every_rendered_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let rendered = rendered();
        write(&path, &rendered).unwrap();

        let archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();

        for required in [
            "mimetype",
            "META-INF/container.xml",
            "OEBPS/content.opf",
            "OEBPS/nav.xhtml",
            "OEBPS/toc.ncx",
            "OEBPS/style.css",
        ] {
            assert!(names.contains(&required.to_owned()), "missing {required}");
        }
        for page in &rendered.pages {
            let expected = format!("OEBPS/{}", page.path);
            assert!(names.contains(&expected), "missing {expected}");
        }
        for image in &rendered.images {
            let expected = format!("OEBPS/{}", image.path);
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn container_xml_points_at_the_package_document() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("book.epub");
        write(&path, &rendered()).unwrap();

        let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut entry = archive.by_name("META-INF/container.xml").unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
        assert!(contents.contains(r#"full-path="OEBPS/content.opf""#));
    }

    #[test]
    fn two_builds_of_the_same_book_are_byte_identical() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("first.epub");
        let second = dir.path().join("second.epub");
        write(&first, &rendered()).unwrap();
        write(&second, &rendered()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    }

    #[test]
    fn writing_to_an_unwritable_path_is_an_error_not_a_panic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no-such-dir").join("book.epub");
        assert!(write(&path, &rendered()).is_err());
    }
}
