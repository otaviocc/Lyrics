// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `lyrics tui`: terminal setup and restore, and the event loop.

pub mod app;
pub mod bigtext;
pub mod clock;
pub mod help;
pub mod input;
pub mod picker;
pub mod view;

use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::lrc;
use crate::theme::Theme;
use crate::tui::app::{App, COUNTDOWN_STEP, Mode, Song};

const IDLE_TICK: Duration = Duration::from_millis(200);

pub fn song_from_lrc(path: &Path) -> Result<Song> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(song_from_synced(path, lrc::parse_synced(&contents)?, None))
}

pub fn songs_from_dir(dir: &Path) -> Result<Vec<Song>> {
    Ok(lrc::synced_files(dir)?
        .into_iter()
        .map(|(path, synced)| {
            let group = group_of(dir, &path);
            song_from_synced(&path, synced, group)
        })
        .collect())
}

fn group_of(root: &Path, path: &Path) -> Option<String> {
    let relative = path.parent()?.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(relative.to_string_lossy().into_owned())
}

fn song_from_synced(path: &Path, synced: lrc::Synced, group: Option<String>) -> Song {
    let lrc::Synced {
        title,
        artist,
        lines,
    } = synced;
    let title = title.unwrap_or_else(|| {
        path.file_stem().map_or_else(
            || path.display().to_string(),
            |stem| stem.to_string_lossy().into_owned(),
        )
    });
    Song {
        title,
        artist,
        group,
        lines,
    }
}

pub(crate) fn clamp_fraction(total: u16, fraction: u16, min: u16, max: u16) -> u16 {
    total
        .saturating_mul(fraction)
        .saturating_div(100)
        .clamp(min, max)
        .min(total)
}

pub fn run(song: Song, theme: Theme, counter: bool) -> Result<()> {
    run_app(App::new(song, theme, counter))
}

pub fn run_app(mut app: App) -> Result<()> {
    let mut terminal = ratatui::try_init().context("cannot open the terminal")?;

    let (tx, rx) = mpsc::channel();
    spawn_input(tx);

    let outcome = event_loop(&mut terminal, &mut app, &rx);

    ratatui::restore();
    outcome
}

enum Wake {
    Input(Event),
    InputLost,
}

fn spawn_input(tx: Sender<Wake>) {
    std::thread::spawn(move || {
        loop {
            let wake = if let Ok(event) = event::read() {
                Wake::Input(event)
            } else {
                let _ = tx.send(Wake::InputLost);
                return;
            };
            if tx.send(wake).is_err() {
                return;
            }
        }
    });
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    rx: &Receiver<Wake>,
) -> Result<()> {
    loop {
        let now = Instant::now();
        let elapsed = app.clock.now(now);
        terminal
            .draw(|frame| view::draw(frame, app, elapsed))
            .context("cannot draw")?;

        let timed_out = if matches!(app.mode, Mode::Countdown { .. }) {
            match rx.recv_timeout(COUNTDOWN_STEP) {
                Ok(wake) => {
                    handle(app, &wake)?;
                    false
                }
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else if (app.browsing().is_none() && app.clock.is_playing()) || app.notice(now).is_some()
        {
            match rx.recv_timeout(IDLE_TICK) {
                Ok(wake) => {
                    handle(app, &wake)?;
                    false
                }
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else {
            match rx.recv() {
                Ok(wake) => {
                    handle(app, &wake)?;
                    false
                }
                Err(_) => return Ok(()),
            }
        };
        if timed_out && matches!(app.mode, Mode::Countdown { .. }) {
            app.advance_countdown(Instant::now());
        }
        while let Ok(wake) = rx.try_recv() {
            handle(app, &wake)?;
        }

        if app.quit {
            return Ok(());
        }
    }
}

fn handle(app: &mut App, wake: &Wake) -> Result<()> {
    match wake {
        Wake::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => {
            if let Some(action) = crate::tui::input::action(key, app.mode) {
                app.apply(action, Instant::now());
            }
        }
        Wake::Input(_) => {}
        Wake::InputLost => anyhow::bail!("cannot read keyboard input"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_lrc_is_grouped_by_its_directory_relative_to_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let album = root.join("Metallica").join("Ride the Lightning");
        fs::create_dir_all(&album).unwrap();
        fs::write(root.join("01 Top.lrc"), "[00:01.00]a\n").unwrap();
        fs::write(album.join("05 Fade to Black.lrc"), "[00:01.00]b\n").unwrap();

        let songs = songs_from_dir(root).unwrap();
        assert_eq!(songs.len(), 2);
        assert_eq!(songs.first().unwrap().group, None);
        assert_eq!(
            songs.get(1).unwrap().group.as_deref(),
            Some("Metallica/Ride the Lightning")
        );
    }
}
