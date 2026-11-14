//! A read-back that patches a row of a listing whose MEMBERSHIP depends on viewing (the Unwatched
//! switch, ordering by last viewed) marks that section stale; the stale listing is re-run exactly
//! once, on request, and never by the patch itself.

use super::test_support::*;
use super::*;
use crate::viewstate::{ViewAt, ViewEdit};

const WATCHED_FACTS: ViewEdit = ViewEdit::Facts { watched: true, unwatched: false, resume_ms: 0 };

fn resident(sid: ServerId, browse: &mut TestBrowse) {
    let state = browse.state.state_mut(0).unwrap();
    state.fetch = SecFetch::Ready;
    state.total = 1;
    state.items = SecItems::from_vec(vec![Some(PmsMovie { sid, rk: "7".into(), unwatched: true, ..Default::default() })]);
    state.cursor = Some(Arc::new(Cursor { at: CursorAt::SlotIndex(0), scroll: 0.0 }));
}

fn apply(browse: &mut TestBrowse, sid: ServerId, edit: ViewEdit) -> bool {
    browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::ApplyView {
        at: ViewAt::leaf(sid, "7"), edit })
}

fn stale(browse: &mut TestBrowse) -> bool {
    browse.state.listing_snapshot().view().membership_stale()
}

#[test]
fn a_facts_patch_in_an_unwatched_filtered_listing_marks_it_stale_without_requerying() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    let gen = browse.state.query_gen();
    assert!(apply(&mut browse, sid, WATCHED_FACTS));
    let row = browse.state.states()[0].items.get(0).unwrap();
    assert!(row.watched && !row.unwatched, "the row is patched where it stands");
    assert!(stale(&mut browse), "…and the listing knows its membership is out of date");
    assert_eq!(browse.state.query_gen(), gen, "the patch itself never re-queries under the cursor");
    assert_eq!(browse.state.states()[0].fetch, SecFetch::Ready);
    assert!(!browse.state.requery_owed());
}

#[test]
fn the_optimistic_press_alone_does_not_mark_a_listing_stale() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    assert!(apply(&mut browse, sid, ViewEdit::Watched(true)));
    assert!(!stale(&mut browse), "only the server's answer says the membership changed");
}

#[test]
fn a_listing_whose_query_ignores_viewing_stays_current() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    resident(sid, &mut browse);
    assert!(apply(&mut browse, sid, WATCHED_FACTS));
    assert!(!stale(&mut browse), "ordered by title with no filter: the row stays where it is");
}

#[test]
fn ordering_by_last_viewed_makes_a_facts_patch_stale() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    resident(sid, &mut browse);
    browse.state.state_mut(0).unwrap().sorts = Arc::new(vec![SortEntry {
        key: "lastViewedAt".into(), desc_key: "lastViewedAt:desc".into(), title: "Last Viewed".into(), default_desc: true }]);
    assert!(apply(&mut browse, sid, WATCHED_FACTS));
    assert!(stale(&mut browse));
}

#[test]
fn requery_stale_runs_the_query_once_and_keeps_the_readers_place() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    assert!(apply(&mut browse, sid, WATCHED_FACTS));
    let gen = browse.state.query_gen();
    let shown = browse.state.listing_snapshot().view().id().unwrap().query;

    assert!(browse.state.requery_stale());
    assert_eq!(browse.state.query_gen(), gen.wrapping_add(1), "exactly one re-fetch: a late page of the old fetch is dropped");
    assert_eq!(browse.state.listing_snapshot().view().id().unwrap().query, shown,
        "…but the screen-facing query identity stands: a membership refresh is not a query edit");
    assert!(!stale(&mut browse), "the flag is spent");
    let state = &browse.state.states()[0];
    assert_eq!((state.fetch, state.total), (SecFetch::Loading, -1));
    assert!(state.cursor.is_some(), "a membership re-query is not a new query: the reader's place stands");
    assert!(browse.state.requery_owed(), "the first page is owed with no Library page ticking the store");

    assert!(!browse.state.requery_stale(), "a second ask has nothing to do");
    assert_eq!(browse.state.query_gen(), gen.wrapping_add(1));

    browse.state.set_unwatched(false); // a real query edit
    assert_ne!(browse.state.listing_snapshot().view().id().unwrap().query, shown, "a query edit still moves it");
    browse.state.set_unwatched(true);

    browse.state.set_unwatched(false); // the reader does something else: the Library page is ticking again
    assert!(!browse.state.requery_owed());
}

#[test]
fn a_stale_section_that_is_not_showing_forgets_its_pages() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    browse.append_sections(0, vec![(2, "Other".into(), SecKind::Movie)]);
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    browse.state.want(0, PAGE, None); // the window the grid last showed, kept as section 0 is left
    browse.state.set_cur(1);
    assert!(apply(&mut browse, sid, WATCHED_FACTS));
    let first = &browse.state.states()[0];
    assert_eq!((first.fetch, first.total), (SecFetch::Loading, -1), "its next visit fetches afresh");
    assert!(!first.membership_stale);
}

