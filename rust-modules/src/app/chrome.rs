//! Application-owned data for the nav rail. Capture before stepping a frame; paint consumes
//! borrowed strings and never opens the session file or polls the Browse vocabulary.

use std::ffi::CString;

use plx_machine::machine::FocusKey;
use plx_ui::icons::Icon;
use plx_ui::nav_rail::{RailItem, RailMark, RailRole};
use plx_ui::widgets::{self, TopFocus};
use plx_data::stores::browse::DirectoryView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pill {
    Home,
    /// A library, by its index in the directory's section table.
    Library(usize),
    Watchlist,
    Search,
}

fn server_name(directory: DirectoryView<'_>, src: usize) -> &str {
    directory.sources().get(src).map_or("", |(_, group)| group.name.as_str())
}

/// The favorite libraries in the profile's rail order, as `(section index, title, server name)`.
fn favorites(directory: DirectoryView<'_>) -> impl Iterator<Item = (usize, &str, &str)> {
    directory.favorite_order().iter().filter_map(move |&i| {
        let section = directory.sections().get(i)?;
        Some((i, section.row.title.as_str(), server_name(directory, section.row.src)))
    })
}

/// The focus keys the rail publishes, in rail order: every item but the headers, each More row
/// followed by its ★. The one place that order is written.
fn rail_keys(items: &[RailItem]) -> Vec<u32> {
    let mut keys = Vec::with_capacity(items.len());
    for item in items {
        match item.role {
            RailRole::Header => {}
            RailRole::Row { star, .. } => keys.extend([item.key, star]),
            _ => keys.push(item.key),
        }
    }
    keys
}

pub(crate) fn pill_at(directory: DirectoryView<'_>, i: usize) -> Pill {
    pill_in(i, &rail_keys(&rail_items(directory, "", "", false)))
}

/// The destination a rail index names, by the KEY published at that index.
fn pill_in(i: usize, keys: &[u32]) -> Pill {
    match keys.get(i).copied() {
        Some(widgets::TAB_KEY_WATCHLIST) => Pill::Watchlist,
        Some(widgets::TAB_KEY_SEARCH) => Pill::Search,
        Some(key) => widgets::library_of_tab_key(key).or_else(|| widgets::more_row_of_tab_key(key))
            .map_or(Pill::Home, Pill::Library),
        None => Pill::Home,
    }
}

/// A library's place on the rail: its favorite item, else its row in More's list, else More.
fn pill_index(pill: Pill, keys: &[u32]) -> Option<usize> {
    let at = |key: u32| keys.iter().position(|&k| k == key);
    match pill {
        Pill::Home => at(widgets::TAB_KEY_HOME),
        Pill::Library(i) => at(widgets::tab_key_library(i))
            .or_else(|| at(widgets::tab_key_more_row(i)))
            .or_else(|| at(widgets::TAB_KEY_MORE)),
        Pill::Watchlist => at(widgets::TAB_KEY_WATCHLIST),
        Pill::Search => at(widgets::TAB_KEY_SEARCH),
    }
}

fn caps(word: &str) -> CString {
    CString::new(word.to_uppercase()).unwrap_or_default()
}

fn first_letter(word: &str) -> CString {
    CString::new(word.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()).unwrap_or_default()
}

