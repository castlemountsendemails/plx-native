//! The nav rail: the column of destinations down the left edge of every main screen (Home,
//! Search, Watchlist, the favorite libraries, More and its list of every library, then Profile at
//! the foot). Collapsed, it shows marks only, inside the left margin; while focus is in it, it
//! widens to show labels and the page slides right by the difference (`Rig::content_dx`) rather
//! than being covered. The application owns one [`RailRender`], publishes the items and draws it
//! in the chrome pass.
//!
//! The top items stay fixed and the profile sits at the foot; everything between (the favorites,
//! More and More's list) is a band that scrolls to keep the focused item, or else the active one,
//! in view, fading out where its items are cut off.

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
const HEADER_PITCH: f32 = 48.0;
const MARK_D: f32 = 32.0;
const STAR_BOX: f32 = 48.0;
const STAR_D: f32 = 28.0;
const INSET: f32 = 8.0;
const LABEL_X: f32 = COLLAPSED_W + theme::space::XS;
const LABEL_TRACKING: f32 = 1.5;
const K_EXPAND: f32 = 240.0;
const K_SCROLL: f32 = 240.0;
/// Air between the fixed items and the scrolling band, and the height of the band's edge fades.
const BAND_GAP: f32 = theme::space::SM;
const FADE_H: f32 = 40.0;
/// How far short of the rail's own width the background panel stops, so a focused card at the
/// content's left edge (its `CARD_FOCUS_SCALE` pop reaches about 9px past the card's base left
/// edge) and its caption are not painted over. Only live while collapsed (`e` near 0): once the
/// rail itself holds focus and expands, nothing in the content is popped.
const COLLAPSED_BLEED_CLEARANCE: f32 = 32.0;
/// §7's dim weight for an unreachable/access-refused item — the Sources list's own
/// `Section::dim` alpha (`ui::table::GROUP_DIM_A`), reused here for the same visual meaning.
const DIM_A: f32 = 0.52;

