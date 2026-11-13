//! The catalog-id lookup's forward half as an owned store (the types are `crate::catalog_ids`): a
//! surface asks what a legacy title's catalog id is ([`CatalogIdStore::resolve`]), a worker asks
//! Discover, and the answer lands at the next pump. It is never reset: the answers are
//! account-independent, so a profile switch keeps them. It also owns the record cache the
//! Watchlist's match workers share ([`CatalogIdStore::records`]).
//!
//! A failed ask is not stored and not asked again on its own: [`CatalogIdStore::retry`] (a press)
//! and [`CatalogIdStore::forget_failures`] (a page opening, a watchlist load) let the next resolve
//! ask again.

use crate::catalog_ids::{LegacyTitle, Match, RecordIds, Resolved, TitleId};
use crate::watchlist::{LiveIo, WatchlistIo};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

type Spawn = fn(&str, Box<dyn FnOnce() + Send + 'static>) -> bool;

fn spawn_worker(what: &str, f: Box<dyn FnOnce() + Send + 'static>) -> bool {
    plx_base::task::spawn_small(what, f)
}

/// One ask's outcome as a pump reports it: `None` is a failed ask.
pub type Landing = (LegacyTitle, Option<Match>);

#[derive(Default)]
struct Mail {
    /// Titles a surface asked about from `&self`, spawned by the next pump.
    wanted: Mutex<Vec<LegacyTitle>>,
    landed: Mutex<Vec<Landing>>,
}

pub struct CatalogIdStore {
    answers: HashMap<LegacyTitle, Match>,
    asking: HashSet<LegacyTitle>,
    failed: HashSet<LegacyTitle>,
    mail: Arc<Mail>,
    records: Arc<RecordIds>,
    io: Arc<dyn WatchlistIo>,
    spawn: Spawn,
}

impl Default for CatalogIdStore {
    fn default() -> Self {
        Self::with_io(Arc::new(LiveIo))
    }
}

impl CatalogIdStore {
    pub fn with_io(io: Arc<dyn WatchlistIo>) -> Self {
        Self {
            answers: HashMap::new(),
            asking: HashSet::new(),
            failed: HashSet::new(),
            mail: Arc::default(),
            records: Arc::default(),
            io,
            spawn: spawn_worker,
        }
    }

    /// The catalog-to-server half's cache, for the match workers.
    pub fn records(&self) -> Arc<RecordIds> {
        Arc::clone(&self.records)
    }

    /// MAIN THREAD. What `id` stands for now. A legacy title with no answer is asked about at the
    /// next pump, unless its last ask failed.
    pub fn resolve(&self, id: &TitleId) -> Resolved {
        let title = match id {
            TitleId::Catalog(hex) => return Resolved::Hex(*hex),
            TitleId::Legacy(title) => title,
        };
        match self.answers.get(title) {
            Some(Match::Found(hex)) => Resolved::Hex(*hex),
            Some(Match::NoMatch) => Resolved::NoMatch,
            None => {
                if !self.failed.contains(title) {
                    self.want(title);
                }
                Resolved::Waiting
            }
        }
    }

    /// Ask about `title` again even though its last ask failed.
    pub fn retry(&mut self, title: &LegacyTitle) {
        self.failed.remove(title);
        if !self.answers.contains_key(title) {
            self.want(title);
        }
    }

    /// Let every title whose last ask failed be asked again by the next resolve.
    pub fn forget_failures(&mut self) {
        self.failed.clear();
    }

    fn want(&self, title: &LegacyTitle) {
        if self.asking.contains(title) {
            return;
        }
        let mut wanted = self.mail.wanted.lock().unwrap_or_else(|e| e.into_inner());
        if !wanted.contains(title) {
            wanted.push(title.clone());
        }
    }

    /// MAIN THREAD, once a frame: land the answers that arrived and start the asks owed. Returns
    /// this frame's outcomes, so a press waiting on a title can finish.
    pub fn pump(&mut self, gate: &plx_machine::landgate::Gate) -> Vec<Landing> {
        let (mail, claimed) = (&self.mail, self.asking.len());
        let mut out = super::take_landings_owed(gate, super::StoreId::Watchlist, |got| got.len() < claimed, || {
            mail.landed.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
        });
        for (title, answer) in &out {
            self.asking.remove(title);
            match answer {
                Some(m) => {
                    self.answers.insert(title.clone(), *m);
                }
                None => {
                    self.failed.insert(title.clone());
                }
            }
        }
        let wanted: Vec<LegacyTitle> = self.mail.wanted.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
        if wanted.is_empty() {
            return out;
        }
        // The identity is captured here, on the main thread; the answer is account-independent.
        let auth = self.io.capture_auth();
        for title in wanted {
            if self.answers.contains_key(&title) || self.asking.contains(&title) {
                continue;
            }
            let Some(auth) = auth.clone() else {
                self.failed.insert(title.clone());
                out.push((title, None));
                continue;
            };
            self.asking.insert(title.clone());
            let (mail, io, key) = (Arc::clone(&self.mail), Arc::clone(&self.io), title.clone());
            let spawned = (self.spawn)("catalog-id", Box::new(move || {
                let asked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| io.catalog_match(&auth, &key)));
                let answer = match asked {
                    Ok(Ok(Some(hex))) => Some(Match::Found(hex)),
                    Ok(Ok(None)) => Some(Match::NoMatch),
                    Ok(Err(_)) | Err(_) => None,
                };
                mail.landed.lock().unwrap_or_else(|e| e.into_inner()).push((key, answer));
                plx_machine::idle::invalidate();
            }));
            if !spawned {
                // Nothing will run: forget the claim so a later resolve asks again.
                self.asking.remove(&title);
            }
        }
        out
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_spawn_for_test(mut self, spawn: Spawn) -> Self {
        self.spawn = spawn;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pump_for_test(&mut self) -> Vec<Landing> {
        self.pump(plx_machine::landgate::fixture_gate())
    }
}

#[cfg(test)]
#[path = "catalog_ids_tests.rs"]
mod tests;
