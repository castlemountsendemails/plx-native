//! Paint from the instance and its retained frame views. No live Search/roster/focus reads.
use super::*;
use crate::registry::tile_facts;
use plx_ui::consts::{MARGIN_X, SCR_H};
use plx_ui::label::{HAlign, Label, VAlign};
use plx_ui::widgets::{Art, Button, StatusKind, StatusOverlay};
use plx_ui::{theme, Env, Painter, View};
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

const CARET_W: f32 = 5.0;
const CARET_GAP: f32 = 8.0;
const GHOST_GAP: f32 = theme::size::BODY as f32;
const COUNT_GAP: f32 = theme::space::SM;
const SOURCE_PAD: f32 = theme::space::XS;

#[derive(Default)]
pub(super) struct Resources {
    query: String,
    caret: usize,
    run: CString,
    head: CString,
    run_w: f32,
    head_w: f32,
    titles: [CString; 5],
    count_keys: [Option<(Kind, usize)>; 5],
    counts: [CString; 5],
    owner: CString,
}

impl Resources {
    pub(super) fn prepare<H: SearchLike>(
        &mut self,
        query: &str,
        caret: usize,
        owner: &str,
        cx: &Cx<'_, H>,
    ) {
        let q = query.split('\0').next().unwrap_or("");
        let mut caret = caret.min(q.len());
        while !q.is_char_boundary(caret) {
            caret -= 1;
        }
        if self.query != q || self.caret != caret || self.run.is_empty() {
            self.query = q.into();
            self.caret = caret;
            self.run = if q.trim().is_empty() {
                plx_platform::i18n::msg::browse_search_placeholder_c().into()
            } else {
                cstring(q)
            };
            let (_, head) = run_and_head(q, caret);
            self.head = cstring(head);
            self.run_w = cx.measure.width(&self.run, theme::size::HERO, true);
            self.head_w = if q.trim().is_empty() {
                0.0
            } else {
                cx.measure.width(&self.head, theme::size::HERO, true)
            };
        }
        let keys = count_keys(H::search(cx).shelves());
        for (i, kind) in plx_data::search::KINDS.iter().enumerate() {
            if self.titles[i].is_empty() {
                self.titles[i] = cstring(kind.title());
            }
            let key = keys[i];
            if self.count_keys[i] != key {
                self.count_keys[i] = key;
                self.counts[i] = key.map_or_else(CString::default, |(kind, n)| {
                    cstring(&kind.count_label(n))
                });
            }
        }
        if self.owner.to_bytes() != owner.as_bytes() {
            self.owner = cstring(owner);
        }
    }
}

/// Each shelf's count, filed under its KIND's slot — the slot `draw` reads the heading from.
///
/// `shelves()` is compact: a kind with no results has no shelf at all. Filing counts by position in
/// that list put every count after a missing kind one slot early, so a search with no TV Shows
/// headed its Episodes shelf with the Cast & Crew count ("Episodes 2 people").
fn count_keys(shelves: &[plx_data::search::Shelf]) -> [Option<(Kind, usize)>; 5] {
    let mut keys = [None; 5];
    for shelf in shelves {
        keys[layout::ordinal(shelf.kind) as usize] = Some((shelf.kind, shelf.items.len()));
    }
    keys
}

