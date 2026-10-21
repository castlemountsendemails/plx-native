//! The nav rail: the column of destinations down the left edge of every main screen (Home,
//! Search, Watchlist, the favorite libraries, then Profile at the foot). Collapsed, it shows marks
//! only, inside the left margin; while focus is in it, it widens to show labels and the page slides
//! right by the difference (`Rig::content_dx`) rather than being covered. The application owns one
//! [`RailRender`], publishes the items and draws it in the chrome pass.
//!
//! The leading icon items stay fixed at the top and the profile at the foot; everything between
//! (the libraries) is a band that scrolls to keep the focused item, or else the active one, in view,
//! fading out where its items are cut off.

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
const K_SCROLL: f32 = 240.0;
/// Air between the fixed items and the scrolling band, and the height of the band's edge fades.
const BAND_GAP: f32 = theme::space::SM;
const FADE_H: f32 = 40.0;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    Top,
    Band,
    Foot,
}

/// The leading run of icon items is fixed at the top, an avatar sits at the foot, and the rest scroll.
fn zone(items: &[RailItem], i: usize) -> Zone {
    match items[i].mark {
        RailMark::Avatar(_) => Zone::Foot,
        RailMark::Icon(_) if items[..i].iter().all(|item| matches!(item.mark, RailMark::Icon(_))) => Zone::Top,
        _ => Zone::Band,
    }
}

fn top_count(items: &[RailItem]) -> usize {
    (0..items.len()).take_while(|&i| zone(items, i) == Zone::Top).count()
}

/// The scrolling band's visible extent, in screen y.
fn band(items: &[RailItem]) -> Rect {
    let y = MARGIN_Y + top_count(items) as f32 * PITCH + BAND_GAP;
    let bottom = SCR_H - MARGIN_Y - ITEM_H - BAND_GAP;
    Rect::new(0.0, y, EXPANDED_W, (bottom - y).max(0.0))
}

/// How far the band can scroll: its content less what fits.
fn max_scroll(items: &[RailItem]) -> f32 {
    let n = (0..items.len()).filter(|&i| zone(items, i) == Zone::Band).count() as f32;
    (n * PITCH - (PITCH - ITEM_H) - band(items).h).max(0.0)
}

fn item_y(items: &[RailItem], i: usize, scroll: f32) -> f32 {
    match zone(items, i) {
        Zone::Top => MARGIN_Y + i as f32 * PITCH,
        Zone::Foot => SCR_H - MARGIN_Y - ITEM_H,
        Zone::Band => band(items).y + (i - top_count(items)) as f32 * PITCH - scroll,
    }
}

/// Where item `i` may be seen: the whole screen for the fixed items, the band for the rest.
fn item_clip(items: &[RailItem], i: usize) -> Rect {
    if zone(items, i) == Zone::Band { band(items) } else { Rect::FULL }
}

pub struct RailRender {
    expand: Spring,
    scroll: Spring,
    scroll_target: f32,
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
        Self { expand: Spring::at(0.0), scroll: Spring::at(0.0), scroll_target: 0.0, pulse: None }
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

    /// Snap shut with no motion: the page on view has no rail to push it aside.
    pub fn collapse(&mut self) {
        self.expand.jump(0.0);
    }

