//! Application-owned data for the nav rail. Capture before stepping a frame; paint consumes
//! borrowed strings and never opens the session file or polls the Browse vocabulary.

use std::ffi::CString;

use plx_machine::machine::FocusKey;
use plx_ui::icons::Icon;
use plx_ui::nav_rail::{RailItem, RailMark};
use plx_ui::widgets::{self, TopFocus};
use plx_data::stores::browse::DirectoryView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pill {
    Home,
    /// A favorite library, by its index in the directory's section table.
    Library(usize),
    Watchlist,
    Search,
}

/// The favorite libraries in discovery order, as `(section index, title, server name)`.
fn favorites(directory: DirectoryView<'_>) -> impl Iterator<Item = (usize, &str, &str)> {
    directory.sections().iter().enumerate().filter(|(_, section)| section.row.pinned).map(move |(i, section)| {
        let server = directory.sources().get(section.row.src).map_or("", |(_, group)| group.name.as_str());
        (i, section.row.title.as_str(), server)
    })
}

/// The rail's destination keys in rail order: Home, Search, Watchlist, one key per favorite
/// library, then the profile. The one place that order is written.
fn rail_keys(directory: DirectoryView<'_>) -> Vec<u32> {
    let mut keys = vec![widgets::TAB_KEY_HOME, widgets::TAB_KEY_SEARCH, widgets::TAB_KEY_WATCHLIST];
    keys.extend(favorites(directory).map(|(i, _, _)| widgets::tab_key_library(i)));
    keys.push(widgets::TAB_KEY_CHIP);
    keys
}

pub(crate) fn pill_at(directory: DirectoryView<'_>, i: usize) -> Pill {
    pill_in(i, &rail_keys(directory))
}

/// The destination a rail index names, by the KEY published at that index.
fn pill_in(i: usize, keys: &[u32]) -> Pill {
    match keys.get(i).copied() {
        Some(widgets::TAB_KEY_WATCHLIST) => Pill::Watchlist,
        Some(widgets::TAB_KEY_SEARCH) => Pill::Search,
        Some(key) => widgets::library_of_tab_key(key).map_or(Pill::Home, Pill::Library),
        None => Pill::Home,
    }
}

pub(crate) fn pill_of(directory: DirectoryView<'_>, pill: Pill) -> Option<usize> {
    pill_index(pill, &rail_keys(directory))
}

fn pill_index(pill: Pill, keys: &[u32]) -> Option<usize> {
    let key = match pill {
        Pill::Home => widgets::TAB_KEY_HOME,
        Pill::Library(i) => widgets::tab_key_library(i),
        Pill::Watchlist => widgets::TAB_KEY_WATCHLIST,
        Pill::Search => widgets::TAB_KEY_SEARCH,
    };
    keys.iter().position(|&k| k == key)
}

fn caps(word: &str) -> CString {
    CString::new(word.to_uppercase()).unwrap_or_default()
}

fn first_letter(word: &str) -> CString {
    CString::new(word.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()).unwrap_or_default()
}

/// The rail's items for `directory`, with the profile item named `name` and drawn with `initial`.
fn rail_items(directory: DirectoryView<'_>, name: &str, initial: &str) -> Vec<RailItem> {
    let fixed = |key, icon, word: &str| RailItem { key, mark: RailMark::Icon(icon), label: caps(word), caps: true };
    let mut items = vec![
        fixed(widgets::TAB_KEY_HOME, Icon::Home, plx_platform::i18n::msg::browse_chrome_home()),
        fixed(widgets::TAB_KEY_SEARCH, Icon::Search, plx_platform::i18n::msg::browse_chrome_search()),
        fixed(widgets::TAB_KEY_WATCHLIST, Icon::Bookmark, plx_platform::i18n::msg::watchlist_page_title()),
    ];
    let libraries: Vec<_> = favorites(directory).collect();
    items.extend(libraries.iter().map(|&(i, title, server)| {
        // Two favorites with one name are told apart by their server.
        let shared = libraries.iter().filter(|(_, other, _)| *other == title).count() > 1;
        let label = if shared && !server.is_empty() { format!("{title} · {server}") } else { title.to_owned() };
        RailItem { key: widgets::tab_key_library(i), mark: RailMark::Badge(first_letter(title)), label: caps(&label), caps: true }
    }));
    items.push(RailItem {
        key: widgets::TAB_KEY_CHIP,
        mark: RailMark::Avatar(CString::new(initial).unwrap_or_default()),
        label: CString::new(name).unwrap_or_default(),
        caps: false,
    });
    items
}

