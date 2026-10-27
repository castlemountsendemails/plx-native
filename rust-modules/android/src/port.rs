//! The Fire TV port table. The engine path reads only the sink, the sandbox verdict and the device
//! info, so every entry but the sink and the sealed-secret trio answers as a television with
//! no platform services.

use plx_platform::tv::{sandbox, toast, LocaleReply, Port};

use crate::amediacodec::AmcSink;
use crate::secure;

fn nothing() {}

pub static PORT: Port = Port {
    probe_device: nothing,
    start_capability_probe: nothing,
    repair_sandbox: || Err(sandbox::Failure::Unsupported),
    seal: secure::seal,
    open: secure::open,
    remove: secure::remove,
    system_locale: || LocaleReply::NoPlatform,
    go_home: nothing,
    poll_home: || false,
    deliver_toast: |_, _| toast::Sent {
        reply: None,
        outcome: toast::Outcome::NoBus,
    },
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
