//! The exit-confirm: "Exit PlxNative?" over whatever page is on top, before BACK at a root the
//! television would not hand back to, or EXIT, actually ends the process.
//!
//! It has no owning screen, so it is `App`-level state rather than a field on one, driven
//! directly by `run()`'s raw key ladder: [`ExitConfirm::is_open`] is asked before the dispatcher
//! is asked whether it owns the key, so a page underneath takes no input while this is up.

use plx_ui::consts::{SDLK_LEFT, SDLK_RIGHT};
use plx_ui::decision_alert::{Choice, DecisionAlert};

const QUESTION: &core::ffi::CStr = c"Exit PlxNative?";
const CANCEL: &core::ffi::CStr = c"Cancel";
const EXIT: &core::ffi::CStr = c"Exit";

pub(crate) struct ExitConfirm {
    alert: DecisionAlert,
}

impl ExitConfirm {
    pub(crate) fn new() -> Self {
        Self { alert: DecisionAlert::new() }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.alert.is_open()
    }

    /// Raise the alert, always on Cancel.
    pub(crate) fn open(&mut self) {
        self.alert.open(QUESTION);
    }

    /// LEFT/RIGHT between the two answers, clamped rather than wrapped — pure, so it is
    /// host-testable without a window.
    pub(crate) fn move_focus(&mut self, sym: u32) {
        let choice = match sym {
            SDLK_LEFT => Choice::Cancel,
            SDLK_RIGHT => Choice::Destructive,
            _ => self.alert.choice(),
        };
        self.alert.set_choice(choice);
    }

    /// Commit the focused answer and take the alert down. `true` when it was *Exit*.
    pub(crate) fn on_ok(&mut self) -> bool {
        let exit = self.alert.choice() == Choice::Destructive;
        self.alert.dismiss();
        exit
    }

    /// BACK: dismiss as Cancel, like every other decision alert in the app.
    pub(crate) fn on_back(&mut self) {
        self.alert.dismiss();
    }

    pub(crate) fn update(&mut self, dt: f32) {
        self.alert.update(dt);
    }

    pub(crate) fn draw(&mut self) {
        if !self.alert.visible() {
            return;
        }
        self.alert.draw_scrim();
        let measure = plx_gfx::text::TtfMeasure;
        self.alert.draw(CANCEL, EXIT, &measure);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_ui::consts::{SDLK_DOWN, SDLK_UP};

    #[test]
    fn opens_on_cancel_and_left_right_walk_the_pair_without_wrapping() {
        let _serial = plx_base::testlock::serial();
        let mut c = ExitConfirm::new();
        c.open();
        assert!(c.is_open());
        assert_eq!(c.alert.choice(), Choice::Cancel);
        c.move_focus(SDLK_LEFT);
        assert_eq!(c.alert.choice(), Choice::Cancel, "no wrap onto Exit");
        c.move_focus(SDLK_RIGHT);
        assert_eq!(c.alert.choice(), Choice::Destructive);
        c.move_focus(SDLK_RIGHT);
        assert_eq!(c.alert.choice(), Choice::Destructive, "no wrap off the end");
        c.move_focus(SDLK_LEFT);
        assert_eq!(c.alert.choice(), Choice::Cancel);
    }

    #[test]
    fn an_unrelated_key_does_not_move_the_ring() {
        let _serial = plx_base::testlock::serial();
        let mut c = ExitConfirm::new();
        c.open();
        c.move_focus(SDLK_RIGHT);
        c.move_focus(SDLK_UP);
        c.move_focus(SDLK_DOWN);
        assert_eq!(c.alert.choice(), Choice::Destructive);
    }

    #[test]
    fn ok_on_cancel_closes_without_asking_to_quit() {
        let _serial = plx_base::testlock::serial();
        let mut c = ExitConfirm::new();
        c.open();
        assert!(!c.on_ok());
        assert!(!c.is_open());
    }

    #[test]
    fn ok_on_exit_closes_and_asks_to_quit() {
        let _serial = plx_base::testlock::serial();
        let mut c = ExitConfirm::new();
        c.open();
        c.move_focus(SDLK_RIGHT);
        assert!(c.on_ok());
        assert!(!c.is_open());
    }

    #[test]
    fn back_cancels_without_asking_to_quit_and_reopening_resets_to_cancel() {
        let _serial = plx_base::testlock::serial();
        let mut c = ExitConfirm::new();
        c.open();
        c.move_focus(SDLK_RIGHT);
        c.on_back();
        assert!(!c.is_open());
        c.open();
        assert_eq!(c.alert.choice(), Choice::Cancel);
    }
}
