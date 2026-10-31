//! The Android session record: the public half (`split_public`) as plain JSON beside the
//! credentials sealed by the platform key (`tv::secure`). A seal that will not open reads as
//! `Locked`, and only a fresh sign-in may seal over it, so a passing Keystore error never costs
//! the credentials.

use plx_platform::storage::{state::PublicPayload, StoreError};
use plx_platform::tv::secure::{self, Sealed};
use serde::{Deserialize, Serialize};

use super::persistence::CanonicalRead;
use super::{SaveAuthority, Session};

const FORMAT: &str = "plxnative-session-sealed";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    format: String,
    version: u32,
    public: PublicPayload,
    sealed: Sealed,
}

fn parse(payload: &str) -> Option<Record> {
    serde_json::from_str::<Record>(payload).ok().filter(|record| record.format == FORMAT && record.version == 1)
}

/// Whether the payload names this format at all: a damaged sealed record must never be taken
/// for a plain session.
fn claims_format(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .is_ok_and(|value| value.get("format").and_then(serde_json::Value::as_str) == Some(FORMAT))
}

/// A plain payload is a record written before sealing; it reads as `Data`, and the save that
/// follows a plaintext read seals it.
pub(super) fn read(revision: u64, payload: String) -> CanonicalRead {
    let Some(record) = parse(&payload) else {
        if claims_format(&payload) {
            return CanonicalRead::Blocked(StoreError::InvalidSchema);
        }
        return CanonicalRead::Data { revision, payload };
    };
    let Some(plain) = secure::open(&record.sealed) else {
        return CanonicalRead::Locked {
            revision,
            public: super::public_session(&record.public),
            protection: Some(protection()),
        };
    };
    match std::str::from_utf8(&plain).ok().and_then(|auth| super::join_canonical(&record.public, auth).ok()) {
        Some(session) => CanonicalRead::Opened { revision, session },
        None => CanonicalRead::Blocked(StoreError::InvalidSchema),
    }
}

/// The record `session` is written as over the stored payload `current`. Unless this is a fresh
/// sign-in, a stored seal is kept when it opens to the same credentials, and also when it does not
/// open at all: then only the public half is rewritten.
pub(super) fn encode(
    session: &Session,
    authority: SaveAuthority,
    current: Option<&str>,
) -> Result<String, StoreError> {
    let (public, auth) = super::split_canonical(session).map_err(|()| StoreError::InvalidSchema)?;
    let kept = match current {
        Some(current) if authority != SaveAuthority::FreshReauthentication => match parse(current) {
            Some(old) => match secure::open(&old.sealed) {
                Some(plain) if plain != auth.as_bytes() => None,
                _ => Some(old.sealed),
            },
            None if claims_format(current) => return Err(StoreError::AuthLocked),
            None => None,
        },
        _ => None,
    };
    let sealed = match kept {
        Some(sealed) => sealed,
        None => secure::seal(auth.as_bytes()).ok_or(StoreError::AuthLocked)?,
    };
    serde_json::to_string(&Record { format: FORMAT.into(), version: 1, public, sealed })
        .map_err(|_| StoreError::InvalidSchema)
}

