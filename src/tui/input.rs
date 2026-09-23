// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Keys to actions. See `help.rs` for the table shown to the listener; a test there proves
//! every key it advertises resolves here.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::app::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePlay,
    /// `true` for the short (5s) seek, `false` for the long (10s) one.
    SeekBackward(bool),
    SeekForward(bool),
    PreviousLine,
    NextLine,
    /// `true` for the short (100ms) nudge, `false` for the long (500ms) one.
    NudgeEarlier(bool),
    NudgeLater(bool),
    /// Snap the clock to the nearest line's timestamp, pressed as that line is sung.
    TapSync,
    Restart,
    ReplayCountdown,
    ToggleHelp,
    Quit,
}

/// Resolve one key press into an `Action`, given the mode it was pressed in.
///
/// `Help` only listens for the keys that close it; a countdown still accepts every key
/// (`TogglePlay` cancels it — see `App::apply`), since sitting through an unskippable countdown
/// to reach the quit key would be a poor way to leave a mistaken invocation.
#[must_use]
pub fn action(key: &KeyEvent, mode: Mode) -> Option<Action> {
    if mode == Mode::Help {
        return match key.code {
            KeyCode::Char('?' | 'q') | KeyCode::Esc => Some(Action::ToggleHelp),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char(' ') => Some(Action::TogglePlay),
        KeyCode::Left | KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(Action::SeekBackward(false))
        }
        KeyCode::Left | KeyCode::Char('h') => Some(Action::SeekBackward(true)),
        KeyCode::Right | KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(Action::SeekForward(false))
        }
        KeyCode::Right | KeyCode::Char('l') => Some(Action::SeekForward(true)),
        KeyCode::Char('H') => Some(Action::SeekBackward(false)),
        KeyCode::Char('L') => Some(Action::SeekForward(false)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::PreviousLine),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::NextLine),
        KeyCode::Char(',') => Some(Action::NudgeEarlier(true)),
        KeyCode::Char('.') => Some(Action::NudgeLater(true)),
        // Matched on the character alone: terminals disagree on whether `<`/`>` arrive with
        // SHIFT set, and either way the listener pressed the same key.
        KeyCode::Char('<') => Some(Action::NudgeEarlier(false)),
        KeyCode::Char('>') => Some(Action::NudgeLater(false)),
        KeyCode::Enter => Some(Action::TapSync),
        KeyCode::Char('0' | 'r') => Some(Action::Restart),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('c') => Some(Action::ReplayCountdown),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn space_toggles_play() {
        assert_eq!(
            action(&key(KeyCode::Char(' ')), Mode::Playing),
            Some(Action::TogglePlay)
        );
    }

    #[test]
    fn plain_and_shifted_seeks_pick_the_right_amount() {
        assert_eq!(
            action(&key(KeyCode::Left), Mode::Playing),
            Some(Action::SeekBackward(true))
        );
        assert_eq!(
            action(&shift(KeyCode::Left), Mode::Playing),
            Some(Action::SeekBackward(false))
        );
        assert_eq!(
            action(&key(KeyCode::Char('l')), Mode::Playing),
            Some(Action::SeekForward(true))
        );
        assert_eq!(
            action(&key(KeyCode::Char('H')), Mode::Playing),
            Some(Action::SeekBackward(false))
        );
        assert_eq!(
            action(&key(KeyCode::Char('L')), Mode::Playing),
            Some(Action::SeekForward(false))
        );
    }

    #[test]
    fn line_jump_keys_resolve() {
        assert_eq!(
            action(&key(KeyCode::Up), Mode::Playing),
            Some(Action::PreviousLine)
        );
        assert_eq!(
            action(&key(KeyCode::Char('k')), Mode::Playing),
            Some(Action::PreviousLine)
        );
        assert_eq!(
            action(&key(KeyCode::Down), Mode::Playing),
            Some(Action::NextLine)
        );
        assert_eq!(
            action(&key(KeyCode::Char('j')), Mode::Playing),
            Some(Action::NextLine)
        );
    }

    #[test]
    fn nudge_keys_resolve() {
        assert_eq!(
            action(&key(KeyCode::Char(',')), Mode::Playing),
            Some(Action::NudgeEarlier(true))
        );
        assert_eq!(
            action(&key(KeyCode::Char('.')), Mode::Playing),
            Some(Action::NudgeLater(true))
        );
        for event in [key(KeyCode::Char('<')), shift(KeyCode::Char('<'))] {
            assert_eq!(
                action(&event, Mode::Playing),
                Some(Action::NudgeEarlier(false))
            );
        }
        for event in [key(KeyCode::Char('>')), shift(KeyCode::Char('>'))] {
            assert_eq!(
                action(&event, Mode::Playing),
                Some(Action::NudgeLater(false))
            );
        }
    }

    #[test]
    fn enter_taps_sync() {
        assert_eq!(
            action(&key(KeyCode::Enter), Mode::Playing),
            Some(Action::TapSync)
        );
    }

    #[test]
    fn restart_and_countdown_keys_resolve() {
        assert_eq!(
            action(&key(KeyCode::Char('0')), Mode::Playing),
            Some(Action::Restart)
        );
        assert_eq!(
            action(&key(KeyCode::Char('r')), Mode::Playing),
            Some(Action::Restart)
        );
        assert_eq!(
            action(&key(KeyCode::Char('c')), Mode::Playing),
            Some(Action::ReplayCountdown)
        );
    }

    #[test]
    fn quit_keys_resolve_outside_help() {
        assert_eq!(
            action(&key(KeyCode::Char('q')), Mode::Playing),
            Some(Action::Quit)
        );
        assert_eq!(
            action(&key(KeyCode::Esc), Mode::Playing),
            Some(Action::Quit)
        );
        assert_eq!(
            action(&ctrl(KeyCode::Char('c')), Mode::Playing),
            Some(Action::Quit)
        );
    }

    #[test]
    fn help_opens_and_a_countdown_still_accepts_every_key() {
        assert_eq!(
            action(&key(KeyCode::Char('?')), Mode::Playing),
            Some(Action::ToggleHelp)
        );
        assert_eq!(
            action(&key(KeyCode::Char(' ')), Mode::Countdown { step: 1 }),
            Some(Action::TogglePlay)
        );
    }

    #[test]
    fn help_mode_only_answers_to_the_keys_that_close_it() {
        assert_eq!(
            action(&key(KeyCode::Char('?')), Mode::Help),
            Some(Action::ToggleHelp)
        );
        assert_eq!(
            action(&key(KeyCode::Char('q')), Mode::Help),
            Some(Action::ToggleHelp)
        );
        assert_eq!(
            action(&key(KeyCode::Esc), Mode::Help),
            Some(Action::ToggleHelp)
        );
        assert_eq!(action(&key(KeyCode::Char(' ')), Mode::Help), None);
        assert_eq!(action(&key(KeyCode::Left), Mode::Help), None);
    }

    #[test]
    fn an_unbound_key_resolves_to_nothing() {
        assert_eq!(action(&key(KeyCode::Char('z')), Mode::Playing), None);
    }
}