    /// Step the rail: expanded while `focused` is one of its items, and its band scrolled to keep
    /// `focused`, or else `active`, in view with a row of context on either side.
    pub fn update(&mut self, items: &[RailItem], focused: Option<u32>, active: Option<u32>, dt: f32) {
        let target = if focused.is_some() { 1.0 } else { 0.0 };
        self.expand.step(target, K_EXPAND, dt);
        self.expand.land(target, dt);
        let follow = focused.or(active).and_then(|key| items.iter().position(|item| item.key == key))
            .filter(|&i| zone(items, i) == Zone::Band);
        if let Some(i) = follow {
            let y = (i - top_count(items)) as f32 * PITCH;
            let h = band(items).h;
            if y - PITCH < self.scroll_target { self.scroll_target = y - PITCH; }
            if y + ITEM_H + PITCH > self.scroll_target + h { self.scroll_target = y + ITEM_H + PITCH - h; }
        }
        self.scroll_target = self.scroll_target.clamp(0.0, max_scroll(items));
        self.scroll.step(self.scroll_target, K_SCROLL, dt);
        self.scroll.land(self.scroll_target, dt);
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

    fn item_rect(&self, items: &[RailItem], i: usize) -> Rect {
        Rect::new(INSET, item_y(items, i, self.scroll.pos), width(self.expand_pos()) - 2.0 * INSET, ITEM_H)
    }

    /// The items as focus members, at the rects drawn this frame and clipped to where they show.
    pub fn members(&self, items: &[RailItem], out: &mut Vec<StripMember<u32>>) {
        out.clear();
        out.extend((0..items.len()).map(|i| {
            let r = self.item_rect(items, i);
            StripMember { elem: items[i].key, drawn: r, target: r, clip: item_clip(items, i) }
        }));
    }

    /// The item `key`'s rect, for a surface that hangs from it (the account menu from Profile).
    pub fn item_rect_of(&self, items: &[RailItem], key: u32) -> Option<Rect> {
        let i = items.iter().position(|item| item.key == key)?;
        Some(self.item_rect(items, i))
    }

    /// `active` is the item of the screen on view; `focused` the item holding focus, if any.
    pub fn draw(&self, p: Painter, items: &[RailItem], active: Option<u32>, focused: Option<u32>) {
        let w = width(self.expand_pos());
        p.rect(Rect::new(0.0, 0.0, w, SCR_H), 0.0, theme::PANEL_TOP, theme::PANEL_BOT, 0.0);
        for item in items {
            self.draw_item(p, items, item.key, active, focused);
        }
        // The band fades out at an edge where items are cut off.
        let b = band(items);
        let clear = |c: [f32; 4]| [c[0], c[1], c[2], 0.0];
        if self.scroll.pos > 0.5 {
            p.rect(Rect::new(0.0, b.y, w, FADE_H), 0.0, theme::PANEL_TOP, clear(theme::PANEL_TOP), 0.0);
        }
        if self.scroll.pos < max_scroll(items) - 0.5 {
            p.rect(Rect::new(0.0, b.y + b.h - FADE_H, w, FADE_H), 0.0, clear(theme::PANEL_BOT), theme::PANEL_BOT, 0.0);
        }
    }

    /// One item without the panel behind it: also the lit copy an opener lift draws above a dim.
    pub fn draw_item(&self, p: Painter, items: &[RailItem], key: u32, active: Option<u32>, focused: Option<u32>) {
        let Some(i) = items.iter().position(|item| item.key == key) else { return };
        let item = &items[i];
        let e = self.expand_pos();
        let r = self.item_rect(items, i);
        let clip = item_clip(items, i).intersect(Rect::new(0.0, 0.0, width(e) - INSET, SCR_H));
        if r.y + r.h <= clip.y || r.y >= clip.y + clip.h { return; }
        p.clip(clip);
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
            let ty = plx_gfx::text::text_vcenter_y(theme::size::LABEL, 1, r.cy());
            let spacing = if item.caps { LABEL_TRACKING } else { 0.0 };
            p.alpha(e * e).text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::LABEL, ink, 0, 1, spacing);
        }
        p.clip_clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rail(libraries: usize) -> Vec<RailItem> {
        let item = |key, mark| RailItem { key, mark, label: CString::default(), caps: true };
        let mut items = vec![item(0, RailMark::Icon(Icon::Home)), item(1, RailMark::Icon(Icon::Search)),
            item(2, RailMark::Icon(Icon::Bookmark))];
        items.extend((0..libraries).map(|i| item(10 + i as u32, RailMark::Badge(CString::default()))));
        items.push(item(99, RailMark::Avatar(CString::default())));
        items
    }

    fn settle(render: &mut RailRender, items: &[RailItem], focused: Option<u32>) {
        for _ in 0..240 { render.update(items, focused, None, 1.0 / 60.0); }
    }

    #[test]
    fn a_long_rail_keeps_every_item_a_full_pitch_apart() {
        let items = rail(50);
        let render = RailRender::new();
        for i in 1..items.len() {
            let gap = item_y(&items, i, render.scroll.pos) - item_y(&items, i - 1, render.scroll.pos);
            assert!(i == items.len() - 1 || gap >= PITCH - 0.01, "item {i} overlaps the one above (gap {gap})");
        }
    }

    #[test]
    fn the_band_scrolls_to_keep_the_focused_library_inside_it() {
        let items = rail(50);
        let mut render = RailRender::new();
        let b = band(&items);
        for key in [10, 40, 59, 25, 10] {
            settle(&mut render, &items, Some(key));
            let i = items.iter().position(|item| item.key == key).unwrap();
            let r = render.item_rect(&items, i);
            assert!(r.y >= b.y - 0.5 && r.y + r.h <= b.y + b.h + 0.5, "library {key} drawn outside the band: {r:?} vs {b:?}");
        }
    }

    #[test]
    fn the_fixed_items_never_scroll() {
        let items = rail(50);
        let mut render = RailRender::new();
        settle(&mut render, &items, Some(59));
        assert!(render.scroll.pos > 0.0, "the band scrolled");
        assert_eq!(render.item_rect(&items, 0).y, MARGIN_Y, "Home stays at the top");
        assert_eq!(render.item_rect(&items, items.len() - 1).y, SCR_H - MARGIN_Y - ITEM_H, "Profile stays at the foot");
    }

    #[test]
    fn a_short_rail_does_not_scroll() {
        let items = rail(3);
        let mut render = RailRender::new();
        settle(&mut render, &items, Some(12));
        assert_eq!(render.scroll.pos, 0.0);
    }
}