#[test]
fn an_agreeing_read_back_still_marks_an_unwatched_filtered_listing_stale() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    assert!(apply(&mut browse, sid, ViewEdit::Watched(true)));
    assert!(!stale(&mut browse), "the press alone says nothing");
    assert!(apply(&mut browse, sid, WATCHED_FACTS), "marking the listing stale is a change the store must publish");
    let row = browse.state.states()[0].items.get(0).unwrap();
    assert!(row.watched && !row.unwatched);
    assert!(stale(&mut browse), "the server's answer agrees with the row, and that answer is what marks staleness");
    assert!(!apply(&mut browse, sid, WATCHED_FACTS), "a second agreeing answer changes nothing");
}

#[test]
fn an_agreeing_read_back_resets_a_stale_section_that_is_not_showing() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    browse.append_sections(0, vec![(2, "Other".into(), SecKind::Movie)]);
    assert!(browse.state.set_unwatched(true));
    resident(sid, &mut browse);
    browse.state.want(0, PAGE, None);
    browse.state.set_cur(1);
    assert!(apply(&mut browse, sid, ViewEdit::Watched(true)));
    assert!(apply(&mut browse, sid, WATCHED_FACTS), "the reset is a change the store must publish");
    let first = &browse.state.states()[0];
    assert_eq!((first.fetch, first.total), (SecFetch::Loading, -1), "its next visit fetches afresh");
}

/// Rows rk `0 ..= n-1` in section 0, one more page than `n` rows need to be worth shifting across.
fn rows(sid: ServerId, browse: &mut TestBrowse, n: usize) {
    let state = browse.state.state_mut(0).unwrap();
    state.fetch = SecFetch::Ready;
    state.total = n as i64;
    state.items = SecItems::from_vec((0..n).map(|i| Some(PmsMovie { sid, rk: i.to_string(), ..Default::default() })).collect());
    state.cursor = Some(Arc::new(Cursor { at: CursorAt::SlotIndex(0), scroll: 0.0 }));
}

fn rk_at(browse: &TestBrowse, i: usize) -> Option<String> {
    browse.state.states()[0].items.get(i).map(|m| m.rk.clone())
}

/// A deleted card must leave the listing in view now, with the rows after it moved up one, so the
/// reader's place falls to the neighbour. It is not a re-query: a re-query empties the grid until
/// the server answers, and a focus that is reconciled in that gap falls to the header.
#[test]
fn a_deleted_item_leaves_the_listing_in_view_at_once_and_the_rows_after_it_move_up() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    rows(sid, &mut browse, PAGE + 3); // the shift crosses a page boundary
    let gen = browse.state.query_gen();
    let shown = browse.state.listing_snapshot().view().id().unwrap().query;
    let removed = browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::ApplyView {
        at: ViewAt::leaf(sid, "1"), edit: ViewEdit::Removed });
    assert!(removed);
    assert_eq!(rk_at(&browse, 0).as_deref(), Some("0"), "rows before it stay");
    assert_eq!(rk_at(&browse, 1).as_deref(), Some("2"), "the neighbour takes its place");
    assert_eq!(rk_at(&browse, PAGE - 1).as_deref(), Some(PAGE.to_string().as_str()), "…across the page boundary");
    assert_eq!(rk_at(&browse, PAGE + 1).as_deref(), Some((PAGE + 2).to_string().as_str()), "…to the end");
    assert_eq!(rk_at(&browse, PAGE + 2), None, "the listing is one row shorter");
    let state = &browse.state.states()[0];
    assert_eq!((state.fetch, state.total), (SecFetch::Ready, PAGE as i64 + 2), "never empty, never loading");
    assert!(state.cursor.is_some(), "the reader's place stands");
    assert_eq!(browse.state.query_gen(), gen, "no re-fetch");
    assert!(!browse.state.requery_owed());
    assert!(!stale(&mut browse));
    assert_eq!(browse.state.listing_snapshot().view().id().unwrap().query, shown, "not a query edit");
}

/// A show deleted from an Episodes listing takes its episodes with it: every covered row goes, and
/// the gap closes by the number that went.
#[test]
fn a_deleted_show_takes_all_its_rows_out_of_the_listing_in_view() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    rows(sid, &mut browse, 6);
    for rk in ["1", "2", "4"] {
        let state = browse.state.state_mut(0).unwrap();
        let mut m = state.items.get(rk.parse().unwrap()).unwrap().clone();
        (m.kind, m.show_rk) = (3, "S".into()); // episodes of one show, rows 1, 2 and 4
        state.items.set(rk.parse().unwrap(), m);
    }
    let removed = browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::ApplyView {
        at: ViewAt { sid, rk: "S".into(), scope: crate::viewstate::Scope::Show }, edit: ViewEdit::Removed });
    assert!(removed);
    let left: Vec<_> = (0..4).map(|i| rk_at(&browse, i)).collect();
    assert_eq!(left, ["0", "3", "5"].map(|rk| Some(rk.to_string())).into_iter().chain([None]).collect::<Vec<_>>());
    assert_eq!(browse.state.states()[0].total, 3);
}

