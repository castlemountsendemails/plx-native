//! Search's document geometry. Both engine placement and rendering use these expressions.
use plx_data::search::Kind;
use plx_ui::cards::RowStyle;
use plx_ui::consts::{CARD_H, CARD_W, MARGIN_X, SCR_H, SCR_W};
use plx_machine::machine::GroupId;
use plx_ui::Rect;

/// `pub`, alongside this module itself (`screens/search/mod.rs`'s `pub mod layout`)
/// so `ui::consts`'s overscan-rects audit can reach it as `crate::search::layout::FIELD`
/// — the deleted legacy Search renderer's own `FIELD`/`CONTENT_TOP` this replaces.
pub const FIELD: Rect = Rect {
    x: MARGIN_X,
    y: 138.0,
    w: SCR_W - 2.0 * MARGIN_X,
    h: 80.0,
};
/// See [`FIELD`]'s doc: also reachable as `crate::search::CONTENT_TOP`. Value pinned at
/// 300.0 — the overscan audit and the page's `Head` section height (`Stack` stacks the shelves
/// below it) both depend on it.
pub const CONTENT_TOP: f32 = 300.0;
pub(super) const HEAD_TO_ROW: f32 = 60.0;
pub(super) const KEYBOARD_H: f32 = 324.0;
pub(super) const CHIP_H: f32 = 60.0;
const CHIP_GAP: f32 = plx_ui::theme::space::SM;
pub(super) const CHIP_SIZE: i32 = plx_ui::theme::size::BODY;
pub(super) const HINT_SIZE: i32 = plx_ui::theme::size::BODY;

pub(super) fn ordinal(kind: Kind) -> u32 {
    match kind {
        Kind::Movie => 0,
        Kind::Show => 1,
        Kind::Episode => 2,
        Kind::Person => 3,
        Kind::Collection => 4,
    }
}
pub(super) fn group(kind: Kind) -> GroupId {
    GroupId(0x5345_4200 + ordinal(kind))
}
static EPISODE: RowStyle = RowStyle { w: 420.0, h: 236.0, ..RowStyle::HOME };
static PERSON: RowStyle = RowStyle { w: 250.0, h: 250.0, circular: true, ..RowStyle::HOME };
static POSTER: RowStyle = RowStyle { w: CARD_W, h: CARD_H, ..RowStyle::HOME };

/// The row style of `kind`'s shelf: the poster shelf's, with the still and the headshot sized to
/// their own aspect.
pub(super) fn style(kind: Kind) -> &'static RowStyle {
    match kind {
        Kind::Episode => &EPISODE,
        Kind::Person => &PERSON,
        _ => &POSTER,
    }
}
/// Whether `kind`'s tiles are POSTERS, and so follow the Under posters setting (their caption
/// built by `registry::captions`, their band sized by `caption_policy::band_h`). The Episode row is a
/// landscape still and the Person row a headshot: neither is a poster, and both keep the legacy
/// block under every setting.
pub(super) fn is_poster(kind: Kind) -> bool {
    matches!(kind, Kind::Movie | Kind::Show | Kind::Collection)
}
/// A shelf block's height under a caption band of `band` px (a shelf's live
/// [`Shelf::under_band`](plx_ui::cards::Shelf::under_band), or [`card_row::under_band`] of a
/// settled 0 / 1).
pub(super) fn block_h(kind: Kind, band: f32) -> f32 {
    HEAD_TO_ROW + style(kind).h + caption_band(band)
}
pub(super) fn caption_band(band: f32) -> f32 {
    plx_ui::consts::UNDER_LABEL_AIR + band
}

/// The recent-search chips and then Clear, left to right from the field's left edge, each with
/// the label it draws; `y` is 0, the row's top. Terms share the width left after Clear evenly
/// and are elided to it, so the whole row fits under the field.
pub(super) fn chips(terms: &[String], measure: &dyn plx_machine::machine::Measure) -> Vec<(Rect, String)> {
    let cstr = |s: &str| std::ffi::CString::new(s).unwrap_or_default();
    let pill = |label: &str| plx_ui::widgets::Button::pill_w_measured(&cstr(label), CHIP_SIZE, true, false, measure);
    let clear = plx_platform::i18n::msg::browse_search_clear();
    let n = terms.len().max(1) as f32;
    let budget = ((FIELD.w - pill(clear) - n * CHIP_GAP) / n - pill("")).max(0.0);
    let labels = terms.iter()
        .map(|term| plx_machine::machine::elide_by(term, budget, false, |s| measure.width(&cstr(s), CHIP_SIZE, true)))
        .chain([clear.to_owned()]);
    let mut x = FIELD.x;
    labels.map(|label| {
        let rect = Rect::new(x, 0.0, pill(&label), CHIP_H);
        x += rect.w + CHIP_GAP;
        (rect, label)
    }).collect()
}

