//! The Fire TV port table, installed before the app or the clip harness runs. Every entry but the
//! device probe, the locale, the toast, the root press, the sink and the sealed-secret trio answers
//! as a television with no platform services.

use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, Ordering};

use plx_platform::tv::{device, sandbox, toast, LocaleReply, Port};

use crate::amediacodec::AmcSink;
use crate::jni;
use crate::secure;

fn nothing() {}

fn property(name: &CStr) -> String {
    let mut value = [0 as c_char; 92]; // PROP_VALUE_MAX
    unsafe {
        libc::__system_property_get(name.as_ptr(), value.as_mut_ptr());
        CStr::from_ptr(value.as_ptr()).to_string_lossy().into_owned()
    }
}

/// `major` stays 0 with a known release: the shared playback gates read it as a webOS version.
fn probe_device() {
    device::publish_info(device::Info {
        release: property(c"ro.build.version.release"),
        codename: String::new(),
        api: property(c"ro.build.version.sdk"),
        name: "Android".into(),
        major: 0,
    });
    device::publish_hardware(device::Hardware {
        model: property(c"ro.product.model"),
        board: property(c"ro.board.platform"),
        hw_revision: String::new(),
    });
}

/// `Accepted` means the toast was posted to the UI thread; Android gives no reply after that.
fn deliver_toast(message: &str, _identity: toast::Identity) -> toast::Sent {
    let outcome = match jni::show_toast(message) {
        Some(true) => toast::Outcome::Accepted,
        Some(false) => toast::Outcome::Bus { stage: "context", code: None, detail: "no activity yet".into() },
        None => toast::Outcome::Bus { stage: "jni", code: None, detail: String::new() },
    };
    toast::Sent { reply: None, outcome }
}

/// A root BACK opens the exit confirm rather than leaving for the Fire launcher: `go_home` raises
/// this and the frame thread's `poll_home` takes it.
static HOME_QUIT: AtomicBool = AtomicBool::new(false);

pub static PORT: Port = Port {
    probe_device,
    start_capability_probe: nothing,
    repair_sandbox: || Err(sandbox::Failure::Unsupported),
    seal: secure::seal,
    open: secure::open,
    remove: secure::remove,
    system_locale: || jni::system_locale().map_or(LocaleReply::Unavailable, LocaleReply::Reply),
    go_home: || HOME_QUIT.store(true, Ordering::Release),
    poll_home: || HOME_QUIT.swap(false, Ordering::AcqRel),
    deliver_toast,
    bind_window: |_| {},
    grab_surface: |_| {},
    release_surface: || false,
    arm_opaque_region: nothing,
    opaque_route: |_| {},
    clear_opaque_region: nothing,
    pump_bus: nothing,
    frame_probe_request: nothing,
    frame_probe_prepare_begin: nothing,
    frame_probe_prepare_end: nothing,
    frame_probe_waiting: nothing,
    frame_probe_acquired: nothing,
    frame_probe_fields: String::new,
    sink: &AmcSink,
};
