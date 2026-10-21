//! The nav rail: the column of destinations down the left edge of every main screen (Home,
//! Search, Watchlist, the favorite libraries, then Profile at the foot). Collapsed, it shows marks
//! only, inside the left margin; while focus is in it, it widens to show labels and the page slides
//! right by the difference (`Rig::content_dx`) rather than being covered. The application owns one
//! [`RailRender`], publishes the items and draws it in the chrome pass.

use std::ffi::CString;

use crate::consts::{MARGIN_X, MARGIN_Y, SCR_H};
use crate::containers::tabs::StripMember;
use crate::icons::Icon;
use crate::{theme, Painter, Rect, Spring};

pub const COLLAPSED_W: f32 = MARGIN_X;
pub const EXPANDED_W: f32 = 480.0;
/// How far the page slides right with the rail fully expanded.
pub const PUSH: f32 = EXPANDED_W - COLLAPSED_W;
const ITEM_H: f32 = 64.0;
const PITCH: f32 = 72.0;
const MARK_D: f32 = 32.0;
const INSET: f32 = 8.0;
const LABEL_X: f32 = COLLAPSED_W + theme::space::XS;
const LABEL_TRACKING: f32 = 1.5;
const K_EXPAND: f32 = 240.0;

pub enum RailMark {
    Icon(Icon),
    /// A library: the first letter of its name in a square.
    Badge(CString),
    /// The profile: its initial in an accent-coloured circle. This item sits at the rail's foot.
    Avatar(CString),
}

pub struct RailItem {
    pub key: u32,
    pub mark: RailMark,
    pub label: CString,
    /// Tracked as a capital label (the destinations); the profile's name is drawn as written.
    pub caps: bool,
}

/// The rail's width at expansion `e` (0 collapsed, 1 expanded).
pub fn width(e: f32) -> f32 {
    COLLAPSED_W + PUSH * e.clamp(0.0, 1.0)
}

fn item_y(items: &[RailItem], i: usize) -> f32 {
    if matches!(items[i].mark, RailMark::Avatar(_)) {
        return SCR_H - MARGIN_Y - ITEM_H;
    }
    // ponytail: a rail too long for the screen tightens its pitch instead of scrolling its middle;
    // fine to about a dozen favorites, after which the middle needs a scroll.
    let top = items.iter().filter(|item| !matches!(item.mark, RailMark::Avatar(_))).count().max(1);
    let room = SCR_H - 2.0 * MARGIN_Y - ITEM_H - PITCH;
    MARGIN_Y + i as f32 * PITCH.min(room / top as f32)
}

fn item_rect(items: &[RailItem], i: usize, e: f32) -> Rect {
    Rect::new(INSET, item_y(items, i), width(e) - 2.0 * INSET, ITEM_H)
}

pub struct RailRender {
    expand: Spring,
    /// A running Watchlist add-pulse: the item's key and how long it has run.
    pulse: Option<(u32, f32)>,
}

impl Default for RailRender {
    fn default() -> Self {
        Self::new()
    }
}

impl RailRender {
    pub fn new() -> Self {
        Self { expand: Spring::at(0.0), pulse: None }
    }

    /// 0 collapsed .. 1 expanded, as drawn this frame.
    pub fn expand_pos(&self) -> f32 {
        self.expand.pos.clamp(0.0, 1.0)
    }

    /// The page's slide this frame.
    pub fn content_dx(&self) -> f32 {
        PUSH * self.expand_pos()
    }

    pub fn pulse(&mut self, key: u32) {
        self.pulse = Some((key, 0.0));
        plx_machine::idle::invalidate();
    }

    pub fn update(&mut self, expanded: bool, dt: f32) {
        let target = if expanded { 1.0 } else { 0.0 };
        self.expand.step(target, K_EXPAND, dt);
        self.expand.land(target, dt);
        // Decorative motion, as the strip's pulse was: it moves no layout and asks for its own frames.
        if let Some((key, age)) = self.pulse.take() {
            plx_machine::idle::decorative(|| {
                if age + dt < crate::widgets::TAB_PULSE_S {
                    self.pulse = Some((key, age + dt));
                    plx_machine::idle::note_motion();
                }
            });
        }
    }