/// The top of the hint line: under the chips, or where they would be when there are none.
pub(super) fn hint_y(chips: bool) -> f32 {
    if chips { CONTENT_TOP + CHIP_H + plx_ui::theme::space::MD } else { CONTENT_TOP }
}

pub(super) fn empty_band(editing: bool) -> Rect {
    let bottom = if editing { SCR_H - KEYBOARD_H } else { SCR_H };
    Rect::new(0.0, CONTENT_TOP, SCR_W, bottom - CONTENT_TOP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chip_row_fits_under_the_field_however_long_the_terms() {
        let measure = plx_ui::fixture::FixtureMeasure;
        let long = "a very long remembered search term that would never fit".to_owned();
        let row = chips(&[long.clone(), long.clone(), long.clone(), long], &measure);
        assert_eq!(row.len(), 5, "four terms and Clear");
        assert_eq!(row[0].0.x, FIELD.x);
        for pair in row.windows(2) {
            assert!(pair[1].0.x > pair[0].0.x + pair[0].0.w, "left to right, apart");
        }
        assert!(row[4].0.x + row[4].0.w <= FIELD.x + FIELD.w, "Clear ends inside the field's width");
        assert!(row[0].1.ends_with('\u{2026}'), "a long term is elided: {}", row[0].1);
        let short = chips(&["up".to_owned()], &measure);
        assert_eq!(short[0].1, "up", "a short term is kept whole");
    }

    #[test]
    fn the_hint_line_sits_under_the_chips_or_takes_their_place() {
        assert_eq!(hint_y(false), CONTENT_TOP);
        assert!(hint_y(true) >= CONTENT_TOP + CHIP_H);
    }

    fn full() -> f32 {
        plx_ui::cards::under_band(1.0)
    }
    const ALL: [Kind; 5] = plx_data::search::KINDS;

    #[test]
    fn the_reserved_caption_band_holds_the_block_the_shared_component_draws() {
        let drawn = plx_ui::cards::TileLabel::height(true);
        assert!(
            drawn <= caption_band(full()),
            "the label block draws {drawn}px into a band of {}px",
            caption_band(full())
        );
    }

    #[test]
    fn block_heights_are_the_row_pitch_for_posters_and_follow_the_style_otherwise() {
        assert_eq!(block_h(Kind::Movie, full()), plx_ui::consts::ROW_PITCH);
        assert_eq!(block_h(Kind::Show, full()), plx_ui::consts::ROW_PITCH);
        assert_eq!(block_h(Kind::Collection, full()), plx_ui::consts::ROW_PITCH);
        assert_eq!(block_h(Kind::Episode, full()), HEAD_TO_ROW + 236.0 + caption_band(full()));
        assert_eq!(block_h(Kind::Person, full()), HEAD_TO_ROW + 250.0 + caption_band(full()));
        assert_eq!(block_h(Kind::Movie, full()) - block_h(Kind::Episode, full()), CARD_H - 236.0);
    }

    #[test]
    fn the_first_shelfs_whole_row_clears_the_raised_keyboard() {
        let floor = SCR_H - KEYBOARD_H;
        for kind in ALL {
            let bottom = CONTENT_TOP + HEAD_TO_ROW + style(kind).h;
            assert!(bottom <= floor, "{kind:?}: the first row ends at {bottom}, under the keyboard at {floor}");
        }
        assert_eq!(CONTENT_TOP + HEAD_TO_ROW + CARD_H, 735.0);
        assert_eq!(floor, 756.0);
    }

    #[test]
    fn the_content_line_clears_the_documents_head() {
        let head_bottom = FIELD.y + FIELD.h;
        assert!(
            CONTENT_TOP > head_bottom,
            "content starts at {CONTENT_TOP} inside a head that ends at {head_bottom}"
        );
    }

    #[test]
    fn the_hint_under_the_chips_finishes_clear_of_the_raised_keyboard() {
        let kbd_top = SCR_H - KEYBOARD_H;
        let bottom = hint_y(true) + HINT_SIZE as f32 * 1.35;
        let clearance = kbd_top - bottom;
        assert!(
            clearance >= plx_ui::theme::space::LG,
            "the hint ends at {bottom} and the keyboard starts at {kbd_top} — {clearance}px"
        );
    }
}