#[derive(Default)]
pub(crate) struct ChromeSnapshot {
    directory_stamp: Option<(u32, u32, Option<u32>)>,
    profile_generation: Option<u32>,
    session_watch: plx_plex::plex::session::VisibleSessionWatch,
    name: String,
    initial: String,
    pub(crate) keys: Vec<u32>,
    items: Vec<RailItem>,
    /// A pulse asked for and not yet handed to the rail's renderer: the destination key.
    pulse: Option<u32>,
}

impl ChromeSnapshot {
    /// **Pulse the rail destination `key`** (~1.35x scale and a glow, decorative): the Watchlist
    /// mark after a title was put on the list. The APPLICATION asks, never a screen; the bridge
    /// hands it to the rail's renderer on its next chrome update. A frame is requested so an
    /// idle app wakes for it.
    pub(crate) fn pulse(&mut self, key: u32) {
        self.pulse = Some(key);
        plx_machine::idle::invalidate();
    }

    /// The pending pulse's key, when its destination is on the rail. Consumes it.
    pub(crate) fn take_pulse(&mut self) -> Option<u32> {
        self.pulse.take().filter(|key| self.keys.contains(key))
    }

    /// Forget a pending pulse: the rail is not on screen, so it would play late on the next page.
    pub(crate) fn drop_pulse(&mut self) {
        self.pulse = None;
    }

    pub(crate) fn refresh(&mut self, directory: DirectoryView<'_>) {
        self.refresh_with_profile(directory, None);
    }

    pub(crate) fn refresh_with_profile(&mut self, directory: DirectoryView<'_>,
        captured: Option<(&plx_plex::plex::session::CurrentProfile, &plx_plex::plex::session::Session)>) {
        let mut changed = false;
        let generation = captured.map_or_else(plx_plex::plex::session::current_gen, |(profile, _)| profile.generation);
        let session_changed = captured.is_none() && self.session_watch.changed();
        if self.profile_generation != Some(generation) || session_changed {
            let current = captured.map_or_else(plx_plex::plex::session::current, |(profile, _)| profile.user.clone());
            let account = if let Some((_, saved)) = captured { saved.account(current.as_ref()) }
                else { plx_plex::plex::session::peek().account(current.as_ref()) };
            self.name = plx_screens::account_menu::chip_label(&account);
            self.initial = account.name.as_deref().and_then(|name| name.chars().next())
                .map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            self.profile_generation = Some(generation);
            changed = true;
        }
        let stamp = (directory.source_list_gen(), directory.tabs_gen(), directory.epoch());
        if self.directory_stamp != Some(stamp) {
            self.directory_stamp = Some(stamp);
            changed = true;
        }
        if changed {
            self.keys = rail_keys(directory);
            self.items = rail_items(directory, &self.name, &self.initial);
        }
    }

    pub(crate) fn items(&self) -> &[RailItem] {
        &self.items
    }

    /// The rail key published at `selected` (an index into [`Self::keys`]): the page's own item,
    /// for the landing rule `TabContainer::strip_selected` states.
    pub(crate) fn key_at(&self, selected: i32) -> Option<u32> {
        self.keys.get(selected.max(0) as usize).copied()
    }

    pub(crate) fn focus(&self, focus: Option<FocusKey<u32>>) -> TopFocus {
        match focus.map(|focus| focus.elem) {
            Some(key) if key == widgets::TAB_KEY_CHIP => TopFocus::Chip,
            Some(key) => self.keys.iter().position(|&id| id == key).map(TopFocus::Pill).unwrap_or(TopFocus::Away),
            None => TopFocus::Away,
        }
    }

