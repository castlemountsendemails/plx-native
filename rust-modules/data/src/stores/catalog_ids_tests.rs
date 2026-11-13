//! The catalog-id lookup's forward store: what it asks, what it keeps, and what it never keeps.

use super::*;
use crate::watchlist::ledger::Intent;
use crate::watchlist::model::{Kind, Reach};
use crate::watchlist::WatchlistAuth;
use plx_plex::plex::watchlist::{WatchItem, WatchPage, WatchlistError};
use plx_plex::plex::{GuidKind, GuidLookupError, GuidMatch, ServerId};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const HEX: [u8; 12] = [0x5d; 12];

/// Discover as a script: each ask takes the next answer, `Err` once the script is spent.
#[derive(Default)]
struct Script {
    answers: Mutex<Vec<Result<Option<[u8; 12]>, WatchlistError>>>,
    asks: AtomicUsize,
    no_account: AtomicBool,
}

impl Reach for Script {
    fn reachable(&self, _: ServerId) -> bool { true }
    fn roster(&self) -> Vec<ServerId> { Vec::new() }
    fn copy_rank(&self, _: ServerId) -> u8 { 0 }
}

impl WatchlistIo for Script {
    fn capture_auth(&self) -> Option<WatchlistAuth> {
        (!self.no_account.load(Ordering::SeqCst)).then(|| WatchlistAuth {
            client_id: "test-client".into(), credential: "test-credential".into(), user: Default::default(), generation: 1,
        })
    }
    fn is_current(&self, _: &WatchlistAuth) -> bool { true }
    fn page(&self, _: &WatchlistAuth, _: u32, _: u32, _: Option<&str>) -> Result<WatchPage, WatchlistError> {
        Err(WatchlistError::Transport)
    }
    fn released(&self, _: &WatchlistAuth) -> Result<Vec<WatchItem>, WatchlistError> { Err(WatchlistError::Transport) }
    fn write(&self, _: &WatchlistAuth, _: &str, _: Intent) -> Result<(), WatchlistError> { Err(WatchlistError::Transport) }
    fn find(&self, _: ServerId, _: GuidKind, _: &[String]) -> Result<Vec<GuidMatch>, GuidLookupError> {
        Err(GuidLookupError::Unanswered)
    }
    fn roster_gen(&self) -> u32 { 1 }
    fn facts_gen(&self) -> u32 { 1 }
    fn today(&self) -> Option<String> { None }
    fn catalog_match(&self, _: &WatchlistAuth, _: &LegacyTitle) -> Result<Option<[u8; 12]>, WatchlistError> {
        self.asks.fetch_add(1, Ordering::SeqCst);
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() { Err(WatchlistError::Transport) } else { answers.remove(0) }
    }
}

fn inline(_: &str, f: Box<dyn FnOnce() + Send + 'static>) -> bool {
    f();
    true
}

fn store(script: &Arc<Script>) -> CatalogIdStore {
    CatalogIdStore::with_io(Arc::clone(script) as Arc<dyn WatchlistIo>).with_spawn_for_test(inline)
}

fn annie_hall() -> TitleId {
    TitleId::Legacy(LegacyTitle::of(Kind::Movie, "com.plexapp.agents.imdb://tt0075686?lang=en").unwrap())
}

fn legacy(id: &TitleId) -> &LegacyTitle {
    match id {
        TitleId::Legacy(t) => t,
        TitleId::Catalog(_) => unreachable!(),
    }
}

/// Ask, run the worker inline, and land it: two pumps (the ask spawns on one, lands on the next).
fn settle(s: &mut CatalogIdStore, id: &TitleId) -> (Resolved, Vec<Landing>) {
    s.resolve(id);
    let mut landed = s.pump_for_test();
    landed.extend(s.pump_for_test());
    (s.resolve(id), landed)
}

#[test]
fn a_catalog_id_needs_no_ask() {
    let script = Arc::new(Script::default());
    let mut s = store(&script);
    assert_eq!(s.resolve(&TitleId::Catalog(HEX)), Resolved::Hex(HEX));
    assert!(s.pump_for_test().is_empty());
    assert_eq!(script.asks.load(Ordering::SeqCst), 0);
}

#[test]
fn a_found_id_is_asked_once_and_kept() {
    let script = Arc::new(Script::default());
    script.answers.lock().unwrap().push(Ok(Some(HEX)));
    let mut s = store(&script);
    let id = annie_hall();
    assert_eq!(s.resolve(&id), Resolved::Waiting, "asked, not yet answered");
    let (now, landed) = settle(&mut s, &id);
    assert_eq!(now, Resolved::Hex(HEX));
    assert_eq!(landed, vec![(legacy(&id).clone(), Some(Match::Found(HEX)))]);
    s.resolve(&id);
    assert!(s.pump_for_test().is_empty());
    assert_eq!(script.asks.load(Ordering::SeqCst), 1, "the answer is kept for the run");
}

#[test]
fn the_catalogs_no_match_is_an_answer() {
    let script = Arc::new(Script::default());
    script.answers.lock().unwrap().push(Ok(None));
    let mut s = store(&script);
    let id = annie_hall();
    let (now, landed) = settle(&mut s, &id);
    assert_eq!(now, Resolved::NoMatch);
    assert_eq!(landed, vec![(legacy(&id).clone(), Some(Match::NoMatch))]);
}

#[test]
fn a_failed_lookup_is_not_stored_and_waits_for_a_retry() {
    let script = Arc::new(Script::default());
    script.answers.lock().unwrap().extend([Err(WatchlistError::Other(429)), Ok(Some(HEX))]);
    let mut s = store(&script);
    let id = annie_hall();
    let (now, landed) = settle(&mut s, &id);
    assert_eq!(now, Resolved::Waiting, "a failure is never shown as a result");
    assert_eq!(landed, vec![(legacy(&id).clone(), None)]);
    for _ in 0..3 {
        s.resolve(&id);
        assert!(s.pump_for_test().is_empty(), "not re-asked every frame");
    }
    assert_eq!(script.asks.load(Ordering::SeqCst), 1);
    s.retry(legacy(&id));
    let (now, _) = settle(&mut s, &id);
    assert_eq!(now, Resolved::Hex(HEX), "the press asked again");
    assert_eq!(script.asks.load(Ordering::SeqCst), 2);
}

#[test]
fn a_page_opening_lets_a_failed_title_be_asked_again() {
    let script = Arc::new(Script::default());
    script.answers.lock().unwrap().extend([Err(WatchlistError::Transport), Ok(None)]);
    let mut s = store(&script);
    let id = annie_hall();
    settle(&mut s, &id);
    s.forget_failures();
    assert_eq!(settle(&mut s, &id).0, Resolved::NoMatch);
}

#[test]
fn with_no_account_the_ask_fails_at_once_without_a_request() {
    let script = Arc::new(Script::default());
    script.no_account.store(true, Ordering::SeqCst);
    let mut s = store(&script);
    let id = annie_hall();
    s.resolve(&id);
    assert_eq!(s.pump_for_test(), vec![(legacy(&id).clone(), None)]);
    assert_eq!(script.asks.load(Ordering::SeqCst), 0);
}