pub(super) fn draw<H: SearchLike>(screen: &SearchScreen, f: &mut DrawFrame<'_, '_, H>) {
    plx_gfx::gfx::frame_clear(theme::CLEAR_RGB.0, theme::CLEAR_RGB.1, theme::CLEAR_RGB.2);
    let p = f.painter.alpha(f.page_alpha);
    screen.ground.draw(p, Rect::FULL);
    field(screen, f, p);
    let p = p.alpha(screen.fade.alpha());
    if !screen.real_query() {
        recents(screen, f, p);
    } else if screen.rows.is_empty() {
        empty(screen, f, p);
    }
    for (i, row) in screen.rows.iter().enumerate() {
        let Some((frame, shelf)) = screen.row_frame(f.cx, i) else { continue };
        let top = frame.y - layout::HEAD_TO_ROW;
        if !plx_ui::on_axis(
            top,
            layout::block_h(row.kind, shelf.under_band()),
            SCR_H,
            0.0,
        ) {
            continue;
        }
        let slot = layout::ordinal(row.kind) as usize;
        let title = &screen.render.titles[slot];
        let cap = f.cx.measure.cap_h(theme::size::HEADLINE);
        let source = if screen.owner_row == Some(i) && screen.owner_alpha.pos > OWNER_FLOOR {
            screen.render.owner.as_c_str()
        } else {
            c""
        };
        heading_flow(
            title,
            &screen.render.counts[slot],
            source,
            |run, dx, size, bold, ink, faded| {
                let mut label = Label::new(run.as_ptr(), size, ink).v(VAlign::Baseline);
                if bold != 0 {
                    label = label.bold();
                }
                label.draw(
                    if faded {
                        p.alpha(screen.owner_alpha.pos.clamp(0.0, 1.0))
                    } else {
                        p
                    },
                    Rect::new(MARGIN_X + dx, top - shelf.heading_lift(), 0.0, cap),
                );
                f.cx.measure.width(run, size, bold != 0)
            },
        );
        if let Some(src) = screen.row_cards(H::search(f.cx), i) {
            shelf.draw(f, p, &src, frame);
        }
    }
    // A standing note, never a target: the page paints through here, not `StackView::paint`, so it
    // ends the draw itself, over everything the page painted.
    screen.stack.draw_hold_hint(f.painter.alpha(f.page_alpha), f.measure);
}

fn field<H: SearchLike>(screen: &SearchScreen, f: &mut DrawFrame<'_, '_, H>, p: Painter) {
    let data = &screen.render;
    let rect = Rect::new(
        layout::FIELD.x,
        layout::FIELD.y - screen.stack.scroll(),
        layout::FIELD.w,
        layout::FIELD.h,
    );
    let blank = data.query.trim().is_empty();
    let hot = screen.hot.pos;
    let ink = if blank {
        theme::cross(theme::TEXT_TERTIARY, theme::TEXT_SECONDARY, hot)
    } else {
        theme::cross(theme::TEXT_SECONDARY, ink_target(screen.editing), hot)
    };
    let (run_dx, caret_dx) = run_layout(
        data.run_w,
        run_caret_w(blank, data.head_w),
        rect.w,
        screen.editing,
    );
    let (cap_top, cap_base) = plx_gfx::text::text_cap_band(theme::size::HERO, 1);
    let pad = descent_pad(
        rect.h,
        plx_gfx::text::text_height(theme::size::HERO, 1),
        cap_top,
        cap_base,
    );
    {
        let _clip = f.clip(p, Rect::new(rect.x, rect.y, rect.w, rect.h + pad));
        Label::new(data.run.as_ptr(), theme::size::HERO, ink)
            .bold()
            .draw(p, Rect::new(rect.x + run_dx, rect.y, rect.w, rect.h));
        let text_y = plx_gfx::text::text_vcenter_y(theme::size::HERO, 1, rect.cy());
        if caret_shown(screen.editing, screen.blink_us < super::BLINK_US) {
            p.rect(
                Rect::new(
                    rect.x + caret_dx,
                    text_y + cap_top,
                    CARET_W,
                    cap_base - cap_top,
                ),
                0.0,
                theme::TEXT_PRIMARY,
                theme::TEXT_PRIMARY,
                0.0,
            );
        }
        if ghost_shown(&data.query) {
            let y = plx_gfx::text::baseline_y(theme::size::BODY, 0, theme::size::HERO, 1, text_y);
            p.text(
                plx_platform::i18n::msg::browse_search_one_more_c().as_ptr(),
                rect.x + caret_dx + CARET_W + GHOST_GAP,
                y,
                theme::size::BODY,
                theme::cross(theme::TEXT_TERTIARY, theme::TEXT_SECONDARY, hot),
                0,
                0,
            );
        }
    }
    stop(screen, FIELD, ElemKind::Bare, f, p);
}

