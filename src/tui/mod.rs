// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `lyrics tui`: a full-screen teleprompter.
//!
//! The current lyric line stays pinned to the screen's center, driven by a clock the listener
//! controls (Space plays/pauses, the seek keys nudge it) rather than by wall-clock time synced
//! to anything external. This is why invariant 3 in `AGENTS.md` is framed the way it is: this
//! command's whole job is to display lyrics on the screen, on the alternate screen only, never
//! to stdout/stderr as log output.

pub mod app;
pub mod bigtext;
pub mod clock;
pub mod help;
pub mod input;
pub mod view;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::theme::Theme;
use crate::tui::app::{App, COUNTDOWN_STEP, Mode, Song};

/// How often to redraw while nothing is happening, so the running clock's status-bar display
/// stays live and a transient notice clears on time.
const IDLE_TICK: Duration = Duration::from_millis(200);

/// Run the teleprompter until the listener quits.
///
/// # Errors
///
/// Returns an error if the terminal can't be initialized, if drawing fails, or if the input
/// thread loses its connection to the terminal.
pub fn run(song: Song, theme: Theme, counter: bool) -> Result<()> {
    let mut terminal = ratatui::try_init().context("cannot open the terminal")?;

    let (tx, rx) = mpsc::channel();
    spawn_input(tx);

    let mut app = App::new(song, theme, counter);
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

        // Redraws are only worth polling for on a timer while something on screen is actually
        // moving on its own: the countdown, the running clock (status-bar time, the current
        // line), or a transient notice waiting to expire. Otherwise (paused, nothing pending)
        // there is nothing to repaint until the next key press, so block on it instead of
        // waking 5x/second to redraw an unchanged screen.
        let timed_out = if matches!(app.mode, Mode::Countdown { .. }) {
            match rx.recv_timeout(COUNTDOWN_STEP) {
                Ok(wake) => {
                    handle(app, &wake)?;
                    false
                }
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else if app.clock.is_playing() || app.notice(now).is_some() {
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
        // Filter to `Press`: some terminals (notably Windows, and any with the kitty keyboard
        // protocol enabled) also report `Release`/`Repeat`, which would otherwise double up
        // every action.
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