    /// The items as focus members, at the rects drawn this frame.
    pub fn members(&self, items: &[RailItem], out: &mut Vec<StripMember<u32>>) {
        out.clear();
        let e = self.expand_pos();
        out.extend((0..items.len()).map(|i| StripMember::new(items[i].key, item_rect(items, i, e))));
    }

    /// The item `key`'s rect, for a surface that hangs from it (the account menu from Profile).
    pub fn item_rect(&self, items: &[RailItem], key: u32) -> Option<Rect> {
        let i = items.iter().position(|item| item.key == key)?;
        Some(item_rect(items, i, self.expand_pos()))
    }

    /// Snap shut with no motion: the page on view has no rail to push it aside.
    pub fn collapse(&mut self) {
        self.expand.jump(0.0);
    }

    /// `active` is the item of the screen on view; `focused` the item holding focus, if any.
    pub fn draw(&self, p: Painter, items: &[RailItem], active: Option<u32>, focused: Option<u32>) {
        p.rect(Rect::new(0.0, 0.0, width(self.expand_pos()), SCR_H), 0.0, theme::PANEL_TOP, theme::PANEL_BOT, 0.0);
        for item in items {
            self.draw_item(p, items, item.key, active, focused);
        }
    }

    /// One item without the panel behind it: also the lit copy an opener lift draws above a dim.
    pub fn draw_item(&self, p: Painter, items: &[RailItem], key: u32, active: Option<u32>, focused: Option<u32>) {
        let Some(i) = items.iter().position(|item| item.key == key) else { return };
        let item = &items[i];
        let e = self.expand_pos();
        let r = item_rect(items, i, e);
        let is_focused = focused == Some(key);
        let is_active = active == Some(key);
        if is_focused {
            p.rrect(r, r.h * 0.5, r.h * 0.5, theme::ACCENT);
        }
        if is_active {
            p.rrect(Rect::new(0.0, r.y + 16.0, 4.0, r.h - 32.0), 2.0, 2.0, theme::ACCENT);
        }
        let ink = if is_focused { theme::INK_ON_ACCENT }
            else if is_active { theme::TEXT_PRIMARY }
            else { theme::CONTROL_TEXT_REST };
        self.draw_mark(p, item, r, ink, is_focused);
        if e > 0.0 {
            let label = p.alpha(e * e);
            label.clip(Rect::new(0.0, r.y, width(e) - theme::space::MD, r.h));
            let ty = plx_gfx::text::text_vcenter_y(theme::size::LABEL, 1, r.cy());
            let spacing = if item.caps { LABEL_TRACKING } else { 0.0 };
            label.text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::LABEL, ink, 0, 1, spacing);
            label.clip_clear();
        }
    }

    fn draw_mark(&self, p: Painter, item: &RailItem, r: Rect, ink: [f32; 4], focused: bool) {
        let m = Rect::new(COLLAPSED_W * 0.5 - MARK_D * 0.5, r.cy() - MARK_D * 0.5, MARK_D, MARK_D);
        match &item.mark {
            RailMark::Icon(icon) => {
                let env = self.pulse.filter(|(key, _)| *key == item.key)
                    .map_or(0.0, |(_, age)| crate::widgets::tab_pulse_envelope(age));
                crate::widgets::draw_tab_mark(p, *icon, m, ink, env);
            }
            RailMark::Badge(letter) => {
                p.rring(m, 6.0, 2.0, ink);
                let ty = plx_gfx::text::text_vcenter_y(theme::size::CAPTION, 1, m.cy());
                p.text(letter.as_ptr(), m.cx(), ty, theme::size::CAPTION, ink, 1, 1);
            }
            RailMark::Avatar(initial) => {
                let (fill, letter_ink) = if focused { (theme::INK_ON_ACCENT, theme::ACCENT) }
                    else { (theme::ACCENT, theme::INK_ON_ACCENT) };
                p.rrect(m, MARK_D * 0.5, MARK_D * 0.5, fill);
                let ty = plx_gfx::text::text_vcenter_y(theme::size::CAPTION, 1, m.cy());
                p.text(initial.as_ptr(), m.cx(), ty, theme::size::CAPTION, letter_ink, 1, 1);
            }
        }
    }
}
