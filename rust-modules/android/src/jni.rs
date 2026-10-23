//! The static natives `PlxActivity` calls. They run on the Activity's UI thread and the SDL thread
//! reads what they leave, so all state sits behind one mutex.

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use crate::keys::{self, Keys, Press};
use crate::skeleton::{log, LOG_INFO};

struct Bridge {
    keys: Keys,
    readout: String,
}

static BRIDGE: Mutex<Bridge> = Mutex::new(Bridge {
    keys: Keys::new(),
    readout: String::new(),
});

fn bridge() -> MutexGuard<'static, Bridge> {
    BRIDGE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

impl Bridge {
    fn record(&mut self, presses: Vec<Press>) {
        for Press { key, edge, code } in presses {
            self.readout = format!("keycode {code} -> {key:?} ({edge:?})");
            log(LOG_INFO, &self.readout);
        }
    }
}

/// `static native boolean nativeKey(int keyCode, int action, int repeatCount)`. True for a mapped
/// key, which Java then consumes so SDL never sees it (SDL's BACK would finish the Activity).
#[no_mangle]
pub extern "system" fn Java_com_beb_plxnative_PlxActivity_nativeKey(
    _env: *mut c_void,
    _class: *mut c_void,
    code: i32,
    action: i32,
    repeat: i32,
) -> u8 {
    let mut bridge = bridge();
    let presses = bridge.keys.event(code, action, repeat, now_ms());
    bridge.record(presses);
    u8::from(keys::map(code).is_some())
}

pub fn tick() {
    let mut bridge = bridge();
    let presses = bridge.keys.tick(now_ms());
    bridge.record(presses);
}

/// The last press, as the skeleton draws it; empty before the first.
pub fn readout() -> String {
    bridge().readout.clone()
}
