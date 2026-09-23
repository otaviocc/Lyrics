// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Sidecar path derivation, on-disk state detection, and writes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const INSTRUMENTAL_MARKER: &str = "[00:00.00]Instrumental\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarState {
    Synced,
    Plain,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarDetail {
    Synced,
    Instrumental,
    Plain,
    None,
}

#[must_use]
pub fn sidecar_path(audio_path: &Path, extension: &str) -> PathBuf {
    let path = audio_path.with_extension(extension);
    debug_assert_ne!(
        path, audio_path,
        "sidecar_path must never return the audio file's own path"
    );
    path
}

fn lrc_path(audio_path: &Path) -> PathBuf {
    sidecar_path(audio_path, "lrc")
}

fn txt_path(audio_path: &Path) -> PathBuf {
    sidecar_path(audio_path, "txt")
}

#[allow(clippy::string_slice)]
fn is_timestamp_line(line: &str) -> bool {
    let line = line.trim_start();
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let Some(close) = rest.find(']') else {
        return false;
    };
    let tag = &rest[..close];
    let Some((mins, secs)) = tag.split_once(':') else {
        return false;
    };
    !mins.is_empty()
        && mins.chars().all(|c| c.is_ascii_digit())
        && !secs.is_empty()
        && secs
            .chars()
            .take_while(|c| *c != '.')
            .all(|c| c.is_ascii_digit())
}

#[must_use]
pub fn sidecar_state(audio_path: &Path) -> SidecarState {
    match sidecar_detail(audio_path) {
        SidecarDetail::Synced | SidecarDetail::Instrumental => SidecarState::Synced,
        SidecarDetail::Plain => SidecarState::Plain,
        SidecarDetail::None => SidecarState::None,
    }
}

#[must_use]
pub fn sidecar_detail(audio_path: &Path) -> SidecarDetail {
    let lrc = lrc_path(audio_path);
    if lrc.exists() {
        if let Ok(contents) = fs::read_to_string(&lrc) {
            if contents.trim() == INSTRUMENTAL_MARKER.trim() {
                return SidecarDetail::Instrumental;
            }
            if contents.lines().any(is_timestamp_line) {
                return SidecarDetail::Synced;
            }
        }
        return SidecarDetail::Plain;
    }

    if txt_path(audio_path).exists() {
        return SidecarDetail::Plain;
    }

    SidecarDetail::None
}

fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sidecar");
    let tmp_path = dir.join(format!(".{file_name}.tmp"));
    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, path)
}

fn normalize(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

pub fn write_synced(audio_path: &Path, lyrics: &str, keep_plain: bool) -> io::Result<()> {
    write_atomic(&lrc_path(audio_path), &normalize(lyrics.to_string()))?;
    if !keep_plain {
        let txt = txt_path(audio_path);
        if txt.exists() {
            debug_assert_eq!(txt.extension().and_then(|e| e.to_str()), Some("txt"));
            fs::remove_file(&txt)?;
        }
    }
    Ok(())
}

pub fn write_plain(audio_path: &Path, lyrics: &str) -> io::Result<()> {
    write_atomic(&txt_path(audio_path), &normalize(lyrics.to_string()))
}

pub fn write_instrumental_marker_if_absent(audio_path: &Path) -> io::Result<bool> {
    if sidecar_state(audio_path) != SidecarState::None {
        return Ok(false);
    }
    write_atomic(&lrc_path(audio_path), INSTRUMENTAL_MARKER)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn audio(dir: &Path) -> PathBuf {
        let p = dir.join("01 Track.flac");
        fs::write(&p, b"not really audio").unwrap();
        p
    }

    #[test]
    fn sidecar_path_never_returns_the_audio_path() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        for ext in ["lrc", "txt"] {
            assert_ne!(sidecar_path(&audio, ext), audio);
        }
    }

    #[test]
    fn state_none_when_no_sidecar() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        assert_eq!(sidecar_state(&audio), SidecarState::None);
    }

    #[test]
    fn state_synced_when_lrc_has_timestamps() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(lrc_path(&audio), "[00:01.00]Hello\n[00:02.00]World\n").unwrap();
        assert_eq!(sidecar_state(&audio), SidecarState::Synced);
    }

    #[test]
    fn state_plain_when_lrc_has_no_timestamps() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(lrc_path(&audio), "[ar:Some Artist]\nHello\nWorld\n").unwrap();
        assert_eq!(sidecar_state(&audio), SidecarState::Plain);
    }

    #[test]
    fn state_plain_when_only_txt_exists() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(txt_path(&audio), "Hello\nWorld\n").unwrap();
        assert_eq!(sidecar_state(&audio), SidecarState::Plain);
    }

    #[test]
    fn state_synced_when_instrumental_marker_present() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(lrc_path(&audio), INSTRUMENTAL_MARKER).unwrap();
        assert_eq!(sidecar_state(&audio), SidecarState::Synced);
    }

    #[test]
    fn write_synced_removes_stale_txt_unless_keep_plain() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(txt_path(&audio), "old plain\n").unwrap();

        write_synced(&audio, "[00:01.00]Hi\n", false).unwrap();
        assert!(lrc_path(&audio).exists());
        assert!(!txt_path(&audio).exists());
    }

    #[test]
    fn write_synced_keeps_txt_when_requested() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(txt_path(&audio), "old plain\n").unwrap();

        write_synced(&audio, "[00:01.00]Hi\n", true).unwrap();
        assert!(txt_path(&audio).exists());
    }

    #[test]
    fn instrumental_marker_never_clobbers_real_plain_lyrics() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(txt_path(&audio), "Real lyrics\n").unwrap();

        let wrote = write_instrumental_marker_if_absent(&audio).unwrap();
        assert!(!wrote);
        assert!(!lrc_path(&audio).exists());
        assert_eq!(
            fs::read_to_string(txt_path(&audio)).unwrap(),
            "Real lyrics\n"
        );
    }

    #[test]
    fn instrumental_marker_never_clobbers_real_synced_lyrics() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());
        fs::write(lrc_path(&audio), "[00:01.00]Real synced lyrics\n").unwrap();

        let wrote = write_instrumental_marker_if_absent(&audio).unwrap();
        assert!(!wrote);
        assert_eq!(
            fs::read_to_string(lrc_path(&audio)).unwrap(),
            "[00:01.00]Real synced lyrics\n"
        );
    }

    #[test]
    fn instrumental_marker_written_as_lrc_when_absent() {
        let dir = tempdir().unwrap();
        let audio = audio(dir.path());

        let wrote = write_instrumental_marker_if_absent(&audio).unwrap();
        assert!(wrote);
        assert!(!txt_path(&audio).exists());
        assert_eq!(
            fs::read_to_string(lrc_path(&audio)).unwrap(),
            INSTRUMENTAL_MARKER
        );
        assert_eq!(sidecar_state(&audio), SidecarState::Synced);
    }
}