fn recents<H: SearchLike>(screen: &SearchScreen, f: &mut DrawFrame<'_, '_, H>, p: Painter) {
    let env = Env::inert();
    let y = layout::CONTENT_TOP - screen.stack.scroll();
    let elems = screen.recents.iter().copied().chain([CLEAR]);
    for ((rect, label), elem) in screen.chips.iter().zip(elems) {
        let icon = if elem == CLEAR { plx_ui::icons::Icon::Close } else { plx_ui::icons::Icon::Clock };
        Button::new(label.as_ptr(), layout::CHIP_SIZE, Rect { y, ..*rect })
            .icon(icon)
            .focused(f.cx.focus.current == Some(screen.key(elem)))
            .draw(&env, p);
        stop(screen, elem, ElemKind::Bare, f, p);
    }
    Label::new(plx_platform::i18n::msg::browse_search_hint_c().as_ptr(), layout::HINT_SIZE, theme::TEXT_TERTIARY)
        .draw(p, Rect::new(MARGIN_X, layout::hint_y(!screen.chips.is_empty()) - screen.stack.scroll(), 0.0,
            layout::HINT_SIZE as f32 * 1.35));
}

fn empty<H: SearchLike>(screen: &SearchScreen, f: &DrawFrame<'_, '_, H>, p: Painter) {
    let state = H::search(f.cx).state();
    if screen.draft.pending() {
        return;
    }
    let Some(empty) = empty_state(state, false) else {
        return;
    };
    let mut rect = layout::empty_band(screen.editing);
    rect.y -= screen.stack.scroll();
    if empty == EmptyState::Fault {
        StatusOverlay::new(rect, plx_platform::i18n::msg::browse_search_failed_c(), StatusKind::Failed)
            .reason(plx_platform::i18n::msg::browse_search_failed_detail_c())
            .draw(&Env::inert(), p);
        return;
    }
    let header = plx_platform::i18n::msg::browse_search_results_c();
    let shell =
        f.cx.measure
            .width_str(&plx_platform::i18n::msg::browse_search_no_results(""), theme::size::TITLE, true);
    let statement = cstring(&no_results_line(&elide(
        screen.draft.query().trim(),
        1200.0 - shell,
        theme::size::TITLE,
        true,
        f.cx.measure,
    )));
    let hh = f.cx.measure.cap_h(theme::size::CAPTION);
    let sh = f.cx.measure.cap_h(theme::size::TITLE);
    let top = rect.y + (rect.h - hh - sh - theme::space::MD) * 0.5;
    Label::new(header.as_ptr(), theme::size::CAPTION, theme::TEXT_TERTIARY)
        .h(HAlign::Center)
        .v(VAlign::CapTop)
        .draw(p, Rect::new(rect.x, top, rect.w, 0.0));
    Label::new(statement.as_ptr(), theme::size::TITLE, theme::TEXT_HEADING)
        .bold()
        .h(HAlign::Center)
        .v(VAlign::CapTop)
        .draw(
            p,
            Rect::new(rect.x, top + hh + theme::space::MD, rect.w, 0.0),
        );
}

/// What a result tile draws. A collection always draws through `Art::Poster` with its row, so the
/// shared card composite decides between its poster (custom, or #274's fan for the server's
/// composite) and the shared neutral collection tile (`ui::collection_tile`, for a row with no
/// thumb) — this screen draws no collection face of its own. A tag-shaped collection hit is
/// already a thumb-less collection row by the time it lands (`search::CollectionHit::from_tag`).
pub(super) fn tile_art(kind: Kind, item: &Item) -> Art<'_> {
    match (kind, item) {
        (Kind::Episode, Item::Media(media)) => Art::Still(Some(tile_facts::of(media))),
        (_, Item::Media(media)) => Art::Poster(Some(tile_facts::of(media))),
        (_, Item::Collection(hit)) => Art::Poster(Some(tile_facts::of(&hit.item))),
        (Kind::Person, Item::Tag(tag)) => Art::Person {
            sid: tag.sid.raw(),
            key: &tag.thumb,
            res: (300, 300),
        },
        (_, Item::Tag(tag)) if tag.thumb.is_empty() => Art::Poster(None),
        (_, Item::Tag(tag)) => Art::Thumb {
            sid: tag.sid.raw(),
            key: &tag.thumb,
            res: (250, 375),
        },
    }
}

