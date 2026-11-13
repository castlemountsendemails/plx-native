//! The catalog-id lookup, both halves: a title a server knows only by a legacy-agent guid to the
//! catalog id the Watchlist reads and writes (Discover `matches`, `stores::catalog_ids` asks it on
//! demand), and a catalog id to the legacy forms a server may hold it under (the catalog record's
//! `Guid[]`, [`RecordIds`], read by the Watchlist's match workers). The Watchlist is the first
//! caller; nothing here is Watchlist state.
//!
//! The answers are account-independent, so they are kept in memory for the run and survive a
//! profile switch. A failed lookup (catalog unreachable, refused, throttled) is never stored.

use crate::watchlist::model::Kind;
use plx_plex::plex::watchlist::WatchlistError;
use std::collections::HashMap;
use std::sync::Mutex;

/// A title known by its bare legacy-agent guid ([`plx_plex::plex::legacy_catalog_guid`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LegacyTitle {
    pub kind: Kind,
    pub guid: String,
}

impl LegacyTitle {
    /// `None` for a guid the catalog cannot match (`local://`, `none`, another agent).
    pub fn of(kind: Kind, guid: &str) -> Option<LegacyTitle> {
        plx_plex::plex::legacy_catalog_guid(guid).map(|g| LegacyTitle { kind, guid: g.to_owned() })
    }
}

/// What the Watchlist knows a title by when a screen first sees it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TitleId {
    Catalog([u8; 12]),
    Legacy(LegacyTitle),
}

impl TitleId {
    /// A row's own catalog id, else its legacy title. `None`: the Watchlist cannot name it.
    pub fn of(kind: Kind, hex: Option<[u8; 12]>, legacy: Option<&str>) -> Option<TitleId> {
        match hex {
            Some(hex) => Some(TitleId::Catalog(hex)),
            None => legacy.and_then(|g| LegacyTitle::of(kind, g)).map(TitleId::Legacy),
        }
    }
}

/// The catalog's answer for a legacy title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Found([u8; 12]),
    NoMatch,
}

/// A title as a surface may act on it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    Hex([u8; 12]),
    /// Asked and not answered (or the last ask failed): the surface shows the Add face, and a press
    /// waits for the answer.
    Waiting,
    /// The catalog has no such title: no disc and no menu row.
    NoMatch,
}

/// The catalog-to-server half's cache: the external ids of a catalog record, by catalog id.
/// Shared by every match worker.
#[derive(Default)]
pub struct RecordIds {
    by_hex: Mutex<HashMap<[u8; 12], Vec<String>>>,
}

impl RecordIds {
    /// WORKER. The legacy forms the title `hex` may be held under, read through `fetch` (the
    /// catalog record's `Guid[]`) the first time it is asked this run. A record the catalog does
    /// not know has none. `Err`: the record could not be read, and nothing was stored.
    pub fn legacy_forms(
        &self, hex: &[u8; 12], fetch: impl FnOnce() -> Result<Vec<String>, WatchlistError>,
    ) -> Result<Vec<String>, WatchlistError> {
        if let Some(ids) = self.by_hex.lock().unwrap_or_else(|e| e.into_inner()).get(hex) {
            return Ok(plx_plex::plex::legacy_forms(ids));
        }
        let ids = match fetch() {
            Ok(ids) => ids,
            Err(WatchlistError::NotFound) => Vec::new(),
            Err(e) => return Err(e),
        };
        let forms = plx_plex::plex::legacy_forms(&ids);
        self.by_hex.lock().unwrap_or_else(|e| e.into_inner()).insert(*hex, ids);
        Ok(forms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: [u8; 12] = [7; 12];

    #[test]
    fn a_title_is_named_by_its_catalog_id_first_and_only_matchable_legacy_guids_otherwise() {
        let legacy = "com.plexapp.agents.imdb://tt0075686?lang=en";
        assert_eq!(TitleId::of(Kind::Movie, Some(HEX), Some(legacy)), Some(TitleId::Catalog(HEX)));
        assert_eq!(
            TitleId::of(Kind::Movie, None, Some(legacy)),
            Some(TitleId::Legacy(LegacyTitle { kind: Kind::Movie, guid: "com.plexapp.agents.imdb://tt0075686".into() })),
        );
        assert_eq!(TitleId::of(Kind::Show, None, Some("local://4242")), None);
        assert_eq!(TitleId::of(Kind::Show, None, None), None);
    }

    #[test]
    fn a_record_is_read_once_and_a_failed_read_is_not_stored() {
        let ids = RecordIds::default();
        assert_eq!(ids.legacy_forms(&HEX, || Err(WatchlistError::Transport)), Err(WatchlistError::Transport));
        let mut reads = 0;
        let mut read = || {
            reads += 1;
            Ok(vec!["imdb://tt4565380".to_string(), "tvdb://291866".to_string()])
        };
        let want = vec!["com.plexapp.agents.imdb://tt4565380".to_string(), "com.plexapp.agents.thetvdb://291866".to_string()];
        assert_eq!(ids.legacy_forms(&HEX, &mut read), Ok(want.clone()), "the failure was forgotten, so this reads");
        assert_eq!(ids.legacy_forms(&HEX, &mut read), Ok(want));
        assert_eq!(reads, 1, "the second ask is answered from memory");
    }

    #[test]
    fn a_record_the_catalog_does_not_know_has_no_legacy_forms() {
        let ids = RecordIds::default();
        assert_eq!(ids.legacy_forms(&HEX, || Err(WatchlistError::NotFound)), Ok(Vec::new()));
        assert_eq!(ids.legacy_forms(&HEX, || panic!("stored")), Ok(Vec::new()));
    }
}
