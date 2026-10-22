//! Application-owned data for the nav rail. Capture before stepping a frame; paint consumes
//! borrowed strings and never opens the session file or polls the Browse vocabulary.

use std::ffi::CString;

use plx_machine::machine::FocusKey;
use plx_ui::icons::Icon;
use plx_ui::nav_rail::{RailItem, RailMark, RailRole};
use plx_ui::widgets::{self, TopFocus};
use plx_data::browse::SourceState;
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

/// §7: whether a source's state draws dimmed — unreachable, access-refused, or reachable only
/// over a transport this build can't use. Not `NotProbed`: a source nobody has dialled yet must
/// not open dimmed (the same rule `SrcGroup::reachable` and `BrowseSource::reachable` already
/// apply to the Sources list and the Library page).
fn source_dim(directory: DirectoryView<'_>, src: usize) -> bool {
    directory.sources().get(src).is_some_and(|(_, group)|
        !matches!(group.state, SourceState::Reachable | SourceState::NotProbed))
}

/// The favorite libraries in `order`'s sequence, as `(section index, title, server name, dim)`.
/// `order` is `directory.favorite_order()` except while the rail's hold-to-reorder gesture (§3.5)
/// has a working order of its own not yet saved.
fn favorites<'a>(directory: DirectoryView<'a>, order: &[usize]) -> Vec<(usize, &'a str, &'a str, bool)> {
    order.iter().filter_map(|&i| {
        let section = directory.sections().get(i)?;
        Some((i, section.row.title.as_str(), server_name(directory, section.row.src),
            source_dim(directory, section.row.src)))
    }).collect()
}

/// A reorder of the favorites in progress (§3.5): which one is held and its favorites order as
/// arranged so far. Not yet saved — the live `directory.favorite_order()` is untouched until
/// [`ChromeSnapshot::drop_rail_move`], so leaving move mode any other way simply stops overriding
/// it, which is all "restore the order from before the hold" (Back) needs.
struct MoveState {
    section: usize,
    order: Vec<usize>,
    epoch: Option<u32>,
}

