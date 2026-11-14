//! The header's Scan library control: what its label says, and whether a press may send a scan.
//!
//! The label reports the server's answer, never the scan itself: a plain scan can finish in under
//! a second and the page hears no activity events, so there is no "scanning" state to show.
use plx_data::stores::browse::SectionAddress;

/// How long "Scan requested" or the error line stays before the label returns, in ms.
pub(super) const ANSWER_MS: u32 = 3_000;
/// How long a sent request may go unanswered before the control takes presses again, in ms. The
/// client call always answers within its own timeout; this covers an answer that reached no page.
pub(super) const SENDING_MS: u32 = 20_000;

/// Each state holds the tick time (`Tick::ms`) it began at; it ends by reading the clock, never by
/// summing frame deltas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ScanControl {
    #[default]
    Idle,
    Sending { target: SectionAddress, since: u32 },
    Requested { since: u32 },
    Failed { since: u32 },
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
    pub(super) fn press(&mut self, target: SectionAddress, now: u32) -> bool {
        if *self != ScanControl::Idle { return false; }
        *self = ScanControl::Sending { target, since: now };
        true
    }

    /// The server answered a scan of `target`. An answer for another library, or one that comes
    /// after the wait ran out, is not this request's.
    pub(super) fn answered(&mut self, target: SectionAddress, accepted: bool, now: u32) {
        if !matches!(*self, ScanControl::Sending { target: sent, .. } if sent == target) { return; }
        *self = if accepted { ScanControl::Requested { since: now } } else { ScanControl::Failed { since: now } };
    }

    /// Back to idle once the state has lasted its time. The difference is read signed, so the
    /// tick clock wrapping does not end a state early.
    pub(super) fn advance(&mut self, now: u32) {
        let (since, lasts) = match *self {
            ScanControl::Idle => return,
            ScanControl::Sending { since, .. } => (since, SENDING_MS),
            ScanControl::Requested { since } | ScanControl::Failed { since } => (since, ANSWER_MS),
        };
        if now.wrapping_sub(since) as i32 >= lasts as i32 { *self = ScanControl::Idle; }
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
        assert!(scan.press(at(69), 1_000));
        assert_eq!(scan.label(), ScanLabel::Scan, "nothing is claimed before the server answers");
        assert!(!scan.press(at(69), 1_100), "a second press while the request is out sends nothing");
        scan.answered(at(69), true, 1_200);
        assert_eq!(scan.label(), ScanLabel::Requested);
        assert!(!scan.press(at(69), 1_300), "a press while Scan requested shows is ignored");
        scan.advance(1_200 + ANSWER_MS - 1);
        assert_eq!(scan.label(), ScanLabel::Requested);
        scan.advance(1_200 + ANSWER_MS);
        assert_eq!(scan, ScanControl::Idle);
        assert!(scan.press(at(69), 5_000), "the control takes presses again once the label is back");
    }

    /// A refusal or no answer shows the error line, never "Scan requested".
    #[test]
    fn a_refused_scan_shows_the_error_line() {
        let mut scan = ScanControl::default();
        assert!(scan.press(at(69), 0));
        scan.answered(at(69), false, 10);
        assert_eq!(scan.label(), ScanLabel::Failed);
        assert!(!scan.press(at(69), 20));
        scan.advance(10 + ANSWER_MS);
        assert_eq!(scan.label(), ScanLabel::Scan);
    }

    /// An answer for another library, or one with no request out, changes nothing.
    #[test]
    fn a_stray_answer_is_ignored() {
        let mut scan = ScanControl::default();
        scan.answered(at(69), true, 0);
        assert_eq!(scan, ScanControl::Idle);
        assert!(scan.press(at(69), 0));
        scan.answered(at(70), true, 0);
        assert!(matches!(scan, ScanControl::Sending { .. }));
    }

    /// A request whose answer never reached the page frees the control after the wait, and a late
    /// answer is not shown.
    #[test]
    fn an_unanswered_request_frees_the_control() {
        let mut scan = ScanControl::default();
        assert!(scan.press(at(69), 0));
        scan.advance(SENDING_MS);
        assert_eq!(scan, ScanControl::Idle);
        scan.answered(at(69), true, SENDING_MS + 1);
        assert_eq!(scan, ScanControl::Idle);
    }

    /// A state that began just before the tick clock wraps still lasts its full time.
    #[test]
    fn the_clock_wrapping_does_not_end_a_state_early() {
        let mut scan = ScanControl::default();
        let start = u32::MAX - 500;
        assert!(scan.press(at(69), start));
        scan.answered(at(69), true, start);
        scan.advance(start.wrapping_add(ANSWER_MS - 1));
        assert_eq!(scan.label(), ScanLabel::Requested);
        scan.advance(start.wrapping_add(ANSWER_MS));
        assert_eq!(scan, ScanControl::Idle);
    }

    /// The three labels are all present and all different.
    #[test]
    fn the_three_labels_differ() {
        let labels = [ScanLabel::Scan, ScanLabel::Requested, ScanLabel::Failed].map(ScanLabel::text);
        assert!(labels.iter().all(|s| !s.is_empty()));
        assert!(labels[0] != labels[1] && labels[1] != labels[2] && labels[0] != labels[2]);
    }
}
