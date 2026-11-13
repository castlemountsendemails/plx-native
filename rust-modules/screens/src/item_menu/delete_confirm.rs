//! The confirm the card menu's Delete row opens: a `DecisionAlert` the menu hosts, Cancel focused
//! first. The menu does not dismiss on the Delete row; only the Delete answer reports the action.
//!
//! The same shape as `plaintext_question::PlaintextAlert` (a decision alert plus the focus group
//! its two answers form, which traps focus while open), with a yes/no answer in place of a session
//! command. The alert's question must be `'static`, so it is a fixed line and the item's title and
//! what is deleted ride in the body paragraphs.

use std::borrow::Cow;
use std::cell::Cell;
use std::os::raw::c_int;

use plx_machine::machine::{
    Cx, Edge, Effects, EntryId, FocusKey, GroupId, Handled, Host, InputEvent, InputKind, Key,
    MachineId,
};
use plx_platform::i18n::msg;
use plx_ui::decision_alert::{Choice, DecisionAlert};
use plx_ui::screen::{
    Activate, AxisMask, Dir, DrawFrame, EdgeRule, ElemKind, GroupKind, GroupSpec, Hover, Placed,
    ScreenEvent, Seat, Step, Stop,
};
use plx_ui::{Painter, Rect};

/// The answers' focus group and elements. The menu's own group is `GroupId(0)` and its row keys
/// are 1 to 12, so these collide with nothing.
const GROUP: GroupId = GroupId(1);
pub(super) const CANCEL: u32 = 100;
pub(super) const AFFIRM: u32 = 101;

/// What [`DeleteConfirm::step`] made of an event.
pub enum Verdict {
    /// Not the confirm's: the menu handles the event as it always does.
    Pass,
    /// The confirm decided the event; return this.
    Done(Handled),
    /// The person answered: `true` is Delete, `false` is Cancel or Back.
    Answer(bool),
}

/// The body paragraphs: the title, then what is deleted. A movie says how many versions when there
/// are several; a show says how many episodes when the server gave a count.
pub fn paragraphs(title: &str, kind: c_int, versions: u32, leaf_count: Option<u32>) -> Vec<Cow<'static, str>> {
    let scope: Cow<'static, str> = match kind {
        1 => match leaf_count.filter(|&n| n > 0) {
            Some(n) => Cow::Owned(msg::browse_menu_delete_show(i64::from(n))),
            None => Cow::Borrowed(msg::browse_menu_delete_show_all()),
        },
        _ if versions > 1 => Cow::Owned(msg::browse_menu_delete_versions(i64::from(versions))),
        _ => Cow::Borrowed(msg::browse_menu_delete_movie()),
    };
    vec![Cow::Owned(title.to_string()), scope]
}

pub struct DeleteConfirm {
    alert: DecisionAlert,
    /// The answers' rects as last drawn; the `Focusable` hooks answer with `&self`.
    frames: Cell<Option<(Rect, Rect)>>,
}

impl DeleteConfirm {
    pub const fn new() -> Self {
        Self { alert: DecisionAlert::new(), frames: Cell::new(None) }
    }

    /// Open: focus belongs to the answers. The input gate.
    pub fn is_open(&self) -> bool {
        self.alert.is_open()
    }

    /// Open or still fading out: the draw and pointer-trap gate.
    pub fn visible(&self) -> bool {
        self.alert.visible()
    }

    fn owns(elem: u32) -> bool {
        elem == CANCEL || elem == AFFIRM
    }

    /// Ask, seating focus on Cancel (every `DecisionAlert` open starts there, so an OK held
    /// through the press that opened it never answers yes).
    pub fn open<H: Host>(&mut self, body: Vec<Cow<'static, str>>, to: MachineId, fx: &mut Effects<'_, H>) {
        self.alert.open_card(msg::browse_menu_delete_question_c(), body, plx_ui::decision_alert::Answers::Two);
        super::super::plaintext_question::enter_group(fx, to, GROUP);
    }

    pub fn update(&mut self, dt: f32) {
        self.alert.update(dt);
    }

    fn answer(&mut self, delete: bool) -> Verdict {
        self.alert.dismiss();
        Verdict::Answer(delete)
    }

    /// Route one screen event through the confirm first. While open it answers its own keys (Back
    /// is Cancel; Up and Down go nowhere), follows focus between its answers and takes the press on
    /// one; while it fades out every key, click and commit is swallowed, so nothing under the
    /// dissolving panel fires through it.
    pub fn step<H: Host<Elem = u32>>(&mut self, ev: &ScreenEvent<H>, cx: &Cx<'_, H>) -> Verdict {
        match ev {
            ScreenEvent::FocusMoved { to, .. } if self.is_open() && Self::owns(to.elem) => {
                self.alert.set_choice(if to.elem == AFFIRM { Choice::Destructive } else { Choice::Cancel });
                Verdict::Done(Handled::Yes)
            }
            ScreenEvent::Activate(_) if self.visible() => Verdict::Done(Handled::Yes),
            ScreenEvent::PressCommit(_) => match cx.focus.current.map(|k| k.elem) {
                // Gate on still being OPEN: a Back queued ahead of an armed commit has already
                // answered, and the stale focus key must not answer twice.
                Some(elem) if Self::owns(elem) && self.is_open() => self.answer(elem == AFFIRM),
                _ if self.visible() => Verdict::Done(Handled::Yes),
                _ => Verdict::Pass,
            },
            ScreenEvent::Input(InputEvent { kind: InputKind::Key { key, edge, .. }, .. }) if self.visible() => {
                if !self.is_open() {
                    return Verdict::Done(Handled::Yes);
                }
                match key {
                    Key::Back if *edge == Edge::Down => self.answer(false),
                    Key::Back | Key::Up | Key::Down => Verdict::Done(Handled::Yes),
                    _ => Verdict::Done(Handled::No),
                }
            }
            _ => Verdict::Pass,
        }
    }