pub(super) fn protection() -> plx_platform::storage::wire::ProtectionOutcome {
    plx_platform::storage::wire::ProtectionOutcome {
        class: plx_platform::storage::wire::ProtectionClass::Keymanager,
        fallback: None,
        fallback_context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plx_platform::tv::secure::{Backend, Sealed, TestStore, STORE_FOR_TEST};
    use std::cell::Cell;

    std::thread_local! {
        static KEY: Cell<u32> = const { Cell::new(0) };
    }

    // A Keystore stand-in: a seal opens only under the key that made it, and `remove` makes the next
    // seal use a new key, as `sealSecret` does once the alias is gone.
    static KEYSTORE: TestStore = TestStore {
        seal: |plain| Some(Sealed {
            backend: Backend::AndroidKeystore,
            key: "plxnative.session.v1".into(),
            iv: KEY.with(Cell::get).to_string(),
            data: plx_base::b64::encode(plain),
        }),
        open: |sealed| (sealed.iv == KEY.with(Cell::get).to_string()).then(|| plx_base::b64::decode(&sealed.data)).flatten(),
        remove: |_, _| KEY.with(|key| key.set(key.get() + 1)),
    };

    struct Keystore;
    impl Keystore {
        fn new() -> Self {
            KEY.with(|key| key.set(0));
            STORE_FOR_TEST.with(|hook| hook.set(Some(&KEYSTORE)));
            Keystore
        }
        fn lose_key(&self) {
            plx_platform::tv::secure::remove(&Backend::AndroidKeystore, "plxnative.session.v1");
        }
        fn restore_key(&self) {
            KEY.with(|key| key.set(0));
        }
    }
    impl Drop for Keystore {
        fn drop(&mut self) {
            STORE_FOR_TEST.with(|hook| hook.set(None));
        }
    }

    fn signed_in() -> Session {
        let mut session = super::super::test_support::signed_in();
        session.recent_searches = vec![super::super::RecentSearches { terms: vec!["alien".into()], ..Default::default() }];
        session
    }

    fn sealed_part(payload: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(payload).unwrap()["sealed"].clone()
    }

    fn opened(read: CanonicalRead) -> Session {
        match read {
            CanonicalRead::Opened { session, .. } => session,
            _ => panic!("expected the record to open"),
        }
    }

    #[test]
    fn the_credentials_are_sealed_and_open_again_beside_the_plain_public_half() {
        let _keystore = Keystore::new();
        let payload = encode(&signed_in(), SaveAuthority::FreshReauthentication, None).unwrap();
        assert!(!payload.contains("acct"), "the account token is in the plain JSON: {payload}");
        assert!(payload.contains("cid-1"), "the client id belongs to the public half: {payload}");
        let session = opened(read(1, payload));
        assert_eq!(session.account_token, "acct");
        assert_eq!(session.client_id, "cid-1");
        assert_eq!(session.recent_searches, signed_in().recent_searches);
    }

    #[test]
    fn key_lost_reads_as_locked_and_boots_signed_out_without_a_save() {
        let keystore = Keystore::new();
        let payload = encode(&signed_in(), SaveAuthority::FreshReauthentication, None).unwrap();
        keystore.lose_key();
        let read = read(1, payload);
        assert!(matches!(read, CanonicalRead::Locked { .. }), "an unopenable seal must read as Locked");
        let state = super::super::read_locked(read);
        assert!(matches!(state, super::super::ReadState::Locked { .. }));
        let (session, save) = super::super::prepare_load(&state, || "cid-new".into());
        assert!(session.account_token.is_empty(), "key lost must boot signed out");
        assert!(!save, "a locked boot must not write over the sealed record");
    }

    #[test]
    fn while_locked_any_save_but_a_fresh_sign_in_keeps_the_sealed_bytes() {
        let keystore = Keystore::new();
        let before = encode(&signed_in(), SaveAuthority::FreshReauthentication, None).unwrap();
        keystore.lose_key();
        for authority in [SaveAuthority::Routine, SaveAuthority::PublicOnly] {
            let signed_out = Session { client_id: "cid-2".into(), ..Default::default() };
            let after = encode(&signed_out, authority, Some(&before)).unwrap();
            assert_eq!(sealed_part(&after), sealed_part(&before), "{authority:?} rewrote the sealed part");
            assert!(after.contains("cid-2"), "{authority:?} did not rewrite the public half: {after}");
            keystore.restore_key();
            assert_eq!(opened(read(2, after)).account_token, "acct", "the kept seal no longer opens");
            keystore.lose_key();
        }
    }

    #[test]
    fn while_locked_a_fresh_sign_in_seals_over_and_opens() {
        let keystore = Keystore::new();
        let before = encode(&signed_in(), SaveAuthority::FreshReauthentication, None).unwrap();
        keystore.lose_key();
        let fresh = Session { client_id: "cid-2".into(), account_token: "acct-2".into(), ..Default::default() };
        let after = encode(&fresh, SaveAuthority::FreshReauthentication, Some(&before)).unwrap();
        let session = opened(read(2, after));
        assert_eq!(session.account_token, "acct-2");
        assert_eq!(session.client_id, "cid-2");
    }

    #[test]
    fn a_plain_record_reads_as_data_and_a_damaged_sealed_one_is_refused() {
        let _keystore = Keystore::new();
        let plain = serde_json::to_string(&signed_in()).unwrap();
        assert!(matches!(read(1, plain.clone()), CanonicalRead::Data { .. }));
        let resealed = encode(&signed_in(), SaveAuthority::Routine, Some(&plain)).unwrap();
        assert_eq!(opened(read(2, resealed)).account_token, "acct");
        let damaged = r#"{"format":"plxnative-session-sealed","version":1}"#;
        assert!(matches!(read(1, damaged.into()), CanonicalRead::Blocked(StoreError::InvalidSchema)));
        assert_eq!(encode(&signed_in(), SaveAuthority::Routine, Some(damaged)), Err(StoreError::AuthLocked));
    }
}