/// The rail's items for `directory`, with the profile item named `name` and drawn with `initial`,
/// and More's list of every library, grouped by server, when `more_open`.
fn rail_items(directory: DirectoryView<'_>, name: &str, initial: &str, more_open: bool) -> Vec<RailItem> {
    let fixed = |key, icon, word: &str, role| RailItem { key, mark: RailMark::Icon(icon), label: caps(word), caps: true, role };
    let mut items = vec![
        fixed(widgets::TAB_KEY_HOME, Icon::Home, plx_platform::i18n::msg::browse_chrome_home(), RailRole::Top),
        fixed(widgets::TAB_KEY_SEARCH, Icon::Search, plx_platform::i18n::msg::browse_chrome_search(), RailRole::Top),
        fixed(widgets::TAB_KEY_WATCHLIST, Icon::Bookmark, plx_platform::i18n::msg::watchlist_page_title(), RailRole::Top),
    ];
    let libraries: Vec<_> = favorites(directory).collect();
    items.extend(libraries.iter().map(|&(i, title, server)| {
        // Two favorites with one name are told apart by their server.
        let shared = libraries.iter().filter(|(_, other, _)| *other == title).count() > 1;
        let label = if shared && !server.is_empty() { format!("{title} · {server}") } else { title.to_owned() };
        RailItem { key: widgets::tab_key_library(i), mark: RailMark::Badge(first_letter(title)), label: caps(&label),
            caps: true, role: RailRole::Band }
    }));
    items.push(fixed(widgets::TAB_KEY_MORE, Icon::More, plx_platform::i18n::msg::browse_action_more(), RailRole::Band));
    if more_open {
        for src in 0..directory.sources().len() {
            let mut rows = directory.sections().iter().enumerate().filter(|(_, section)| section.row.src == src).peekable();
            if rows.peek().is_none() { continue; }
            items.push(RailItem { key: widgets::tab_key_server(src), mark: RailMark::None,
                label: caps(server_name(directory, src)), caps: true, role: RailRole::Header });
            items.extend(rows.map(|(i, section)| RailItem {
                key: widgets::tab_key_more_row(i),
                mark: RailMark::Badge(first_letter(&section.row.title)),
                label: CString::new(section.row.title.as_str()).unwrap_or_default(),
                caps: false,
                role: RailRole::Row { star: widgets::tab_key_star(i), starred: section.row.pinned },
            }));
        }
    }
    items.push(RailItem {
        key: widgets::TAB_KEY_CHIP,
        mark: RailMark::Avatar(CString::new(initial).unwrap_or_default()),
        label: CString::new(name).unwrap_or_default(),
        caps: false,
        role: RailRole::Foot,
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
    /// More's list is open in place.
    more_open: bool,
    /// The non-favorite library More was last opened for, so it opens once per arrival.
    opened_for: Option<usize>,
}

impl ChromeSnapshot {
    pub(crate) fn more_open(&self) -> bool {
        self.more_open
    }

    pub(crate) fn set_more_open(&mut self, open: bool) {
        if self.more_open != open {
            self.more_open = open;
            // Rebuild the items on the next refresh.
            self.directory_stamp = None;
            plx_machine::idle::invalidate();
        }
    }

    /// The page on view shows `section`, a library that is not a favorite (or `None`): More opens
    /// on arrival, so the library's rail spot, its row, is there to land on.
    pub(crate) fn follow_library(&mut self, section: Option<usize>) {
        if section.is_some() && section != self.opened_for { self.set_more_open(true); }
        self.opened_for = section;
    }

    /// The rail index of `pill`'s item, as published now.
    pub(crate) fn pill_of(&self, pill: Pill) -> Option<usize> {
        pill_index(pill, &self.keys)
    }

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
            self.items = rail_items(directory, &self.name, &self.initial, self.more_open);
            self.keys = rail_keys(&self.items);
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
        self.items = rail_items(directory.view(), name, initial, false);
        self.keys = rail_keys(&self.items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_data::stores::browse::{DirectorySnapshot, SecKind, SectionView, SrcRow};

    fn directory(pinned: &[(SecKind, &str, bool)]) -> DirectorySnapshot {
        let sid = plx_plex::plex::ServerId::from_raw(0);
        let server = plx_data::browse::SrcGroup { name: "Den".into(), ..Default::default() };
        DirectorySnapshot::fixture_with_sources(7, 0, vec![(sid, server)],
            pinned.iter().enumerate().map(|(i, &(kind, title, pinned))| SectionView {
                sid: Some(sid),
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

    fn keys(directory: &DirectorySnapshot, more_open: bool) -> Vec<u32> {
        rail_keys(&rail_items(directory.view(), "", "", more_open))
    }

    #[test]
    fn rail_order_is_home_search_watchlist_favorites_more_then_profile() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Anime", false),
            (SecKind::Show, "Series", true)]);
        assert_eq!(keys(&directory, false), vec![widgets::TAB_KEY_HOME, widgets::TAB_KEY_SEARCH,
            widgets::TAB_KEY_WATCHLIST, widgets::tab_key_library(0), widgets::tab_key_library(2),
            widgets::TAB_KEY_MORE, widgets::TAB_KEY_CHIP]);
    }

    #[test]
    fn favorites_follow_the_published_order() {
        let sid = plx_plex::plex::ServerId::from_raw(0);
        let section = |i: usize, title: &str| SectionView { sid: Some(sid), key: i as i64 + 1, kind: SecKind::Movie,
            row: SrcRow { section: i, title: title.to_owned(), pinned: true, ..Default::default() } };
        let mut directory = DirectorySnapshot::fixture(7, 0, vec![section(0, "A"), section(1, "B")]);
        directory.set_favorite_order_for_test(vec![1, 0]);
        assert_eq!(keys(&directory, false)[3..5], [widgets::tab_key_library(1), widgets::tab_key_library(0)]);
    }

    #[test]
    fn more_lists_every_library_with_a_star_after_its_row() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Anime", false)]);
        let keys = keys(&directory, true);
        let more = keys.iter().position(|&k| k == widgets::TAB_KEY_MORE).unwrap();
        assert_eq!(keys[more + 1..keys.len() - 1], [widgets::tab_key_more_row(0), widgets::tab_key_star(0),
            widgets::tab_key_more_row(1), widgets::tab_key_star(1)], "rows of favorites and the rest alike");
        let items = rail_items(directory.view(), "", "", true);
        assert!(items.iter().any(|item| item.role == RailRole::Header), "a server header leads its rows");
        assert!(items.iter().any(|item| item.role == RailRole::Row { star: widgets::tab_key_star(1), starred: false }));
    }

    #[test]
    fn a_library_sits_on_its_favorite_item_else_its_more_row_else_more() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Anime", false)]);
        let closed = keys(&directory, false);
        let open = keys(&directory, true);
        assert_eq!(closed[pill_index(Pill::Library(0), &closed).unwrap()], widgets::tab_key_library(0));
        assert_eq!(closed[pill_index(Pill::Library(1), &closed).unwrap()], widgets::TAB_KEY_MORE);
        assert_eq!(open[pill_index(Pill::Library(1), &open).unwrap()], widgets::tab_key_more_row(1));
    }

    #[test]
    fn every_rail_destination_round_trips_by_key() {
        let directory = directory(&[(SecKind::Movie, "Films", true), (SecKind::Show, "Series", true)]);
        let keys = keys(&directory, false);
        for pill in [Pill::Home, Pill::Search, Pill::Watchlist, Pill::Library(0), Pill::Library(1)] {
            let i = pill_index(pill, &keys).expect("on the rail");
            assert_eq!(pill_in(i, &keys), pill);
        }
    }

    #[test]
    fn same_named_favorites_carry_their_server() {
        let directory = directory(&[(SecKind::Movie, "Movies", true), (SecKind::Movie, "Movies", true)]);
        let items = rail_items(directory.view(), "Sam", "S", false);
        assert_eq!(items.len(), 7);
        assert!(items[3].label.to_str().unwrap().starts_with("MOVIES"));
        assert!(matches!(items[6].mark, RailMark::Avatar(_)), "the profile is the last item");
    }

    #[test]
    fn chrome_production_reads_only_its_retained_directory() {
        let src = include_str!("chrome.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("plx_data::browse::"),
            "Chrome must read only its retained Browse directory");
    }
}
