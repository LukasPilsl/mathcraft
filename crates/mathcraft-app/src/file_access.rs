//! The file-access switches of one open worksheet: `Allow reading files` and `Allow writing files`.
//!
//! Rules (CEO decision):
//! - Both are off whenever a worksheet is opened, and are never saved in the `.mcdx` or remembered
//!   across sessions: [`FileAccess::reset`] runs on every open.
//! - The first time a switch is enabled in a session, a confirmation dialog explains the risk.
//!   Declining leaves it off. Once confirmed, enabling it again in the same session needs no dialog.
//! - While calculation is automatic, a refresh (an edit) never writes: writing is off for it and
//!   the worksheet says to press F9. An explicit calculate (F9 or Calculate) passes the switch on.
//!
//! This module is the state only; the window that shows the switches and the dialogs is thin UI
//! over it (see `app.rs`).

use mathcraft_core::eval::builtins::fileio::FileSwitches;

/// One of the two switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    Read,
    Write,
}

/// The result of asking to change a switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// The switch is now on (it was confirmed earlier in this session).
    On,
    /// The switch is off. Nothing to confirm.
    Off,
    /// The switch needs its confirmation dialog first. Call [`FileAccess::confirm`] or
    /// [`FileAccess::cancel`] after the user answers.
    Confirm(Switch),
}

/// The file-access state of one worksheet session.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileAccess {
    read_on: bool,
    write_on: bool,
    read_confirmed: bool,
    write_confirmed: bool,
    pending: Option<Switch>,
}

impl FileAccess {
    /// All off, nothing confirmed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when a worksheet opens: everything off, and the confirmations are forgotten.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_on(&self, switch: Switch) -> bool {
        match switch {
            Switch::Read => self.read_on,
            Switch::Write => self.write_on,
        }
    }

    /// The switch whose confirmation dialog is open, if any.
    pub fn pending(&self) -> Option<Switch> {
        self.pending
    }

    /// The user ticks (`want_on`) or unticks a switch. Turning a switch off needs no dialog.
    pub fn request(&mut self, switch: Switch, want_on: bool) -> Request {
        if !want_on {
            self.set(switch, false);
            if self.pending == Some(switch) {
                self.pending = None;
            }
            return Request::Off;
        }
        if self.is_confirmed(switch) {
            self.set(switch, true);
            return Request::On;
        }
        self.pending = Some(switch);
        Request::Confirm(switch)
    }

    /// The user accepts the confirmation dialog: the pending switch is on from now on.
    pub fn confirm(&mut self) {
        if let Some(switch) = self.pending.take() {
            match switch {
                Switch::Read => self.read_confirmed = true,
                Switch::Write => self.write_confirmed = true,
            }
            self.set(switch, true);
        }
    }

    /// The user declines the confirmation dialog: the switch stays off.
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// The switches for one evaluation. Writing is passed on only for an explicit calculate
    /// (`explicit`: F9 or the Calculate button); a refresh that an edit causes never writes.
    pub fn switches(&self, explicit: bool) -> FileSwitches {
        FileSwitches {
            read: self.read_on,
            write: self.write_on && explicit,
        }
    }

    /// Whether writing is switched on but waits for an explicit calculate (the notice case).
    pub fn writes_wait_for_calculate(&self, explicit: bool) -> bool {
        self.write_on && !explicit
    }

    fn is_confirmed(&self, switch: Switch) -> bool {
        match switch {
            Switch::Read => self.read_confirmed,
            Switch::Write => self.write_confirmed,
        }
    }

    fn set(&mut self, switch: Switch, on: bool) {
        match switch {
            Switch::Read => self.read_on = on,
            Switch::Write => self.write_on = on,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_starts_off() {
        let state = FileAccess::new();
        assert!(!state.is_on(Switch::Read));
        assert!(!state.is_on(Switch::Write));
        assert_eq!(state.switches(true), FileSwitches::default());
    }

    #[test]
    fn the_first_enable_asks_and_declining_keeps_it_off() {
        let mut state = FileAccess::new();
        assert_eq!(
            state.request(Switch::Write, true),
            Request::Confirm(Switch::Write)
        );
        assert_eq!(state.pending(), Some(Switch::Write));
        state.cancel();
        assert!(!state.is_on(Switch::Write));
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn once_confirmed_the_switch_turns_on_without_asking_again() {
        let mut state = FileAccess::new();
        state.request(Switch::Read, true);
        state.confirm();
        assert!(state.is_on(Switch::Read));
        state.request(Switch::Read, false);
        assert!(!state.is_on(Switch::Read));
        assert_eq!(state.request(Switch::Read, true), Request::On);
        assert!(state.is_on(Switch::Read));
    }

    #[test]
    fn the_two_switches_are_confirmed_separately() {
        let mut state = FileAccess::new();
        state.request(Switch::Read, true);
        state.confirm();
        assert_eq!(
            state.request(Switch::Write, true),
            Request::Confirm(Switch::Write),
            "reading was confirmed; writing still needs its own dialog"
        );
    }

    #[test]
    fn turning_a_switch_off_needs_no_dialog_and_drops_a_pending_one() {
        let mut state = FileAccess::new();
        state.request(Switch::Write, true);
        assert_eq!(state.request(Switch::Write, false), Request::Off);
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn writing_waits_for_an_explicit_calculate() {
        let mut state = FileAccess::new();
        state.request(Switch::Write, true);
        state.confirm();
        // An edit (automatic refresh) does not write, and the worksheet says so.
        assert!(!state.switches(false).write);
        assert!(state.writes_wait_for_calculate(false));
        // F9 or Calculate writes.
        assert!(state.switches(true).write);
        assert!(!state.writes_wait_for_calculate(true));
    }

    #[test]
    fn reading_is_not_held_back_by_the_calculate_rule() {
        let mut state = FileAccess::new();
        state.request(Switch::Read, true);
        state.confirm();
        assert!(state.switches(false).read);
    }

    #[test]
    fn opening_a_worksheet_forgets_everything() {
        let mut state = FileAccess::new();
        state.request(Switch::Read, true);
        state.confirm();
        state.request(Switch::Write, true);
        state.reset();
        assert!(!state.is_on(Switch::Read));
        assert_eq!(state.pending(), None);
        assert_eq!(
            state.request(Switch::Read, true),
            Request::Confirm(Switch::Read),
            "a new session asks again"
        );
    }
}
