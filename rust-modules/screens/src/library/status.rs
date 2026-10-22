//! Retained-view prose for the existing Library status surface.
use std::ffi::{CStr, CString};
use super::*;
use plx_data::browse::SourceState;
use plx_ui::icons::Icon;
use plx_ui::widgets::{StatusKind, StatusOverlay};

impl LibraryScreen {
    /// The failed source's dial outcome, or [`SourceState::NotProbed`] off a readout that isn't
    /// [`Readout::Failed`] — the one place both [`Self::status_overlay`] and [`Self::status_text`]
    /// read it, so the glyph, the caption and the action button can never disagree about which
    /// case they're in.
    pub(super) fn failed_source_state<H: LibraryLike>(&self, cx: &Cx<'_, H>) -> SourceState {
        if self.readout != Readout::Failed { return SourceState::default(); }
        H::directory(cx).source().map_or(SourceState::default(), |(_, source)| source.state)
    }

    pub(super) fn status_overlay<'a, H: LibraryLike>(&self, cx: &Cx<'_, H>, caption: &'a CStr, reason: Option<&'a CStr>) -> StatusOverlay<'a> {
        let kind = match self.readout {
            Readout::Failed => StatusKind::Failed, Readout::Loading => StatusKind::Working,
            Readout::Empty | Readout::Grid => StatusKind::Empty,
        };
        // A failed source fills the page under the live chrome, so it stands on the shared page
        // lines (`StatusOverlay::page`) — level with Home's and the sign-in failure's — rather than
        // centring in the content region, which dropped it well below them. Loading and the
        // empty answer keep the region.
        // §7's one typed cause this page draws differently: Unauthorized (access refused) gets
        // the lock glyph; every other Failed cause (unreachable, not yet probed, insecure-only)
        // reads as the generic "can't reach" case and keeps the wifi-slash glyph.
        let unauthorized = self.failed_source_state(cx) == SourceState::Unauthorized;
        // Unauthorized is checked ahead of the clock cause below for the same reason §7's caption
        // is: a 401 already proves the connection succeeded, so the two can never really compete,
        // but keeping one priority order for both avoids ever drawing a mismatched pair.
        let glyph = if unauthorized {
            Icon::LockBadgeAlert
        } else {
            self.clock_cause().map_or(Icon::WifiSlash, |(_, glyph)| glyph)
        };
        let mut overlay = StatusOverlay::new(self.status_frame(), caption, kind)
            .page(glyph).phase(cx.tick.ms)
            .focused(cx.focus.current == Some(self.key(RETRY)));
        // The tab strip (`draw_library_controls`) stays live above a failed section's read-out —
        // a section failing is not the app failing — so the glyph is told where that chrome's
        // bottom edge is, and would shrink to clear it if the strip ever reached the natural box
        // (`glyph_ceiling`'s own doc). At the current anchor it never does; the ceiling is a guard. `self.libraries` empty is exactly the condition `draw_library_controls` itself
        // uses to skip drawing the strip at all.
        if !self.libraries.is_empty() {
            overlay = overlay.glyph_ceiling(CONTENT_TOP + StatusOverlay::CTRL_H);
        }
        if let Some(reason) = reason { overlay = overlay.reason(reason); }
        if self.readout == Readout::Failed {
            // Access refused (§7) re-checks the account's shares, a different fix than retrying a
            // server that already answered; everything else keeps the existing retry/connect pill.
            overlay = overlay.action(if unauthorized {
                plx_platform::i18n::msg::browse_action_check_shares_c()
            } else {
                super::super::plaintext_question::primary(self.plaintext.verdict())
            });
        }
        overlay
    }

    /// **The clock reason and glyph this read-out shows, if any** — the ONE rule, read by the text
    /// and the glyph alike: only a Failed read-out, and only when no plaintext offer holds the
    /// reason slot (that cause is the one the person can act on). From the held
    /// [`ClockWatch`](super::super::clock_readout::ClockWatch), so a frame's draw and hit rect agree.
    fn clock_cause(&self) -> Option<(&'static CStr, plx_ui::icons::Icon)> {
        if self.readout != Readout::Failed || self.plaintext.verdict().is_some() { return None; }
        self.clock.reason()
    }

    /// Follow the offer for the failed source's server (`plex::grant::offers`) and the clock fact
    /// about that same server (`net::keypin::blocked_for`), and take the question down once the
    /// read-out it was asked from no longer asks about that server. `true` when what the read-out
    /// shows changed, so the caller damages the frame (the reason line moves the action row).
    pub(super) fn watch_readout<H: LibraryLike>(&mut self, cx: &Cx<'_, H>) -> bool {
        use super::super::plaintext_question::{asks, Near};
        let machine = (self.readout == Readout::Failed)
            .then(|| H::directory(cx).source().and_then(|(sid, _)| plx_plex::plex::client_for(*sid)))
            .flatten()
            .map(|client| client.machine_id());
        let offer_moved = self.plaintext.refresh(machine, Near::Only);
        let clock_moved = self.clock.refresh(machine);
        if self.plaintext_alert.is_open()
            && !(self.readout == Readout::Failed
                && self.plaintext_alert.subject() == self.plaintext.verdict().map(|v| v.machine_id.as_str())
                && asks(self.plaintext.verdict()))
        {
            self.plaintext_alert.withdraw();
        }
        offer_moved || clock_moved
    }

    pub(super) fn status_rect<H: LibraryLike>(&self, cx: &Cx<'_, H>) -> Option<Rect> {
        if self.readout != Readout::Failed { return None; }
        let (caption, reason) = self.status_text(cx);
        self.status_overlay(cx, &caption, reason.as_deref()).action_frame_measured(cx.measure)
    }

    pub(super) fn status_text<H: LibraryLike>(&self, cx: &Cx<'_, H>) -> (CString, Option<CString>) {
        let directory = H::directory(cx);
        let listing = H::listing(cx);
        let (caption, reason) = match self.readout {
            Readout::Failed => {
                let source = directory.source().map(|(_, source)| source);
                let name = source.map(|source| source.name.as_str()).filter(|name| !name.is_empty()).unwrap_or(plx_platform::i18n::msg::browse_library_server());
                let owner = source.map(|source| source.handle.as_str()).filter(|owner| !owner.is_empty());
                // Access refused (§7): the server answered with a 401, a fact independent of the
                // plaintext/clock causes below (both are about reaching the server at all, which
                // a 401 already did) — so it is checked first and the two never compete.
                if source.is_some_and(|s| s.state == SourceState::Unauthorized) {
                    return (
                        CString::new(plx_platform::i18n::msg::browse_library_not_shared(name)).unwrap_or_default(),
                        Some(CString::new(plx_platform::i18n::msg::browse_library_access_refused(name)).unwrap_or_default()),
                    );
                }
                // Your own server is "your Plex server", the words Home uses for the same fault;
                // a borrowed one is named, since "your" would be untrue of it.
                let caption = match owner {
                    None => plx_platform::i18n::msg::browse_home_failed().to_string(),
                    Some(_) => plx_platform::i18n::msg::browse_library_unreachable(name),
                };
                // A server discovery offers "Connect without encryption?" for says why instead,
                // and names what *Connect* / *Try again* does (`auth::plaintext_copy`).
                let reason = match self.plaintext.verdict() {
                    Some(verdict) => Some(plx_session::auth::plaintext_copy(Some(verdict),
                        plx_session::auth::ReadoutSurface::SignedIn).into_owned()),
                    // else a wrong clock, then who shares the server.
                    None => match self.clock_cause() {
                        Some((reason, _)) => Some(reason.to_string_lossy().into_owned()),
                        None => owner.map(|owner| plx_platform::i18n::msg::browse_library_shared_unreachable(owner)),
                    },
                };
                (caption, reason)
            }
            Readout::Empty => {
                let caption = if self.wanted_kind.is_some() { plx_platform::i18n::msg::browse_library_no_matches().into() }
                    else if directory.sections().is_empty() { plx_platform::i18n::msg::browse_library_empty().into() }
                    else if listing.unwatched() || listing.genre().is_some() { plx_platform::i18n::msg::browse_library_no_matches().into() }
                    else if let Some(section) = directory.current().and_then(|i| directory.sections().get(i)) {
                        if self.listing == Listing::Collections { super::collections_page::empty_text(&section.row.title) }
                        else { listing.library_type().empty_readout(section.kind, &section.row.title) }
                    } else { plx_platform::i18n::msg::browse_library_no_matches().into() };
                (caption, None)
            }
            // placeholder-exempt: builds the caption only; a Loading readout draws as the Library spinner (draw_page, LibrarySpinner)
            Readout::Loading => (plx_platform::i18n::msg::browse_library_loading().into(), None),
            Readout::Grid => (String::new(), None),
        };
        (CString::new(caption).unwrap_or_default(), reason.map(|reason| CString::new(reason).unwrap_or_default()))
    }

    pub(super) fn status_frame(&self) -> Rect {
        // The readout occupies the fixed content region, inside the overscan frame.
        const STATUS_TOP: f32 = 232.0;
        Rect::new(MARGIN_X, STATUS_TOP, SCR_W - 2.0 * MARGIN_X,
            SCR_H - STATUS_TOP - plx_ui::consts::MARGIN_Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("status_contract_tests.rs");
}