fn stop<H: SearchLike>(
    screen: &SearchScreen,
    elem: u32,
    kind: ElemKind,
    f: &mut DrawFrame<'_, '_, H>,
    p: Painter,
) {
    if !f.records_stops() {
        return;
    }
    use plx_ui::screen::{Activate, Hover, Stop};
    let Some(placed) = <SearchScreen as plx_ui::screen::Focusable<H>>::place(screen, &elem, f.cx, plx_ui::screen::At::Drawn) else {
        return;
    };
    f.stop(
        p,
        Stop {
            key: screen.key(elem),
            rect: placed.rect,
            rest_rect: placed.rest_rect,
            clip: placed.clip,
            hover: Hover::Focus,
            activate: if kind == ElemKind::Bare {
                Activate::Immediate
            } else {
                Activate::Press
            },
        },
    );
}

fn cstring(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}
fn elide(
    text: &str,
    width: f32,
    size: i32,
    bold: bool,
    measure: &dyn plx_machine::machine::Measure,
) -> String {
    plx_gfx::text::elide_by(text, width, false, |s| {
        measure.width(&cstring(s), size, bold)
    })
}
fn run_layout(run: f32, caret: f32, width: f32, editing: bool) -> (f32, f32) {
    let available = (width - if editing { CARET_W + CARET_GAP } else { 0.0 }).max(0.0);
    let overflow = (caret - available).max(0.0).min((run - available).max(0.0));
    (
        -overflow,
        (caret - overflow + CARET_GAP).min((width - CARET_W).max(0.0)),
    )
}

fn run_and_head(q: &str, caret: usize) -> (&str, &str) {
    let run = &q[..q.find('\0').unwrap_or(q.len())];
    let mut caret = caret.min(run.len());
    while caret > 0 && !run.is_char_boundary(caret) {
        caret -= 1;
    }
    (run, &run[..caret])
}

fn run_caret_w(blank: bool, head_w: f32) -> f32 {
    if blank {
        0.0
    } else {
        head_w
    }
}

fn ghost_shown(q: &str) -> bool {
    let n = q.trim().chars().count();
    n > 0 && n + 1 == plx_data::search::MIN_QUERY
}

fn caret_shown(editing: bool, phase_on: bool) -> bool {
    editing && phase_on
}

fn descent_pad(box_h: f32, full_h: f32, cap_top: f32, cap_base: f32) -> f32 {
    (full_h - box_h * 0.5 - (cap_top + cap_base) * 0.5).max(0.0)
}

