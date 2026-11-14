//! Ask a server to scan one library, off the frame thread, and hand the Library page the answer.
//!
//! The call is a plain `POST /library/sections/{key}/refresh`. Only a 2xx counts as accepted;
//! a refusal and no answer at all are one outcome for the viewer. Nothing retries.

use std::sync::{Arc, Mutex};

use plx_data::stores::browse::SectionAddress;

/// One finished request, back on the main thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Landing {
    pub(crate) target: SectionAddress,
    pub(crate) accepted: bool,
}

/// The mailbox between the workers and the frame loop.
#[derive(Default)]
pub(crate) struct ScanJobs {
    mail: Arc<Mutex<Vec<Landing>>>,
}

impl ScanJobs {
    /// Send the scan on a worker. A server with no client left, or a refused spawn, lands at once
    /// as not accepted, so the control shows its error line rather than waiting.
    pub(crate) fn start(&self, target: SectionAddress) {
        let refused = Landing { target, accepted: false };
        let Some(client) = plx_plex::plex::client_for(target.sid) else {
            plx_base::eventlog::log(&format!("scan: server {} is not registered", target.sid.raw()));
            self.post(refused);
            return;
        };
        let mail = Arc::clone(&self.mail);
        let spawned = plx_base::task::spawn_small("scan library", move || {
            let status = client.scan_section(target.section);
            let accepted = status.is_some_and(|s| (200..300).contains(&s));
            plx_base::eventlog::log(&format!(
                "scan: section {} server {} -> {status:?}", target.section, target.sid.raw()));
            mail.lock().unwrap_or_else(|e| e.into_inner()).push(Landing { target, accepted });
        });
        if !spawned {
            self.post(refused);
        }
    }

    fn post(&self, landing: Landing) {
        self.mail.lock().unwrap_or_else(|e| e.into_inner()).push(landing);
    }

    /// Everything that finished since the last frame.
    pub(crate) fn take(&self) -> Vec<Landing> {
        std::mem::take(&mut *self.mail.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_plex::plex::ServerId;

    /// A server that is not registered answers at once, as not accepted.
    #[test]
    fn an_unregistered_server_is_a_refusal_the_page_hears_about() {
        let jobs = ScanJobs::default();
        let target = SectionAddress { epoch: 3, sid: ServerId::UNSET, section: 69 };
        jobs.start(target);
        assert_eq!(jobs.take(), vec![Landing { target, accepted: false }]);
        assert!(jobs.take().is_empty(), "taken once");
    }
}
