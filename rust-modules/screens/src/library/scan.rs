//! The header's Scan library control: what its label says, and whether a press may send a scan.
//!
//! The label reports the server's answer, never the scan itself: a plain scan can finish in under
//! a second and the page hears no activity events, so there is no "scanning" state to show.
use plx_data::stores::browse::SectionAddress;

/// How long "Scan requested" or the error line stays before the label returns.
pub(super) const SHOWN_S: f32 = 3.0;
/// How long a sent request may go unanswered before the control takes presses again. The client
/// call always answers within its own timeout; this covers an answer that reached no page.
pub(super) const SENDING_S: f32 = 20.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum ScanControl {
    #[default]
    Idle,
    Sending { target: SectionAddress, left: f32 },
    Requested { left: f32 },
    Failed { left: f32 },
}

/// The label the control draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ScanLabel {
    Scan,
    Requested,
    Failed,
}

impl ScanLabel {
    pub(super) fn text(self) -> &'static str {
        use plx_platform::i18n::msg;
        match self {
            ScanLabel::Scan => msg::library_header_scan(),
            ScanLabel::Requested => msg::library_header_scan_requested(),
            ScanLabel::Failed => msg::library_header_scan_failed(),
        }
    }
}

impl ScanControl {
    pub(super) fn label(&self) -> ScanLabel {
        match self {
            ScanControl::Idle | ScanControl::Sending { .. } => ScanLabel::Scan,
            ScanControl::Requested { .. } => ScanLabel::Requested,
            ScanControl::Failed { .. } => ScanLabel::Failed,
        }
    }

    /// A press on the control: `true` when a scan of `target` is to be sent. A press while a
    /// request is out or its answer is still shown does nothing.
    pub(super) fn press(&mut self, target: SectionAddress) -> bool {
        if *self != ScanControl::Idle { return false; }
        *self = ScanControl::Sending { target, left: SENDING_S };
        true
    }

    /// The server answered a scan of `target`. An answer for another library, or one that comes
    /// after the wait ran out, is not this request's.
    pub(super) fn answered(&mut self, target: SectionAddress, accepted: bool) {
        if !matches!(*self, ScanControl::Sending { target: sent, .. } if sent == target) { return; }
        *self = if accepted { ScanControl::Requested { left: SHOWN_S } } else { ScanControl::Failed { left: SHOWN_S } };
    }

    pub(super) fn advance(&mut self, dt: f32) {
        let left = match self {
            ScanControl::Idle => return,
            ScanControl::Sending { left, .. } | ScanControl::Requested { left } | ScanControl::Failed { left } => left,
        };
        *left -= dt;
        if *left <= 0.0 { *self = ScanControl::Idle; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_plex::plex::ServerId;

    fn at(section: i64) -> SectionAddress {
        SectionAddress { epoch: 1, sid: ServerId::from_raw(1), section }
    }

    /// A press sends once; the label turns to "Scan requested" only on the server's yes, and back
    /// after a few seconds. Presses in between send nothing.
    #[test]
    fn an_accepted_scan_shows_requested_then_returns() {
        let mut scan = ScanControl::default();
        assert!(scan.press(at(69)));
        assert_eq!(scan.label(), ScanLabel::Scan, "nothing is claimed before the server answers");
        assert!(!scan.press(at(69)), "a second press while the request is out sends nothing");
        scan.answered(at(69), true);
        assert_eq!(scan.label(), ScanLabel::Requested);
        assert!(!scan.press(at(69)), "a press while Scan requested shows is ignored");
        scan.advance(SHOWN_S - 0.1);
        assert_eq!(scan.label(), ScanLabel::Requested);
        scan.advance(0.2);
        assert_eq!(scan, ScanControl::Idle);
        assert!(scan.press(at(69)), "the control takes presses again once the label is back");
    }

    /// A refusal or no answer shows the error line, never "Scan requested".
    #[test]
    fn a_refused_scan_shows_the_error_line() {
        let mut scan = ScanControl::default();
        assert!(scan.press(at(69)));
        scan.answered(at(69), false);
        assert_eq!(scan.label(), ScanLabel::Failed);
        assert!(!scan.press(at(69)));
        scan.advance(SHOWN_S + 0.1);
        assert_eq!(scan.label(), ScanLabel::Scan);
    }

    /// An answer for another library, or one with no request out, changes nothing.
    #[test]
    fn a_stray_answer_is_ignored() {
        let mut scan = ScanControl::default();
        scan.answered(at(69), true);
        assert_eq!(scan, ScanControl::Idle);
        assert!(scan.press(at(69)));
        scan.answered(at(70), true);
        assert!(matches!(scan, ScanControl::Sending { .. }));
    }

    /// A request whose answer never reached the page frees the control after the wait.
    #[test]
    fn an_unanswered_request_frees_the_control() {
        let mut scan = ScanControl::default();
        assert!(scan.press(at(69)));
        scan.advance(SENDING_S + 0.1);
        assert_eq!(scan, ScanControl::Idle);
        scan.answered(at(69), true);
        assert_eq!(scan, ScanControl::Idle, "a late answer is not shown");
    }

    /// The three labels are all present and all different.
    #[test]
    fn the_three_labels_differ() {
        let labels = [ScanLabel::Scan, ScanLabel::Requested, ScanLabel::Failed].map(ScanLabel::text);
        assert!(labels.iter().all(|s| !s.is_empty()));
        assert!(labels[0] != labels[1] && labels[1] != labels[2] && labels[0] != labels[2]);
    }
}
