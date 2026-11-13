//! Delete an item's media from its server, off the frame thread, and tell the viewer how it went.
//!
//! The call is `DELETE /library/metadata/{rk}`, sent only after the item menu's confirm was
//! answered Delete (`ItemMenuScreen::confirm_answer`, the one producer of the action). It is not
//! an optimistic edit: a refusal must leave the card where it is, so nothing changes on screen
//! until the server has said yes.
//!
//! **Only a 2xx counts as deleted.** The refusal code for a server with deletion off is unproven
//! (probably 400), so every other status, and no answer at all, is one outcome for the viewer
//! ("Plex didn't delete it"). Nothing retries.

use std::sync::{Arc, Mutex};

use plx_data::viewstate::Scope;
use plx_plex::plex::ServerId;

/// How a delete ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The server answered 2xx.
    Deleted,
    /// The server answered something else.
    Refused(i32),
    /// Nothing answered, or no client was left to ask.
    NoAnswer,
}

impl Outcome {
    /// The grade of a DELETE's status. `None` is a request that never completed.
    pub(crate) fn of_status(status: Option<i32>) -> Outcome {
        match status {
            Some(s) if (200..300).contains(&s) => Outcome::Deleted,
            Some(s) => Outcome::Refused(s),
            None => Outcome::NoAnswer,
        }
    }
}

/// One finished delete, back on the main thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Landing {
    pub(crate) sid: ServerId,
    pub(crate) rk: String,
    pub(crate) title: String,
    /// How far the item's rows reach: a show's episodes go with it.
    pub(crate) scope: Scope,
    pub(crate) outcome: Outcome,
}

/// The mailbox between the workers and the frame loop.
#[derive(Default)]
pub(crate) struct DeleteJobs {
    mail: Arc<Mutex<Vec<Landing>>>,
}

impl DeleteJobs {
    /// Send the delete on a worker. A server with no client left, or a refused spawn, lands at once
    /// as [`Outcome::NoAnswer`] so the viewer is told rather than left waiting.
    pub(crate) fn start(&self, sid: ServerId, rk: String, title: String, scope: Scope) {
        let base = Landing { sid, rk, title, scope, outcome: Outcome::NoAnswer };
        let Some(client) = plx_plex::plex::client_for(sid) else {
            plx_base::eventlog::log(&format!("delete: server {} is not registered", sid.raw()));
            self.post(base);
            return;
        };
        let (mail, sent) = (Arc::clone(&self.mail), base.clone());
        let spawned = plx_base::task::spawn_small("delete item", move || {
            let status = client.delete_item(&sent.rk);
            let outcome = Outcome::of_status(status);
            plx_base::eventlog::log(&format!(
                "delete: rk={} server {} -> {status:?} ({outcome:?})", sent.rk, sid.raw()));
            mail.lock().unwrap_or_else(|e| e.into_inner()).push(Landing { outcome, ..sent });
        });
        if !spawned {
            self.post(base);
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

/// Raise `message` as the television's own toast, off the frame thread (the call blocks).
pub(crate) fn toast(message: String) {
    plx_base::task::spawn_small("delete-notice", move || {
        let outcome = plx_platform::tv::toast::toast(&message);
        plx_base::eventlog::log(&format!("delete: toast {outcome:?}"));
    });
}

/// The toast for a landing.
pub(crate) fn message(landing: &Landing) -> String {
    use plx_platform::i18n::msg;
    match landing.outcome {
        Outcome::Deleted => msg::browse_menu_deleted_toast(&landing.title),
        Outcome::Refused(_) | Outcome::NoAnswer => msg::browse_menu_delete_failed(&landing.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DELETE is "deleted" only on a 2xx. A refusal whatever its code, and silence, are not, so
    /// a card never leaves on a server that said no.
    #[test]
    fn only_a_2xx_counts_as_deleted() {
        for ok in [200, 201, 204, 299] {
            assert_eq!(Outcome::of_status(Some(ok)), Outcome::Deleted, "{ok}");
        }
        for no in [100, 301, 400, 401, 403, 404, 500] {
            assert_eq!(Outcome::of_status(Some(no)), Outcome::Refused(no), "{no}");
        }
        assert_eq!(Outcome::of_status(None), Outcome::NoAnswer);
    }

    /// A server that is not registered answers at once, as a failure, never as a delete.
    #[test]
    fn an_unregistered_server_is_a_failure_the_viewer_hears_about() {
        let jobs = DeleteJobs::default();
        jobs.start(ServerId::UNSET, "42".into(), "Heat".into(), Scope::Leaf);
        let got = jobs.take();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].rk.as_str(), got[0].title.as_str(), got[0].outcome), ("42", "Heat", Outcome::NoAnswer));
        assert!(jobs.take().is_empty(), "taken once");
        assert!(message(&got[0]).contains("Heat"), "the failure names the title");
    }

    /// The toast names the title in both outcomes, and says different things.
    #[test]
    fn the_toast_names_the_title() {
        let landing = |outcome| Landing { sid: ServerId::UNSET, rk: "1".into(), title: "Heat".into(), scope: Scope::Leaf, outcome };
        let (done, failed) = (message(&landing(Outcome::Deleted)), message(&landing(Outcome::Refused(400))));
        assert!(done.contains("Heat") && failed.contains("Heat"));
        assert_ne!(done, failed);
        assert_eq!(failed, message(&landing(Outcome::NoAnswer)));
    }
}