    /// The rail key focus is on, if it is on the rail.
    pub(crate) fn focused_key(&self, focus: Option<FocusKey<u32>>) -> Option<u32> {
        focus.map(|focus| focus.elem).filter(|key| self.keys.contains(key))
    }

    #[cfg(test)]
    pub(crate) fn seed_for_test(&mut self, name: &str, initial: &str) {
        let directory = plx_data::stores::browse::DirectorySnapshot::fixture(1, 0, Vec::new());
        self.name = name.to_owned();
        self.initial = initial.to_owned();
        self.profile_generation = Some(1);
        self.keys = rail_keys(directory.view());
        self.items = rail_items(directory.view(), name, initial);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_data::stores::browse::{DirectorySnapshot, SecKind, SectionView, SrcRow};

    fn directory(pinned: &[(SecKind, &str, bool)]) -> DirectorySnapshot {
        DirectorySnapshot::fixture(7, 0, pinned.iter().enumerate().map(|(i, &(kind, title, pinned))| SectionView {
            sid: Some(plx_plex::plex::ServerId::from_raw(0)),
            key: i as i64 + 1,
            kind,
            row: SrcRow { section: i, title: title.to_owned(), pinned, ..Default::default() },
        }).collect())
    }

    #[test]
    fn session_refresh_renames_the_profile_item_without_a_profile_switch() {
        let _serial = plx_base::testlock::serial();
        let _session = plx_plex::plex::session::TempSession::new("chrome-session-refresh");
        let mut snapshot = ChromeSnapshot::default();
        let directory = directory(&[]);
        plx_plex::plex::session::install_transient_for_test(true);
        snapshot.refresh(directory.view());
        assert_eq!(snapshot.name, "Sign in");
        plx_plex::plex::session::save(&plx_plex::plex::session::Session {
            client_id: "synthetic-client".into(), account_token: "synthetic-token".into(),
            home_users: vec![plx_plex::plex::session::HomeUserRef {
                title: "Synthetic owner".into(), admin: true, ..Default::default()
            }], ..Default::default()
        });
        snapshot.refresh(directory.view());
        assert_eq!(snapshot.name, "Synthetic owner");
        assert_eq!(snapshot.items().last().map(|item| item.label.to_str().unwrap()), Some("Synthetic owner"));
    }

    #[test]
    fn rail_order_is_home_search_watchlist_favorites_then_profile() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Anime", false),
            (SecKind::Show, "Series", true)]);
        assert_eq!(rail_keys(directory.view()), vec![widgets::TAB_KEY_HOME, widgets::TAB_KEY_SEARCH,
            widgets::TAB_KEY_WATCHLIST, widgets::tab_key_library(0), widgets::tab_key_library(2), widgets::TAB_KEY_CHIP]);
    }

    #[test]
    fn every_rail_destination_round_trips_by_key() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Series", true)]);
        let keys = rail_keys(directory.view());
        for pill in [Pill::Home, Pill::Search, Pill::Watchlist, Pill::Library(0), Pill::Library(1)] {
            let i = pill_index(pill, &keys).expect("on the rail");
            assert_eq!(pill_in(i, &keys), pill);
        }
        assert_eq!(pill_index(Pill::Library(5), &keys), None, "an unpinned library borrows no position");
    }

    #[test]
    fn same_named_favorites_carry_their_server() {
        let directory = directory(&[(SecKind::Movie, "Movies", true), (SecKind::Movie, "Movies", true)]);
        let items = rail_items(directory.view(), "Sam", "S");
        assert_eq!(items.len(), 6);
        assert!(items[3].label.to_str().unwrap().starts_with("MOVIES"));
        assert!(matches!(items[5].mark, RailMark::Avatar(_)), "the profile is the last item");
    }

    #[test]
    fn chrome_production_reads_only_its_retained_directory() {
        let src = include_str!("chrome.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("plx_data::browse::"),
            "Chrome must read only its retained Browse directory");
    }
}