fn ink_target(editing: bool) -> [f32; 4] {
    if editing {
        theme::FIELD_EDITING_INK
    } else {
        theme::FIELD_WAITING_INK
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmptyState {
    NoResults,
    Fault,
}

fn empty_state(state: plx_data::search::State, has_shelves: bool) -> Option<EmptyState> {
    use plx_data::search::State;
    match state {
        State::Idle | State::Searching => None,
        State::Ready => (!has_shelves).then_some(EmptyState::NoResults),
        State::Failed => Some(EmptyState::Fault),
    }
}

fn no_results_line(q: &str) -> String {
    plx_platform::i18n::msg::browse_search_no_results(q)
}

fn heading_flow(
    title: &CStr,
    count: &CStr,
    source: &CStr,
    mut run: impl FnMut(&CStr, f32, c_int, c_int, [f32; 4], bool) -> f32,
) -> f32 {
    let mut dx = run(
        title,
        0.0,
        theme::size::HEADLINE,
        1,
        theme::TEXT_HEADING,
        false,
    );
    if !count.is_empty() {
        dx += COUNT_GAP;
        dx += run(count, dx, theme::size::BODY, 0, theme::TEXT_TERTIARY, false);
    }
    if source.is_empty() {
        return dx;
    }
    dx += SOURCE_PAD;
    dx += run(c"·", dx, theme::size::BODY, 0, theme::TEXT_SEPARATOR, true);
    dx += SOURCE_PAD;
    dx += run(source, dx, theme::size::BODY, 0, theme::TEXT_TERTIARY, true);
    dx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translated_count_messages_follow_belarusian_grammar_with_separate_number_formatting() {
        use plx_platform::i18n::{LocaleContext, Preference, msg};
        let be = LocaleContext::resolve(Preference::Be, None, Some("en-US"), None, None);
        for (count, expected) in [(0, "0 вынікаў"), (1, "1 вынік"), (2, "2 вынікі"),
            (11, "11 вынікаў"), (21, "21 вынік"), (22, "22 вынікі"), (12345, "12,345 вынікаў")] {
            assert_eq!(msg::browse_search_count_in(&be, count), expected);
        }
        assert_eq!(msg::browse_search_people_in(&be, 2), "2 асобы");
        assert_eq!(msg::browse_search_items_in(&be, 11), "11 элементаў");
        let es = LocaleContext::resolve(Preference::Es, None, None, None, None);
        assert_eq!(msg::browse_search_count_in(&es, 1), "1 resultado");
        assert_eq!(msg::browse_search_count_in(&es, 2), "2 resultados");
    }

    #[test]
    fn translated_search_templates_preserve_user_values_and_sentence_order() {
        use plx_platform::i18n::{LocaleContext, Preference, msg};
        let be = LocaleContext::resolve(Preference::Be, None, None, None, None);
        assert_eq!(msg::browse_search_no_results_in(&be, "Кіна {query}"), "Няма вынікаў для «Кіна {query}»");
        assert_eq!(msg::browse_search_placeholder_c_in(&be).to_str().unwrap(), "Пошук");
    }

    #[test]
    fn the_empty_page_reads_like_stock_plex() {
        use plx_platform::i18n::{LocaleContext, Preference, msg};
        let en = LocaleContext::resolve(Preference::En, None, None, None, None);
        assert_eq!(msg::browse_search_placeholder_in(&en), "Search");
        assert_eq!(msg::browse_search_clear_in(&en), "Clear Search History");
        assert_eq!(empty_state(State::Idle, false), None, "no query draws no statement, only the hint");
        assert_eq!(empty_state(State::Ready, false), Some(EmptyState::NoResults));
    }

    use plx_data::search::{Item, Kind, State};

    fn cs(s: &str) -> CString {
        CString::new(s).expect("test literal")
    }

    #[test]
    fn a_count_sits_under_its_own_kind_when_an_earlier_kind_found_nothing() {
        let shelf = |kind, n| plx_data::search::Shelf { epoch: 0, window: Default::default(),
            kind,
            items: (0..n).map(|_| Item::Media(Default::default())).collect(),
        };
        // No TV Shows: the compact shelf list skips that kind entirely.
        let keys = count_keys(&[
            shelf(Kind::Movie, 5),
            shelf(Kind::Episode, 4),
            shelf(Kind::Person, 2),
        ]);
        assert_eq!(keys[layout::ordinal(Kind::Movie) as usize], Some((Kind::Movie, 5)));
        assert_eq!(keys[layout::ordinal(Kind::Show) as usize], None);
        assert_eq!(keys[layout::ordinal(Kind::Episode) as usize], Some((Kind::Episode, 4)));
        assert_eq!(keys[layout::ordinal(Kind::Person) as usize], Some((Kind::Person, 2)));
        assert_eq!(keys[layout::ordinal(Kind::Collection) as usize], None);
    }

    #[test]
    fn the_one_more_ghost_shows_only_one_character_short() {
        assert!(ghost_shown("a"));
        assert!(!ghost_shown(""));
        assert!(!ghost_shown("ab"));
    }

    #[test]
    fn ink_target_picks_pure_white_off_editing_and_the_editing_stop_on() {
        assert_eq!(ink_target(false), theme::FIELD_WAITING_INK);
        assert_eq!(ink_target(true), theme::FIELD_EDITING_INK);
        assert_ne!(theme::FIELD_WAITING_INK, theme::FIELD_EDITING_INK);
    }

    #[test]
    fn focus_is_carried_by_a_wide_ink_step_and_nothing_else() {
        let idle = theme::cross(theme::TEXT_SECONDARY, ink_target(false), 0.0);
        assert_eq!(idle, theme::TEXT_SECONDARY);
        let waiting = theme::cross(theme::TEXT_SECONDARY, ink_target(false), 1.0);
        let editing = theme::cross(theme::TEXT_SECONDARY, ink_target(true), 1.0);
        assert_eq!(waiting, theme::FIELD_WAITING_INK);
        assert_eq!(editing, theme::FIELD_EDITING_INK);
        let luma = |c: [f32; 4]| (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) * c[3];
        assert!(luma(waiting) - luma(idle) > 0.15);
        assert!(luma(editing) - luma(idle) > 0.15);
        assert!(luma(waiting) > luma(editing));
    }

    #[test]
    fn the_caret_is_visible_only_during_the_on_phase_of_an_editing_field() {
        assert!(caret_shown(true, true));
        assert!(!caret_shown(true, false));
        assert!(!caret_shown(false, true));
        assert!(!caret_shown(false, false));
    }

    #[test]
    fn a_run_that_fits_starts_at_the_edge_and_the_caret_trails_it() {
        let (run, caret) = run_layout(100.0, 100.0, 756.0, true);
        assert_eq!(run, 0.0);
        assert_eq!(caret, 100.0 + CARET_GAP);
    }

    #[test]
    fn an_overlong_run_slides_left_so_the_caret_lands_on_the_boxs_right_edge() {
        let box_w = 756.0;
        let (run, caret) = run_layout(900.0, 900.0, box_w, true);
        assert!(run < 0.0);
        assert_eq!(run + 900.0, box_w - CARET_GAP - CARET_W);
        assert_eq!(caret + CARET_W, box_w);
    }

    #[test]
    fn with_no_caret_the_run_uses_the_whole_box() {
        let box_w = 756.0;
        let (run, _) = run_layout(900.0, 900.0, box_w, false);
        assert_eq!(run + 900.0, box_w);
    }

    #[test]
    fn an_empty_run_keeps_the_designs_caret_gap() {
        let (run, caret) = run_layout(0.0, 0.0, 756.0, true);
        assert_eq!(run, 0.0);
        assert_eq!(caret, CARET_GAP);
    }

    #[test]
    fn the_caret_follows_the_insertion_point_whether_or_not_the_run_has_slid() {
        for run_w in [0.0, 1.0, 400.0, 753.0, 900.0, 5000.0] {
            let (run, caret) = run_layout(run_w, run_w, 756.0, true);
            assert_eq!(caret, run + run_w + CARET_GAP, "run_w={run_w}");
        }
    }

    #[test]
    fn the_run_scrolls_to_the_caret_rather_than_to_the_tail() {
        let box_w = 756.0;
        let avail = box_w - CARET_GAP - CARET_W;
        assert_eq!(run_layout(2000.0, 0.0, box_w, true), (0.0, CARET_GAP));
        assert_eq!(
            run_layout(2000.0, avail - 10.0, box_w, true),
            (0.0, avail - 10.0 + CARET_GAP)
        );
        let (run, caret) = run_layout(2000.0, 1000.0, box_w, true);
        assert_eq!(run, -(1000.0 - avail));
        assert_eq!(caret + CARET_W, box_w);
        for c in [0.0, 1.0, 500.0, 1999.0, 2000.0] {
            let (_, caret) = run_layout(2000.0, c, box_w, true);
            assert!(
                (0.0..=box_w - CARET_W).contains(&caret),
                "caret at {c} drew at {caret}"
            );
        }
    }

    #[test]
    fn the_head_is_the_text_before_the_insertion_point() {
        assert_eq!(run_and_head("wallace", 0), ("wallace", ""));
        assert_eq!(run_and_head("wallace", 3), ("wallace", "wal"));
        assert_eq!(run_and_head("wallace", 7), ("wallace", "wallace"));
        assert_eq!(run_and_head("wallace ", 8), ("wallace ", "wallace "));
        assert_eq!(run_and_head("", 0), ("", ""));
    }

    #[test]
    fn a_caret_inside_a_multi_byte_character_never_splits_it() {
        for q in ["суббота", "千と千尋", "Amélie", "🎬🎬", "aé千🎬z"] {
            for c in 0..=q.len() + 4 {
                let (run, head) = run_and_head(q, c);
                assert_eq!(run, q);
                assert!(q.starts_with(head), "the head must be a prefix: {head:?}");
                assert!(head.len() <= c.min(q.len()));
                assert!(c.min(q.len()) - head.len() < 4);
            }
        }
    }

    #[test]
    fn a_nul_cuts_the_run_and_the_caret_lands_inside_what_is_left() {
        assert_eq!(run_and_head("wal\0lace", 3), ("wal", "wal"));
        assert_eq!(run_and_head("wal\0lace", 8), ("wal", "wal"));
        assert_eq!(run_and_head("\0wallace", 4), ("", ""));
        assert_eq!(run_and_head("су\0ббота", 99), ("су", "су"));
    }

    #[test]
    fn a_box_too_narrow_for_the_caret_does_not_go_negative() {
        assert_eq!(run_layout(0.0, 0.0, 2.0, true), (0.0, 0.0));
    }

    #[test]
    fn a_blank_field_puts_the_caret_at_the_field_start_not_after_the_placeholder() {
        assert_eq!(run_caret_w(true, 999.0), 0.0);
        assert_eq!(run_caret_w(false, 42.0), 42.0);
        let (run, caret) = run_layout(480.0, run_caret_w(true, 0.0), 756.0, true);
        assert_eq!(run, 0.0);
        assert_eq!(caret, CARET_GAP);
        let (buggy_run, buggy_caret) = run_layout(480.0, 480.0, 756.0, true);
        assert_eq!(buggy_run, 0.0);
        assert_eq!(buggy_caret, 480.0 + CARET_GAP);
        assert!(caret < buggy_caret);
    }

    #[test]
    fn the_clip_reserves_only_the_descent_centering_does_not_already_clear() {
        assert_eq!(descent_pad(80.0, 87.0, 17.0, 70.0), 3.5);
        assert_eq!(descent_pad(80.0, 40.0, 17.0, 20.0), 0.0);
        assert_eq!(descent_pad(80.0, 10.0, 17.0, 70.0), 0.0);
    }

    #[test]
    fn the_empty_state_preserves_state_and_region_copy() {
        assert_eq!(
            empty_state(State::Ready, false),
            Some(EmptyState::NoResults)
        );
        assert_eq!(empty_state(State::Failed, false), Some(EmptyState::Fault));
        assert_eq!(empty_state(State::Idle, false), None);
        assert_eq!(empty_state(State::Searching, false), None);
        assert_eq!(empty_state(State::Ready, true), None);
        assert_eq!(empty_state(State::Failed, true), Some(EmptyState::Fault));
        assert_eq!(empty_state(State::Idle, true), None);
    }

    #[test]
    fn the_statement_centres_in_the_space_the_user_can_actually_see() {
        let up = layout::empty_band(true);
        let down = layout::empty_band(false);
        assert_eq!(up.y, layout::CONTENT_TOP);
        assert_eq!(down.y, layout::CONTENT_TOP);
        assert_eq!(down.h - up.h, layout::KEYBOARD_H);
        assert!(up.y + up.h <= SCR_H - layout::KEYBOARD_H);
        assert_eq!(down.y + down.h, SCR_H);
        assert_eq!((up.x, up.w), (0.0, SCR_W));
    }

    #[test]
    fn the_query_is_quoted_back_typographically_and_the_closing_quote_survives() {
        assert_eq!(
            no_results_line("wallace"),
            "No results for \u{201C}wallace\u{201D}"
        );
        assert_eq!(
            no_results_line("wal…"),
            "No results for \u{201C}wal…\u{201D}"
        );
        assert!(!no_results_line("q").contains('"'));
    }

    #[test]
    fn the_heading_states_its_count_and_annotates_only_a_borrowed_source() {
        let w = |s: &str, sz: c_int| s.chars().count() as f32 * sz as f32 * 0.5;
        type Run = (String, f32, c_int, c_int, [f32; 4], bool);
        let runs = |title: &str, count: &str, source: &str| {
            let (t, c, s) = (cs(title), cs(count), cs(source));
            let mut out: Vec<Run> = Vec::new();
            heading_flow(&t, &c, &s, |run, dx, size, bold, ink, faded| {
                let text = run.to_str().unwrap_or_default().to_string();
                out.push((text.clone(), dx, size, bold, ink, faded));
                w(&text, size)
            });
            out
        };
        let bare = runs("Movies", "12 results", "");
        assert_eq!(bare.len(), 2);
        assert_eq!((bare[0].2, bare[0].3), (theme::size::HEADLINE, 1));
        assert_eq!(bare[0].4, theme::TEXT_HEADING);
        assert_eq!((bare[1].2, bare[1].3), (theme::size::BODY, 0));
        assert_eq!(bare[1].4, theme::TEXT_TERTIARY);
        assert_eq!(bare[1].1, w("Movies", theme::size::HEADLINE) + COUNT_GAP);
        let shared = runs("Movies", "12 results", "friend");
        assert_eq!(shared.len(), 4);
        assert_eq!(shared[..2], bare[..2]);
        assert_eq!(shared[2].0, "·");
        assert_eq!(shared[2].4, theme::TEXT_SEPARATOR);
        assert_eq!(
            shared[2].1,
            bare[1].1 + w("12 results", theme::size::BODY) + SOURCE_PAD
        );
        assert_eq!(shared[3].0, "friend");
        assert_eq!(shared[3].4, theme::TEXT_TERTIARY);
        assert_eq!((shared[3].2, shared[3].3), (theme::size::BODY, 0));
        assert!(shared[2].5 && shared[3].5);
        assert!(!shared[0].5 && !shared[1].5);
        assert_eq!(runs("Collections", "", "").len(), 1);
    }

    /// A tile that is not a poster keeps this block: an episode still is worded by the one landscape
    /// builder (`registry::captions::still_label`: its title over its air date, its address when it
    /// has no title), a headshot has its name alone, each with the server it came from named last.
    /// (A poster's caption is the Under posters setting's: `a_poster_result_is_worded_by_the_under_posters_builder`.)
    #[test]
    fn a_still_or_headshot_caption_names_a_borrowed_source_last() {
        use crate::registry::captions::still_label;
        let lines = |l: &plx_ui::cards::TileLabel| {
            let text = |c: &Option<std::ffi::CString>| c.as_ref().map(|c| c.to_string_lossy().into_owned());
            [text(&l.title), text(&l.caption)]
        };
        let ep = plx_data::pms::PmsMovie {
            kind: 3,
            title: "A Grand Day Out".into(),
            show_title: "Wallace & Gromit".into(),
            season_index: 1,
            ep_index: 3,
            aired: "1989-11-04".into(),
            ..Default::default()
        };
        assert_eq!(lines(&still_label(&ep, false, "")), [Some("A Grand Day Out".into()), Some("11/4/1989".into())]);
        assert_eq!(
            lines(&still_label(&ep, false, "friend")),
            [Some("A Grand Day Out".into()), Some("11/4/1989 \u{b7} friend".into())]
        );
        let undated = plx_data::pms::PmsMovie {
            kind: 3,
            season_index: 1,
            ep_index: 3,
            ..Default::default()
        };
        assert_eq!(lines(&still_label(&undated, false, "")), [Some("S1 \u{b7} E3".into()), None],
            "an untitled, undated episode says its address alone");
        assert_eq!(lines(&still_label(&undated, false, "friend")), [Some("S1 \u{b7} E3".into()), Some("friend".into())]);
        assert!(still_label(&ep, false, "").shadowed, "a still is shadowed like every other landscape block");
    }

    /// Every collection result reaches the shared card as `Art::Poster` with a kind-4 row, which
    /// is what makes the card draw the shared neutral tile (`ui::collection_tile`) when the row has
    /// no thumb and the poster (or the fan) when it has one. A tag-shaped hit is one of these by
    /// the time it lands (`search::CollectionHit::from_tag`).
    #[test]
    fn a_thumbless_collection_hit_reaches_the_card_as_a_neutral_collection_row() {
        let full = Item::Collection(plx_data::search::CollectionHit {
            item: plx_data::pms::PmsMovie { title: "Shorts".into(), kind: plx_data::pms::KIND_COLLECTION,
                ..Default::default() },
            tag: 7,
        });
        assert!(matches!(tile_art(Kind::Collection, &full),
            Art::Poster(Some(m)) if m.kind == plx_ui::tile::TileKind::Collection && m.thumb.is_empty()
                && m.title == "Shorts"));
        let with_art = Item::Collection(plx_data::search::CollectionHit {
            item: plx_data::pms::PmsMovie { thumb: "/library/metadata/50007/thumb/1".into(),
                kind: plx_data::pms::KIND_COLLECTION, ..Default::default() },
            tag: 7,
        });
        assert!(matches!(tile_art(Kind::Collection, &with_art),
            Art::Poster(Some(m)) if !m.thumb.is_empty()));
    }
}