#[test]
fn a_deleted_item_in_a_section_that_is_not_showing_makes_it_forget_its_pages() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    browse.append_sections(0, vec![(2, "Other".into(), SecKind::Movie)]);
    resident(sid, &mut browse);
    browse.state.want(0, PAGE, None); // the window the grid last showed, kept as section 0 is left
    browse.state.set_cur(1);
    assert!(apply(&mut browse, sid, ViewEdit::Removed));
    let first = &browse.state.states()[0];
    assert_eq!((first.fetch, first.total), (SecFetch::Loading, -1), "its next visit fetches afresh");
}

#[test]
fn a_deleted_item_the_listing_never_held_changes_nothing() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    resident(sid, &mut browse);
    let other = browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::ApplyView {
        at: ViewAt::leaf(sid, "99"), edit: ViewEdit::Removed });
    assert!(!other);
    assert!(!stale(&mut browse));
}

/// The deletion flag comes from `GET /`, and a roster from plex.tv already names its servers, so
/// the root must be read for the flag even when no name is wanted: the Sections job reads it, the
/// Counts job (which neither needs the name nor the flag) does not.
#[test]
fn a_sections_job_reads_the_root_even_when_the_roster_named_the_server() {
    assert!(reads_root(false, true), "named by plex.tv: the flag still needs the root");
    assert!(reads_root(true, true));
    assert!(reads_root(true, false), "an unnamed source asks on whichever job comes first");
    assert!(!reads_root(false, false), "a count job reads no root");
}

/// The server answers the page at `start` with `rks`, `total` titles in all, for the query in force.
fn land_page(browse: &mut TestBrowse, client: &'static plx_plex::plex::Client, start: usize, rks: &[String], total: i64) {
    let sid = client.id();
    browse.adapter.fetching.store(true, Ordering::SeqCst);
    *browse.adapter.page_result.lock().unwrap() = Some(PageResult {
        client, token_gen: client.token_gen(), gen: browse.state.query_gen(), sec: browse.state.cur(), start,
        items: rks.iter().map(|rk| PmsMovie { sid, rk: rk.clone(), ..Default::default() }).collect(),
        total, sorts: None, restored: None, genres: None, genre: None, resolved: Default::default(),
    });
    assert!(browse.state.pump_owned(&browse.adapter).changed, "the page landed");
    plx_base::task::drain_workers_for_test();
}

/// A title created or deleted on the open library's server re-reads its loaded pages in place:
/// the listing never empties or reads as loading, each row stays until its page lands again, and
/// a landed page replaces its rows where they stand (here the server lists a new title first).
#[test]
fn a_library_change_rereads_the_listing_in_view_over_its_rows() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, client) = loopback_page_source();
    rows(sid, &mut browse, PAGE + 3);
    browse.state.want(0, PAGE + 3, None);
    let gen = browse.state.query_gen();
    let shown = browse.state.listing_snapshot().view().id().unwrap().query;
    assert!(browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::LibraryChanged { sid }));
    let state = &browse.state.states()[0];
    assert_eq!((state.fetch, state.total), (SecFetch::Ready, PAGE as i64 + 3), "never empty, never loading");
    assert_eq!(state.stale_pages, [0, 1], "every loaded page is owed again");
    assert!(state.cursor.is_some(), "the reader's place stands");
    assert_eq!(rk_at(&browse, 1).as_deref(), Some("1"), "the rows stay until their page lands");
    assert_eq!(browse.state.query_gen(), gen.wrapping_add(1), "a page still out is dropped");
    assert_eq!(browse.state.listing_snapshot().view().id().unwrap().query, shown, "not a query edit");

    let first: Vec<String> = std::iter::once("new".to_string()).chain((0..PAGE - 1).map(|i| i.to_string())).collect();
    land_page(&mut browse, client, 0, &first, PAGE as i64 + 4);
    let state = &browse.state.states()[0];
    assert_eq!((state.fetch, state.total), (SecFetch::Ready, PAGE as i64 + 4));
    assert_eq!(rk_at(&browse, 0).as_deref(), Some("new"), "the landed page replaces its rows");
    assert_eq!(rk_at(&browse, 1).as_deref(), Some("0"));
    assert_eq!(browse.state.states()[0].stale_pages, [1], "the next page is still owed");
    assert_eq!(rk_at(&browse, PAGE).as_deref(), Some(PAGE.to_string().as_str()), "its old rows stay until it lands");
}

/// A change on a server the open library is not on re-reads nothing.
#[test]
fn a_library_change_on_another_server_leaves_the_listing_in_view_alone() {
    let _g = plx_base::testlock::serial();
    let (_cleanup, mut browse, sid, _) = registered_page_source();
    rows(sid, &mut browse, 3);
    let gen = browse.state.query_gen();
    let other = ServerId::from_raw(sid.raw().wrapping_add(1));
    assert!(!browse.state.run_owned(&browse.adapter, crate::stores::browse::BrowseCmd::LibraryChanged { sid: other }));
    assert!(browse.state.states()[0].stale_pages.is_empty());
    assert_eq!(browse.state.query_gen(), gen);
}