/// Where the rail's hold-to-reorder gesture (§3.5) stands. Owned here, not by the generic
/// press/focus engine: only the favorites band may hold, and `ElemKind` is one setting for the
/// whole rail strip (`containers::tabs::TabContainer::strip_group`) — see `bridge::rail_reorder`,
/// which drives this at the raw input level instead.
#[derive(Default)]
enum RailHold {
    #[default]
    Idle,
    /// OK is down on this favorite, not yet long enough to enter move mode.
    Armed { section: usize, down_at: u32 },
    Moving(MoveState),
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
    pill_in(i, &rail_keys(&rail_items(directory, "", "", false, directory.favorite_order())))
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
/// and More's list of every library, grouped by server, when `more_open`. `order` is the
/// favorites' order (see [`favorites`]).
fn rail_items(directory: DirectoryView<'_>, name: &str, initial: &str, more_open: bool, order: &[usize]) -> Vec<RailItem> {
    let fixed = |key, icon, word: &str, role| RailItem { key, mark: RailMark::Icon(icon), label: caps(word), caps: true, role, dim: false };
    let mut items = vec![
        fixed(widgets::TAB_KEY_HOME, Icon::Home, plx_platform::i18n::msg::browse_chrome_home(), RailRole::Top),
        fixed(widgets::TAB_KEY_SEARCH, Icon::Search, plx_platform::i18n::msg::browse_chrome_search(), RailRole::Top),
        fixed(widgets::TAB_KEY_WATCHLIST, Icon::Bookmark, plx_platform::i18n::msg::watchlist_page_title(), RailRole::Top),
    ];
    let libraries = favorites(directory, order);
    items.extend(libraries.iter().map(|&(i, title, server, dim)| {
        // Two favorites with one name are told apart by their server.
        let shared = libraries.iter().filter(|(_, other, _, _)| *other == title).count() > 1;
        let label = if shared && !server.is_empty() { format!("{title} · {server}") } else { title.to_owned() };
        RailItem { key: widgets::tab_key_library(i), mark: RailMark::Badge(first_letter(title)), label: caps(&label),
            caps: true, role: RailRole::Band, dim }
    }));
    items.push(fixed(widgets::TAB_KEY_MORE, Icon::More, plx_platform::i18n::msg::browse_action_more(), RailRole::Band));
    if more_open {
        for src in 0..directory.sources().len() {
            let mut rows = directory.sections().iter().enumerate().filter(|(_, section)| section.row.src == src).peekable();
            if rows.peek().is_none() { continue; }
            // §7: the group header names the fault (offline for Unreachable/InsecureOnly, not
            // shared for Unauthorized) and the whole group — header and rows alike — dims; a row
            // still opens its page (the per-library Failed read-out covers what OK on it does).
            let state = directory.sources().get(src).map_or(SourceState::NotProbed, |(_, group)| group.state);
            let dim = !matches!(state, SourceState::Reachable | SourceState::NotProbed);
            let server = server_name(directory, src);
            let label = match state {
                SourceState::Unauthorized => plx_platform::i18n::msg::browse_more_group_not_shared(server),
                SourceState::Unreachable | SourceState::InsecureOnly => plx_platform::i18n::msg::browse_more_group_offline(server),
                SourceState::Reachable | SourceState::NotProbed => server.to_owned(),
            };
            items.push(RailItem { key: widgets::tab_key_server(src), mark: RailMark::None,
                label: caps(&label), caps: true, role: RailRole::Header, dim });
            items.extend(rows.map(|(i, section)| RailItem {
                key: widgets::tab_key_more_row(i),
                mark: RailMark::Badge(first_letter(&section.row.title)),
                label: CString::new(section.row.title.as_str()).unwrap_or_default(),
                caps: false,
                role: RailRole::Row { star: widgets::tab_key_star(i), starred: section.row.pinned },
                dim,
            }));
        }
    }
    items.push(RailItem {
        key: widgets::TAB_KEY_CHIP,
        mark: RailMark::Avatar(CString::new(initial).unwrap_or_default()),
        label: CString::new(name).unwrap_or_default(),
        caps: false,
        role: RailRole::Foot,
        dim: false,
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
    /// The rail's hold-to-reorder gesture (§3.5): see [`RailHold`].
    hold: RailHold,
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

    /// Rebuild [`items`](Self::items) on the next [`refresh_with_profile`](Self::refresh_with_profile):
    /// the directory itself has not changed, but what the rail should show from it has.
    fn rebuild_on_next_refresh(&mut self) {
        self.directory_stamp = None;
        plx_machine::idle::invalidate();
    }

    /// The favorite the rail's hold-to-reorder gesture (§3.5) is armed or moving on, if any — the
    /// interceptor's own check that the hold is still on the favorite it began on.
    pub(crate) fn rail_hold_section(&self) -> Option<usize> {
        match &self.hold {
            RailHold::Idle => None,
            RailHold::Armed { section, .. } | RailHold::Moving(MoveState { section, .. }) => Some(*section),
        }
    }

    /// Move mode is live: Up/Down/OK/Back on the rail belong to the gesture, not to navigation.
    pub(crate) fn rail_moving(&self) -> bool {
        matches!(self.hold, RailHold::Moving(_))
    }

    /// OK went down on favorite `section`: start the hold's clock. A fresh press always re-arms,
    /// whatever the hold was doing before (the interceptor only calls this on an `Edge::Down`).
    pub(crate) fn arm_rail_hold(&mut self, section: usize, now: u32) {
        self.hold = RailHold::Armed { section, down_at: now };
    }

    /// The armed hold has crossed `press::LONG_MS` while OK is still physically down — the live
    /// test [`enter_rail_move`](Self::enter_rail_move) acts on, mirroring how a card's hold opens
    /// its item menu on the live `Press::is_long` rather than waiting for the release.
    pub(crate) fn rail_hold_long(&self, now: u32) -> bool {
        matches!(self.hold, RailHold::Armed { down_at, .. } if now.wrapping_sub(down_at) >= plx_ui::press::LONG_MS)
    }

    /// How long the current hold has run, for the hint's fill (§3.5.1) — `None` whenever OK is
    /// not down on a favorite (including while already `Moving`: the hint has nothing left to
    /// teach there).
    pub(crate) fn rail_held_ms(&self, now: u32) -> Option<u32> {
        match self.hold {
            RailHold::Armed { down_at, .. } => Some(now.wrapping_sub(down_at)),
            _ => None,
        }
    }

    /// Enter move mode for the currently-armed favorite: a working copy of the favorites order
    /// the rail now draws from instead of `directory.favorite_order()`, and the hint retired
    /// (§3.5.1) — independently of the card hint, see `plx_ui::hold_hint::mark_rail_taught`. A
    /// no-op off `Armed` (the interceptor calls this only once [`rail_hold_long`](Self::rail_hold_long) holds).
    pub(crate) fn enter_rail_move(&mut self, directory: DirectoryView<'_>) {
        let RailHold::Armed { section, .. } = self.hold else { return };
        self.hold = RailHold::Moving(MoveState {
            section, order: directory.favorite_order().to_vec(), epoch: directory.epoch(),
        });
        plx_ui::hold_hint::mark_rail_taught();
        self.rebuild_on_next_refresh();
    }

    /// The section table was rebuilt since move mode began, so the working order's indices may
    /// now name different libraries, or none; dropping it would save a wrong order.
    pub(crate) fn rail_move_outlived(&self, directory: DirectoryView<'_>) -> bool {
        matches!(&self.hold, RailHold::Moving(state) if state.epoch != directory.epoch())
    }

    /// Swap the moving favorite with its neighbour one slot (`-1` up, `1` down), clamped to the
    /// first and last favorite — it never moves past Watchlist or More, because it never leaves
    /// the favorites band at all. A no-op off `Moving`, or at an end already.
    pub(crate) fn step_rail_move(&mut self, delta: i32) {
        let RailHold::Moving(state) = &mut self.hold else { return };
        let Some(at) = state.order.iter().position(|&i| i == state.section) else { return };
        let Some(to) = at.checked_add_signed(delta as isize).filter(|&to| to < state.order.len()) else { return };
        state.order.swap(at, to);
        self.rebuild_on_next_refresh();
    }

    /// Back: leave move mode without saving. The live order was never touched (only the working
    /// copy was), so simply no longer overriding it IS "restore the order from before the hold" —
    /// also the stale-hold path, when focus has left the favorite the hold was armed on. A no-op
    /// already `Idle`.
    pub(crate) fn cancel_rail_hold(&mut self) {
        if matches!(self.hold, RailHold::Idle) { return; }
        self.hold = RailHold::Idle;
        self.rebuild_on_next_refresh();
    }

    /// OK while moving: take the final working order to persist, and leave move mode. `None` off
    /// `Moving`.
    pub(crate) fn drop_rail_move(&mut self) -> Option<Vec<usize>> {
        let RailHold::Moving(state) = std::mem::take(&mut self.hold) else { return None };
        self.rebuild_on_next_refresh();
        Some(state.order)
    }

    /// The rail key currently drawn with the move mark (§3.5), if any.
    pub(crate) fn moving_key(&self) -> Option<u32> {
        match &self.hold {
            RailHold::Moving(state) => Some(widgets::tab_key_library(state.section)),
            _ => None,
        }
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
            let order: &[usize] = match &self.hold {
                RailHold::Moving(state) => &state.order,
                _ => directory.favorite_order(),
            };
            self.items = rail_items(directory, &self.name, &self.initial, self.more_open, order);
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
        self.items = rail_items(directory.view(), name, initial, false, directory.view().favorite_order());
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
        rail_keys(&rail_items(directory.view(), "", "", more_open, directory.view().favorite_order()))
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
        let items = rail_items(directory.view(), "", "", true, directory.view().favorite_order());
        assert!(items.iter().any(|item| item.role == RailRole::Header), "a server header leads its rows");
        assert!(items.iter().any(|item| item.role == RailRole::Row { star: widgets::tab_key_star(1), starred: false }));
    }

    /// §7: a favorite on a source that is unreachable or has refused this account's access draws
    /// dimmed, and so does More's header for that server — named "offline" or "not shared" rather
    /// than its bare name. `NotProbed` (nobody has dialled it yet) must not dim: see
    /// [`source_dim`]'s own doc for why.
    #[test]
    fn an_unreachable_or_unauthorized_source_dims_its_favorite_and_more_group() {
        let sid = plx_plex::plex::ServerId::from_raw(0);
        let section = |title: &str| SectionView { sid: Some(sid), key: 1, kind: SecKind::Movie,
            row: SrcRow { section: 0, title: title.to_owned(), pinned: true, ..Default::default() } };
        for (state, suffix) in [
            (SourceState::Unreachable, "offline"),
            (SourceState::Unauthorized, "not shared"),
        ] {
            let server = plx_data::browse::SrcGroup { name: "Den".into(), state, ..Default::default() };
            let directory = DirectorySnapshot::fixture_with_sources(7, 0, vec![(sid, server)], vec![section("Films")]);
            let items = rail_items(directory.view(), "", "", true, directory.view().favorite_order());
            assert!(items[3].dim, "a favorite on a {state:?} source draws dimmed");
            let header = items.iter().find(|item| item.role == RailRole::Header).expect("a server header");
            assert!(header.dim, "a {state:?} server's More header draws dimmed");
            assert!(header.label.to_str().unwrap().contains(suffix), "{:?} should name '{suffix}'", header.label);
            let row = items.iter().find(|item| matches!(item.role, RailRole::Row { .. })).expect("the server's row");
            assert!(row.dim, "a {state:?} server's rows draw dimmed too");
        }
        // Reachable and NotProbed (the default, nobody has dialled yet) must not dim.
        for state in [SourceState::Reachable, SourceState::NotProbed] {
            let server = plx_data::browse::SrcGroup { name: "Den".into(), state, ..Default::default() };
            let directory = DirectorySnapshot::fixture_with_sources(7, 0, vec![(sid, server)], vec![section("Films")]);
            let items = rail_items(directory.view(), "", "", true, directory.view().favorite_order());
            assert!(!items[3].dim, "a {state:?} source's favorite must not dim");
            let header = items.iter().find(|item| item.role == RailRole::Header).unwrap();
            assert!(!header.dim);
            assert_eq!(header.label.to_str().unwrap(), "DEN");
        }
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
        let items = rail_items(directory.view(), "Sam", "S", false, directory.view().favorite_order());
        assert_eq!(items.len(), 7);
        assert!(items[3].label.to_str().unwrap().starts_with("MOVIES"));
        assert!(matches!(items[6].mark, RailMark::Avatar(_)), "the profile is the last item");
    }

    /// §3.5: a hold that crosses `press::LONG_MS` while OK is still down enters move mode live,
    /// and Up/Down swap the held favorite with its neighbour, clamped at the first and last
    /// favorite — it can never cross into Watchlist or More, because it never leaves the
    /// favorites band's own order at all.
    #[test]
    fn a_long_enough_hold_enters_move_mode_and_up_down_clamp_at_both_ends() {
        let directory = directory(&[(SecKind::Movie, "A", true), (SecKind::Movie, "B", true),
            (SecKind::Movie, "C", true)]);
        let mut chrome = ChromeSnapshot::default();
        chrome.arm_rail_hold(1, 1_000); // B, the middle favorite
        assert!(!chrome.rail_hold_long(1_000 + plx_ui::press::LONG_MS - 1), "not yet");
        assert!(chrome.rail_hold_long(1_000 + plx_ui::press::LONG_MS), "the live threshold");
        chrome.enter_rail_move(directory.view());
        assert!(chrome.rail_moving());
        assert_eq!(chrome.moving_key(), Some(widgets::tab_key_library(1)));

        chrome.step_rail_move(-1); // B above A: [B, A, C]
        chrome.step_rail_move(-1); // already first: clamped, a no-op
        let order = chrome.drop_rail_move().expect("was moving");
        assert_eq!(order, vec![1, 0, 2], "clamped at the first favorite");
        assert!(!chrome.rail_moving(), "OK dropped it");

        chrome.arm_rail_hold(0, 2_000);
        chrome.enter_rail_move(directory.view());
        chrome.step_rail_move(1);
        chrome.step_rail_move(1); // already last: clamped, a no-op
        assert_eq!(chrome.drop_rail_move().unwrap(), vec![1, 2, 0], "clamped at the last favorite");
    }

    /// Back cancels the hold and restores the order from before it: the working copy is simply
    /// dropped without ever touching the real `favorite_order`, which a cancelled hold never
    /// wrote to in the first place.
    #[test]
    fn back_cancels_the_hold_without_saving_the_working_order() {
        let directory = directory(&[(SecKind::Movie, "A", true), (SecKind::Movie, "B", true)]);
        let mut chrome = ChromeSnapshot::default();
        chrome.arm_rail_hold(0, 1_000);
        chrome.enter_rail_move(directory.view());
        chrome.step_rail_move(1); // [B, A]
        assert!(chrome.rail_moving());

        chrome.cancel_rail_hold();
        assert!(!chrome.rail_moving(), "left move mode");
        assert_eq!(chrome.moving_key(), None);
        assert_eq!(chrome.drop_rail_move(), None, "nothing left to drop — cancel, not save");

        // A hold armed but never long enough to move is just as cleanly abandoned.
        chrome.arm_rail_hold(1, 2_000);
        chrome.cancel_rail_hold();
        assert_eq!(chrome.rail_hold_section(), None);
    }

    #[test]
    fn a_move_is_outlived_by_a_rebuilt_section_table() {
        let directory = directory(&[(SecKind::Movie, "A", true), (SecKind::Movie, "B", true)]);
        let rebuilt = DirectorySnapshot::fixture_with_sources(8, 0, Vec::new(), Vec::new());
        let mut chrome = ChromeSnapshot::default();
        chrome.arm_rail_hold(0, 1_000);
        chrome.enter_rail_move(directory.view());

        assert!(!chrome.rail_move_outlived(directory.view()));
        assert!(chrome.rail_move_outlived(rebuilt.view()));
    }

    #[test]
    fn chrome_production_reads_only_its_retained_directory() {
        let src = include_str!("chrome.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("plx_data::browse::"),
            "Chrome must read only its retained Browse directory");
    }
}