pub enum RailMark {
    Icon(Icon),
    /// A library: the first letter of its name in a square.
    Badge(CString),
    /// The profile: its initial in an accent-coloured circle.
    Avatar(CString),
    /// A server header: a label only.
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailRole {
    /// Fixed at the top: Home, Search, Watchlist.
    Top,
    /// In the scrolling band: a favorite, or More.
    Band,
    /// A server's name above its libraries in More's list: drawn, never focused.
    Header,
    /// A library in More's list, with its ★ target `star`, lit when it is a favorite.
    Row { star: u32, starred: bool },
    /// The profile, fixed at the foot.
    Foot,
}

pub struct RailItem {
    pub key: u32,
    pub mark: RailMark,
    pub label: CString,
    /// Tracked as a capital label (the destinations); the profile's name is drawn as written.
    pub caps: bool,
    pub role: RailRole,
    /// §7: a favorite whose server is unreachable or has refused this account's access, or a
    /// server's header in More when the whole group is. Drawn at reduced alpha; still focusable —
    /// the item is dimmed, not disabled.
    pub dim: bool,
}

/// The rail's width at expansion `e` (0 collapsed, 1 expanded).
pub fn width(e: f32) -> f32 {
    COLLAPSED_W + PUSH * e.clamp(0.0, 1.0)
}

/// The background panel's width at expansion `e`: [`width`], short of the clearance while
/// collapsed, flush with it (and so with the content's own left edge) once expanded.
fn panel_width(e: f32) -> f32 {
    let e = e.clamp(0.0, 1.0);
    width(e) - COLLAPSED_BLEED_CLEARANCE * (1.0 - e)
}

fn in_band(item: &RailItem) -> bool {
    !matches!(item.role, RailRole::Top | RailRole::Foot)
}

fn pitch(item: &RailItem) -> f32 {
    if item.role == RailRole::Header { HEADER_PITCH } else { PITCH }
}

fn height(item: &RailItem) -> f32 {
    if item.role == RailRole::Header { HEADER_PITCH } else { ITEM_H }
}

fn top_count(items: &[RailItem]) -> usize {
    items.iter().take_while(|item| item.role == RailRole::Top).count()
}

/// The scrolling band's visible extent, in screen y.
fn band(items: &[RailItem]) -> Rect {
    let y = MARGIN_Y + top_count(items) as f32 * PITCH + BAND_GAP;
    let bottom = SCR_H - MARGIN_Y - ITEM_H - BAND_GAP;
    Rect::new(0.0, y, EXPANDED_W, (bottom - y).max(0.0))
}

/// Item `i`'s y inside the band's content, unscrolled.
fn band_offset(items: &[RailItem], i: usize) -> f32 {
    items[..i].iter().filter(|item| in_band(item)).map(pitch).sum()
}

/// How far the band can scroll: its content less what fits.
fn max_scroll(items: &[RailItem]) -> f32 {
    let content: f32 = items.iter().filter(|item| in_band(item)).map(pitch).sum();
    (content - (PITCH - ITEM_H) - band(items).h).max(0.0)
}

fn item_y(items: &[RailItem], i: usize, scroll: f32) -> f32 {
    match items[i].role {
        RailRole::Top => MARGIN_Y + i as f32 * PITCH,
        RailRole::Foot => SCR_H - MARGIN_Y - ITEM_H,
        _ => band(items).y + band_offset(items, i) - scroll,
    }
}

/// Where item `i` may be seen: the whole screen for the fixed items, the band for the rest.
fn item_clip(items: &[RailItem], i: usize) -> Rect {
    if in_band(&items[i]) { band(items) } else { Rect::FULL }
}

/// The item a focus key belongs to: the item itself, or the row whose ★ it is.
fn index_of(items: &[RailItem], key: u32) -> Option<usize> {
    items.iter().position(|item| item.key == key || matches!(item.role, RailRole::Row { star, .. } if star == key))
}

/// A row's ★ target, at its right end.
fn star_rect(r: Rect) -> Rect {
    Rect::new(r.x + r.w - STAR_BOX - INSET, r.cy() - STAR_BOX * 0.5, STAR_BOX, STAR_BOX)
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
        let follow = focused.and_then(|key| index_of(items, key))
            .or_else(|| active.and_then(|key| index_of(items, key)))
            .filter(|&i| in_band(&items[i]));
        if let Some(i) = follow {
            let y = band_offset(items, i);
            let h = band(items).h;
            if y - PITCH < self.scroll_target { self.scroll_target = y - PITCH; }
            if y + height(&items[i]) + PITCH > self.scroll_target + h { self.scroll_target = y + height(&items[i]) + PITCH - h; }
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
        Rect::new(INSET, item_y(items, i, self.scroll.pos), width(self.expand_pos()) - 2.0 * INSET, height(&items[i]))
    }

    /// The items as focus members, at the rects drawn this frame and clipped to where they show:
    /// every item but the headers, each More row followed by its ★ as a side member.
    pub fn members(&self, items: &[RailItem], out: &mut Vec<StripMember<u32>>) {
        out.clear();
        for i in 0..items.len() {
            if items[i].role == RailRole::Header { continue; }
            let r = self.item_rect(items, i);
            let clip = item_clip(items, i);
            out.push(StripMember { elem: items[i].key, drawn: r, target: r, clip, side: false });
            if let RailRole::Row { star, .. } = items[i].role {
                let s = star_rect(r);
                out.push(StripMember { elem: star, drawn: s, target: s, clip, side: true });
            }
        }
    }

    /// The item `key`'s rect, for a surface that hangs from it (the account menu from Profile).
    pub fn item_rect_of(&self, items: &[RailItem], key: u32) -> Option<Rect> {
        let i = items.iter().position(|item| item.key == key)?;
        Some(self.item_rect(items, i))
    }

    /// `active` is the item of the screen on view; `focused` the key holding focus, if any;
    /// `moving` the favorite currently held for reorder (§3.5), if any.
    pub fn draw(&self, p: Painter, items: &[RailItem], active: Option<u32>, focused: Option<u32>, moving: Option<u32>) {
        let e = self.expand_pos();
        let panel_w = panel_width(e);
        p.rect(Rect::new(0.0, 0.0, panel_w, SCR_H), 0.0, theme::PANEL_TOP, theme::PANEL_BOT, 0.0);
        for item in items {
            self.draw_item(p, items, item.key, active, focused, moving);
        }
        // The band fades out at an edge where items are cut off.
        let b = band(items);
        let clear = |c: [f32; 4]| [c[0], c[1], c[2], 0.0];
        if self.scroll.pos > 0.5 {
            p.rect(Rect::new(0.0, b.y, panel_w, FADE_H), 0.0, theme::PANEL_TOP, clear(theme::PANEL_TOP), 0.0);
        }
        if self.scroll.pos < max_scroll(items) - 0.5 {
            p.rect(Rect::new(0.0, b.y + b.h - FADE_H, panel_w, FADE_H), 0.0, clear(theme::PANEL_BOT), theme::PANEL_BOT, 0.0);
        }
    }

    /// One item without the panel behind it: also the lit copy an opener lift draws above a dim.
    /// `moving` is the favorite currently held for reorder (§3.5), if any.
    pub fn draw_item(&self, p: Painter, items: &[RailItem], key: u32, active: Option<u32>, focused: Option<u32>, moving: Option<u32>) {
        let Some(i) = items.iter().position(|item| item.key == key) else { return };
        let item = &items[i];
        let e = self.expand_pos();
        let r = self.item_rect(items, i);
        let clip = item_clip(items, i).intersect(Rect::new(0.0, 0.0, width(e) - INSET, SCR_H));
        if r.y + r.h <= clip.y || r.y >= clip.y + clip.h { return; }
        p.clip(clip);
        // §7: a dimmed item (an unreachable/access-refused favorite, or More's header for such a
        // server) draws at DIM_A throughout — mark, label and even its own focus pill, so focusing
        // it is never the one bright thing on a rail that is telling you its server is down.
        let dim_a = if item.dim { DIM_A } else { 1.0 };
        if item.role == RailRole::Header {
            if e > 0.0 {
                let ty = plx_gfx::text::text_vcenter_y(theme::size::CAPTION, 1, r.y + r.h * 0.6);
                p.alpha(e * e * dim_a).text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::CAPTION,
                    theme::TEXT_TERTIARY, 0, 1, LABEL_TRACKING);
            }
            p.clip_clear();
            return;
        }
        let is_focused = focused == Some(key);
        let is_active = active == Some(key);
        if is_focused {
            p.alpha(dim_a).rrect(r, r.h * 0.5, r.h * 0.5, theme::ACCENT);
        }
        if is_active {
            p.alpha(dim_a).rrect(Rect::new(0.0, r.y + 16.0, 4.0, r.h - 32.0), 2.0, 2.0, theme::ACCENT);
        }
        let ink = if is_focused { theme::INK_ON_ACCENT }
            else if is_active { theme::TEXT_PRIMARY }
            else { theme::CONTROL_TEXT_REST };
        self.draw_mark(p.alpha(dim_a), item, r, ink, is_focused);
        if e > 0.0 {
            let ty = plx_gfx::text::text_vcenter_y(theme::size::LABEL, 1, r.cy());
            let spacing = if item.caps { LABEL_TRACKING } else { 0.0 };
            if let RailRole::Row { star, starred } = item.role {
                let s = star_rect(r);
                p.clip(clip.intersect(Rect::new(0.0, 0.0, s.x, SCR_H)));
                p.alpha(e * e * dim_a).text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::LABEL, ink, 0, 1, spacing);
                p.clip(clip);
                self.draw_star(p.alpha(e * e * dim_a), s, starred, focused == Some(star));
            } else if moving == Some(key) {
                let s = star_rect(r);
                p.clip(clip.intersect(Rect::new(0.0, 0.0, s.x, SCR_H)));
                p.alpha(e * e * dim_a).text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::LABEL, ink, 0, 1, spacing);
                p.clip(clip);
                self.draw_move_mark(p.alpha(e * e * dim_a), s, ink);
            } else {
                p.alpha(e * e * dim_a).text_tracked(item.label.as_ptr(), LABEL_X, ty, theme::size::LABEL, ink, 0, 1, spacing);
            }
        }
        p.clip_clear();
    }

    fn draw_star(&self, p: Painter, s: Rect, starred: bool, focused: bool) {
        if focused {
            p.rrect(s, s.h * 0.5, s.h * 0.5, theme::ACCENT);
        }
        let (icon, ink) = match (starred, focused) {
            (true, false) => (Icon::StarFill, theme::ACCENT),
            (true, true) => (Icon::StarFill, theme::INK_ON_ACCENT),
            (false, true) => (Icon::Star, theme::INK_ON_ACCENT),
            (false, false) => (Icon::Star, theme::TEXT_TERTIARY),
        };
        crate::icons::draw(p, icon, Rect::new(s.cx() - STAR_D * 0.5, s.cy() - STAR_D * 0.5, STAR_D, STAR_D), ink);
    }

    /// The ⇅ mark (§3.5) at a favorite's ★ target, in the same box: a held OK has entered move
    /// mode for it. Always on the accent fill, since move mode only ever runs on the focused item.
    fn draw_move_mark(&self, p: Painter, s: Rect, ink: [f32; 4]) {
        let d = STAR_D * 0.6;
        let gap = 2.0;
        crate::icons::draw(p, Icon::ChevronUp, Rect::new(s.cx() - d * 0.5, s.cy() - d - gap * 0.5, d, d), ink);
        crate::icons::draw(p, Icon::ChevronDown, Rect::new(s.cx() - d * 0.5, s.cy() + gap * 0.5, d, d), ink);
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
            RailMark::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: u32, mark: RailMark, role: RailRole) -> RailItem {
        RailItem { key, mark, label: CString::default(), caps: true, role, dim: false }
    }

    /// Home, Search, Watchlist, `libraries` favorites, More, then `rows` More rows under one
    /// server header, then Profile.
    fn rail_with_more(libraries: usize, rows: usize) -> Vec<RailItem> {
        let mut items = vec![item(0, RailMark::Icon(Icon::Home), RailRole::Top),
            item(1, RailMark::Icon(Icon::Search), RailRole::Top),
            item(2, RailMark::Icon(Icon::Bookmark), RailRole::Top)];
        items.extend((0..libraries).map(|i| item(10 + i as u32, RailMark::Badge(CString::default()), RailRole::Band)));
        items.push(item(5, RailMark::Icon(Icon::More), RailRole::Band));
        if rows > 0 {
            items.push(item(6, RailMark::None, RailRole::Header));
            items.extend((0..rows).map(|i| item(200 + i as u32, RailMark::Badge(CString::default()),
                RailRole::Row { star: 400 + i as u32, starred: i % 2 == 0 })));
        }
        items.push(item(99, RailMark::Avatar(CString::default()), RailRole::Foot));
        items
    }

    fn rail(libraries: usize) -> Vec<RailItem> {
        rail_with_more(libraries, 0)
    }

    fn settle(render: &mut RailRender, items: &[RailItem], focused: Option<u32>) {
        for _ in 0..240 { render.update(items, focused, None, 1.0 / 60.0); }
    }

    fn inside_band(render: &RailRender, items: &[RailItem], key: u32) -> bool {
        let b = band(items);
        let r = render.item_rect(items, index_of(items, key).unwrap());
        r.y >= b.y - 0.5 && r.y + r.h <= b.y + b.h + 0.5
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
        for key in [10, 40, 59, 25, 10] {
            settle(&mut render, &items, Some(key));
            assert!(inside_band(&render, &items, key), "library {key} drawn outside the band");
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

    #[test]
    fn more_scrolls_with_the_band_even_without_favorites() {
        let items = rail_with_more(0, 60);
        let mut render = RailRender::new();
        settle(&mut render, &items, Some(259));
        assert!(inside_band(&render, &items, 259));
        assert!(render.item_rect(&items, 3).y < band(&items).y, "More scrolled off the band's top");
    }

    #[test]
    fn headers_are_not_focusable_and_each_row_carries_its_star_beside_it() {
        let items = rail_with_more(2, 3);
        let mut members = Vec::new();
        RailRender::new().members(&items, &mut members);
        assert!(!members.iter().any(|m| m.elem == 6), "the server header is not a focus member");
        let row = members.iter().position(|m| m.elem == 200).unwrap();
        assert_eq!((members[row + 1].elem, members[row + 1].side), (400, true));
        assert_eq!(members.iter().filter(|m| m.side).count(), 3);
    }

    #[test]
    fn the_collapsed_panel_clears_a_focused_card_but_the_expanded_one_is_flush_with_content() {
        assert_eq!(panel_width(0.0), COLLAPSED_W - COLLAPSED_BLEED_CLEARANCE);
        assert_eq!(panel_width(1.0), EXPANDED_W, "no content can be focused once the rail expands");
        assert!(panel_width(0.5) > panel_width(0.0) && panel_width(0.5) < panel_width(1.0));
    }

    #[test]
    fn a_focused_star_keeps_its_row_in_view() {
        let items = rail_with_more(50, 40);
        let mut render = RailRender::new();
        settle(&mut render, &items, Some(439));
        assert!(inside_band(&render, &items, 239), "the starred row is on screen");
    }
}