    fn key(entry: EntryId, elem: u32) -> FocusKey<u32> {
        FocusKey { entry, elem }
    }

    fn rect(&self, elem: u32) -> Rect {
        self.frames
            .get()
            .map(|(cancel, affirm)| if elem == AFFIRM { affirm } else { cancel })
            .unwrap_or(Rect::FULL)
    }

    /// `Focusable::groups`: while open the answers are the ONLY group; `true` means the menu adds none.
    pub fn groups(&self, out: &mut Vec<GroupSpec>) -> bool {
        if !self.is_open() {
            return false;
        }
        out.push(GroupSpec {
            id: GROUP,
            kind: GroupKind::Row { wrap: false },
            seat: Seat::First,
            reachable: AxisMask::BOTH,
            edge: [EdgeRule::Stop; 4],
            extent: self.rect(CANCEL).union(self.rect(AFFIRM)),
            len: 2,
            elem: ElemKind::Control,
        });
        true
    }

    /// `Focusable::group_of`: `Some(answer)` while open, `Some(None)` for a key that is not an
    /// answer (unfocusable now), `None` when closed (the menu's).
    pub fn group_of(&self, key: u32) -> Option<Option<GroupId>> {
        self.is_open().then(|| Self::owns(key).then_some(GROUP))
    }

    /// `Focusable::neighbour` while open: Left and Right between the two answers.
    pub fn neighbour(&self, key: FocusKey<u32>, dir: Dir) -> Option<Step<u32>> {
        if !self.is_open() {
            return None;
        }
        Some(match (key.elem, dir) {
            (CANCEL, Dir::Right) => Step::Move(Self::key(key.entry, AFFIRM)),
            (AFFIRM, Dir::Left) => Step::Move(Self::key(key.entry, CANCEL)),
            _ => Step::Edge,
        })
    }

    /// `Focusable::place` while open.
    pub fn place(&self, key: u32) -> Option<Option<Placed>> {
        if !self.is_open() {
            return None;
        }
        Some(Self::owns(key).then(|| {
            let rect = self.rect(key);
            Placed { rect, rest_rect: rect, clip: Rect::FULL, index: Some((key == AFFIRM) as u32) }
        }))
    }

    /// `Focusable::reconcile` while open: a stray key bounces to the answer the alert stands on.
    pub fn reconcile(&self, want: FocusKey<u32>) -> Option<FocusKey<u32>> {
        if !self.is_open() {
            return None;
        }
        if Self::owns(want.elem) {
            return Some(want);
        }
        Some(Self::key(want.entry, match self.alert.choice() {
            Choice::Cancel => CANCEL,
            Choice::Destructive => AFFIRM,
        }))
    }

    /// `Focusable::seat` for the answers' group: Cancel.
    pub fn seat(&self, g: GroupId, entry: EntryId) -> Option<FocusKey<u32>> {
        (g == GROUP).then(|| Self::key(entry, CANCEL))
    }

    /// Draw over the menu, and register the answers' stops once the sheet has settled.
    pub fn draw<H: Host<Elem = u32>>(&mut self, f: &mut DrawFrame<'_, '_, H>, entry: EntryId) {
        if !self.alert.visible() {
            return;
        }
        self.alert.draw_scrim();
        self.alert.draw(msg::settings_cancel_c(), msg::settings_delete_c(), f.measure);
        let frames = self.alert.frames(f.measure);
        self.frames.set(Some(frames));
        if self.alert.is_open() && self.alert.settled() {
            for (elem, rect) in [(CANCEL, frames.0), (AFFIRM, frames.1)] {
                f.stop(
                    Painter::root(),
                    Stop {
                        key: Self::key(entry, elem),
                        rect,
                        rest_rect: rect,
                        clip: Rect::FULL,
                        hover: Hover::Focus,
                        activate: Activate::Press,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body names the title first, then what goes: one version says nothing about versions,
    /// several name the count, a show names its episodes (or all of them when the count is unknown).
    #[test]
    fn the_body_names_the_title_and_what_is_deleted() {
        let text = |kind, versions, leaves| {
            paragraphs("Heat", kind, versions, leaves).iter().map(|p| p.to_string()).collect::<Vec<_>>()
        };
        let one = text(0, 1, None);
        assert_eq!(one[0], "Heat");
        assert_eq!(one[1], msg::browse_menu_delete_movie());
        assert_eq!(text(0, 0, None)[1], msg::browse_menu_delete_movie(), "no Media is one version");
        let several = text(0, 3, None);
        assert_eq!(several[1], msg::browse_menu_delete_versions(3));
        assert!(several[1].contains('3'));
        let show = text(1, 0, Some(24));
        assert_eq!(show[1], msg::browse_menu_delete_show(24));
        assert!(show[1].contains("24"));
        assert_eq!(text(1, 0, None)[1], msg::browse_menu_delete_show_all());
        assert_eq!(text(1, 0, Some(0))[1], msg::browse_menu_delete_show_all(), "0 episodes is no count");
    }

    /// While closed the confirm claims nothing, so the menu's own hooks answer.
    #[test]
    fn closed_it_claims_nothing() {
        let c = DeleteConfirm::new();
        let mut out = Vec::new();
        assert!(!c.groups(&mut out) && out.is_empty());
        assert!(c.group_of(CANCEL).is_none() && c.place(CANCEL).is_none() && c.reconcile(FocusKey { entry: EntryId(0), elem: 1 }).is_none());
    }
}
