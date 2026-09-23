// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! `lyrics tui`: terminal setup and restore, and the event loop.

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

const IDLE_TICK: Duration = Duration::from_millis(200);

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
