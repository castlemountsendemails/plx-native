//! The Fire TV `tv::secure` backend: the Android Keystore key behind `PlxActivity.sealSecret`,
//! `openSecret` and `removeSecret`, and the harness that exercises it (`--es secure test | open |
//! remove`), whose blob lives in the app's files folder between runs.

use std::ffi::{c_char, CStr};
use std::path::PathBuf;

use plx_base::b64;
use plx_platform::tv::secure::{Backend, Sealed};

use crate::jni;
use crate::entry::{log, LOG_INFO};

const KEY: &str = "plxnative.session.v1";
const IV_LEN: usize = 12;
const DUMMY: &[u8] = b"plx dummy secret";

pub fn seal(plain: &[u8]) -> Option<Sealed> {
    let out = jni::seal_secret(KEY, plain)?;
    if out.len() <= IV_LEN {
        return None;
    }
    let (iv, data) = out.split_at(IV_LEN);
    Some(Sealed { backend: Backend::AndroidKeystore, key: KEY.into(), iv: b64::encode(iv), data: b64::encode(data) })
}

pub fn open(sealed: &Sealed) -> Option<Vec<u8>> {
    if sealed.backend != Backend::AndroidKeystore || sealed.key != KEY {
        return None;
    }
    jni::open_secret(KEY, &b64::decode(&sealed.iv)?, &b64::decode(&sealed.data)?)
}

pub fn remove(backend: &Backend, key: &str) {
    if *backend == Backend::AndroidKeystore && key == KEY {
        jni::remove_secret(KEY);
    }
}

extern "C" {
    fn SDL_AndroidGetInternalStoragePath() -> *const c_char;
}

fn blob_path() -> Option<PathBuf> {
    let dir = unsafe { SDL_AndroidGetInternalStoragePath() };
    (!dir.is_null()).then(|| PathBuf::from(unsafe { CStr::from_ptr(dir) }.to_string_lossy().into_owned()).join("secure-test.blob"))
}

fn stored() -> Option<Sealed> {
    let text = std::fs::read_to_string(blob_path()?).ok()?;
    let (iv, data) = text.split_once('\n')?;
    Some(Sealed { backend: Backend::AndroidKeystore, key: KEY.into(), iv: iv.into(), data: data.trim_end().into() })
}

fn verdict(what: &str, ok: bool) -> String {
    let line = format!("secure {what}: {}", if ok { "PASS" } else { "FAIL" });
    log(LOG_INFO, &line);
    line
}

/// Runs the `secure=` mode the intent asked for and returns its lines for the label.
pub fn harness(mode: &str) -> Vec<String> {
    let mut lines = vec![format!("secure {mode}")];
    match mode {
        "test" => {
            let sealed = seal(DUMMY);
            lines.push(verdict("seal", sealed.is_some()));
            let Some(sealed) = sealed else { return lines };
            lines.push(verdict("open matches", open(&sealed).as_deref() == Some(DUMMY)));
            let mut data = b64::decode(&sealed.data).unwrap_or_default();
            if let Some(first) = data.first_mut() {
                *first ^= 1;
            }
            let corrupt = Sealed { data: b64::encode(&data), ..sealed.clone() };
            lines.push(verdict("corrupt blob rejected", open(&corrupt).is_none()));
            let saved = blob_path().is_some_and(|path| std::fs::write(path, format!("{}\n{}\n", sealed.iv, sealed.data)).is_ok());
            lines.push(verdict("blob written", saved));
        }
        "open" => match stored() {
            Some(sealed) => lines.push(verdict("stored blob opens", open(&sealed).as_deref() == Some(DUMMY))),
            None => lines.push(verdict("stored blob read", false)),
        },
        "remove" => match stored() {
            Some(sealed) => {
                remove(&Backend::AndroidKeystore, KEY);
                lines.push(verdict("open after remove is None", open(&sealed).is_none()));
            }
            None => lines.push(verdict("stored blob read", false)),
        },
        _ => lines.push(verdict("mode known (test, open, remove)", false)),
    }
    lines
}
