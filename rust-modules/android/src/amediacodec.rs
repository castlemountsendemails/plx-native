//! The Fire TV video sink: the engine's video access units decoded by a `MediaCodec` that renders to
//! the video `SurfaceView`. The engine thread feeds input buffers; a drain thread shows each decoded
//! frame at its time on a clock that runs from the first frame after play, and reports it to the
//! engine as the position. Audio AUs are accepted and dropped. Lifecycle answers follow
//! `plx_media::player::ffi_host`'s clock sink, which only a `hostsim` build has.

use std::ffi::{c_char, c_int, c_long, c_uint, CStr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use plx_base::task::MainThread;
use plx_media::player::sf_on_event;
use plx_platform::tv::sink::{UnloadTicket, VideoSink, VP_EXPORTED};

use crate::jni::{Input, Output, VideoCodec};
use crate::skeleton::{log, LOG_ERROR, LOG_INFO};
use crate::video::{self, Action, Clock};

const FEED_OK: c_char = b'O' as c_char;
const FEED_FULL: c_char = b'B' as c_char;
const FEED_ERROR: c_char = b'e' as c_char;
const ES_VIDEO: c_int = 1;
/// The drain thread's pause when there was nothing to show.
const IDLE: Duration = Duration::from_millis(4);
/// The video lane logs its first few access units, then one in this many; so do shown frames.
const LOG_EVERY: u64 = 100;

static EPOCH: AtomicU32 = AtomicU32::new(0);
static READY: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);
static GATE_RETIRED: AtomicBool = AtomicBool::new(false);
static UNLOAD_COMPLETED: AtomicBool = AtomicBool::new(false);
static INTERCEPTS: AtomicU32 = AtomicU32::new(0);
static DRAINING: AtomicBool = AtomicBool::new(false);
static FULL_LOGGED: AtomicBool = AtomicBool::new(false);
static FED: AtomicU64 = AtomicU64::new(0);
static SHOWN: AtomicU64 = AtomicU64::new(0);
static LATE: AtomicU64 = AtomicU64::new(0);
static AUDIO_DROPPED: AtomicU64 = AtomicU64::new(0);

static DECODER: Mutex<Option<Decoder>> = Mutex::new(None);
static DRAIN: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static NAME: Mutex<String> = Mutex::new(String::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    i64::from(ts.tv_sec) * 1_000_000_000 + i64::from(ts.tv_nsec)
}

fn event(ty: c_int, num: i64, text: Option<&CStr>) {
    let epoch = EPOCH.load(Relaxed);
    if epoch != 0 && !GATE_RETIRED.load(Relaxed) {
        INTERCEPTS.fetch_add(1, Relaxed);
        sf_on_event(epoch, ty, num, text.map_or(std::ptr::null(), CStr::as_ptr));
    }
}

struct Decoder {
    codec: VideoCodec,
    /// A decoded frame waiting for its time: its output buffer index and PTS.
    pending: Option<(i32, i64)>,
    playing: bool,
    clock: Clock,
    last_fed_us: i64,
}

enum Step {
    Idle,
    Busy,
    Presented(i64),
    Failed,
}

impl Decoder {
    /// Forgets what the decoder held: after a flush its output buffer indices are stale.
    fn restart(&mut self) {
        self.pending = None;
        self.clock.reset();
    }

    fn step(&mut self) -> Step {
        if self.pending.is_none() {
            match self.codec.dequeue() {
                None => return Step::Failed,
                Some(Output::None) => return Step::Idle,
                Some(Output::FormatChanged) => return Step::Busy,
                Some(Output::Frame { index, end: true, .. }) => {
                    log(LOG_INFO, "amc: end of stream decoded");
                    self.codec.release(index, None);
                    return Step::Busy;
                }
                Some(Output::Frame { index, pts_us, end: false }) => {
                    self.pending = Some((index, pts_us.saturating_mul(1_000)));
                }
            }
        }
        let Some((index, pts)) = self.pending.filter(|_| self.playing) else {
            return Step::Idle;
        };
        let render = match self.clock.action(pts, now_ns()) {
            Action::Wait => return Step::Idle,
            Action::Show(at) => Some(at),
            Action::Drop => None,
        };
        self.pending = None;
        if self.codec.release(index, render).is_none() {
            return Step::Failed;
        }
        let n = if render.is_some() { SHOWN.fetch_add(1, Relaxed) + 1 } else { LATE.fetch_add(1, Relaxed) + 1 };
        if n <= 3 || n.is_multiple_of(LOG_EVERY) {
            let what = if render.is_some() { "shown" } else { "late, dropped" };
            log(LOG_INFO, &format!("amc: frame {what} #{n} pts={}ms", pts / 1_000_000));
        }
        Step::Presented(pts)
    }
}

fn with_decoder<T>(f: impl FnOnce(&mut Decoder) -> T) -> Option<T> {
    lock(&DECODER).as_mut().map(f)
}

/// Shows frames until unload. The position goes to the engine outside the decoder lock.
fn drain() {
    while DRAINING.load(Relaxed) {
        match with_decoder(Decoder::step) {
            Some(Step::Presented(pts)) => event(0, pts, None),
            Some(Step::Busy) => {}
            Some(Step::Idle) | None => std::thread::sleep(IDLE),
            Some(Step::Failed) => {
                log(LOG_ERROR, "amc: the decoder failed; no more frames are shown");
                DRAINING.store(false, Relaxed);
            }
        }
    }
}

fn stop_drain() {
    DRAINING.store(false, Relaxed);
    if let Some(drain) = lock(&DRAIN).take() {
        let _ = drain.join();
    }
}

fn teardown() {
    stop_drain();
    if let Some(decoder) = lock(&DECODER).take() {
        decoder.codec.close();
    }
    log(
        LOG_INFO,
        &format!(
            "amc: closed after fed={} shown={} late={} audio dropped={}",
            FED.load(Relaxed),
            SHOWN.load(Relaxed),
            LATE.load(Relaxed),
            AUDIO_DROPPED.load(Relaxed)
        ),
    );
}

fn note_feed(size: c_uint, pts: i64) {
    let n = FED.fetch_add(1, Relaxed) + 1;
    if n <= 5 || n.is_multiple_of(LOG_EVERY) {
        log(LOG_INFO, &format!("amc: feed video #{n} pts={}ms size={size}", pts / 1_000_000));
    }
}

pub struct AmcSink;

impl VideoSink for AmcSink {
    unsafe fn load(&self, payload: *const c_char, epoch: u32) -> c_int {
        if epoch == 0 || READY.load(Relaxed) {
            return 0;
        }
        let payload = if payload.is_null() { "".into() } else { CStr::from_ptr(payload).to_string_lossy() };
        let Some(format) = video::format(&payload) else {
            log(LOG_ERROR, &format!("amc: no decoder for the load payload {payload}"));
            return 0;
        };
        let Some(codec) = VideoCodec::open(format.mime, format.width, format.height) else {
            log(LOG_ERROR, &format!("amc: {format:?} did not open"));
            return 0;
        };
        log(LOG_INFO, &format!("amc: {} started for {format:?}", codec.name));
        lock(&NAME).clone_from(&codec.name);
        *lock(&DECODER) = Some(Decoder { codec, pending: None, playing: false, clock: Clock::default(), last_fed_us: 0 });
        for count in [&FED, &SHOWN, &LATE, &AUDIO_DROPPED] {
            count.store(0, Relaxed);
        }
        FULL_LOGGED.store(false, Relaxed);
        EPOCH.store(epoch, Relaxed);
        INTERCEPTS.store(0, Relaxed);
        GATE_RETIRED.store(false, Relaxed);
        UNLOAD_COMPLETED.store(false, Relaxed);
        READY.store(true, Relaxed);
        LOADED.store(true, Relaxed);
        DRAINING.store(true, Relaxed);
        match std::thread::Builder::new().name("amc-drain".into()).spawn(drain) {
            Ok(drain) => *lock(&DRAIN) = Some(drain),
            Err(e) => log(LOG_ERROR, &format!("amc: no drain thread ({e}); no frames will be shown")),
        }
        // Completion goes through the engine's callback parse, as a real pipeline's does.
        event(2, 0, Some(c"{\"loadCompleted\":true}"));
        1
    }
    unsafe fn ready(&self, _: &MainThread) -> c_int {
        c_int::from(READY.load(Relaxed))
    }
    unsafe fn is_load_completed(&self, _: &MainThread) -> c_int {
        c_int::from(LOADED.load(Relaxed))
    }
    unsafe fn play(&self, _: &MainThread) -> c_int {
        let resumed = with_decoder(|d| {
            if !d.playing {
                d.playing = true;
                d.clock.reset();
            }
        });
        c_int::from(resumed.is_some())
    }
    unsafe fn pause(&self, _: &MainThread) -> c_int {
        let held = with_decoder(|d| {
            d.playing = false;
            d.clock.reset();
        });
        c_int::from(held.is_some())
    }
    unsafe fn set_volume(&self, _: &MainThread, _volume: c_int) -> c_int {
        1
    }
    unsafe fn flush(&self, _: &MainThread) -> c_int {
        let flushed = with_decoder(|d| {
            d.restart();
            d.codec.flush()
        });
        c_int::from(flushed.flatten().is_some())
    }
    unsafe fn push_eos(&self, _: &MainThread) -> c_int {
        match with_decoder(|d| d.codec.queue(&[], d.last_fed_us, true)).flatten() {
            Some(Input::Queued) => {
                log(LOG_INFO, "amc: end of stream queued");
                1
            }
            _ => {
                log(LOG_ERROR, "amc: end of stream not queued: no free input buffer");
                0
            }
        }
    }
    unsafe fn set_time_to_decode(&self, _: &MainThread, _position_ns: i64) -> c_int {
        1
    }
    unsafe fn set_content_info(&self, _: &MainThread, _position_ns: i64) -> c_int {
        1
    }
    unsafe fn send_segment(&self, _: &MainThread) -> c_int {
        1
    }
    unsafe fn seek(&self, mt: &MainThread, ms: i64) -> c_int {
        if ms < 0 {
            return 0;
        }
        self.flush(mt)
    }
    fn feed_abi_is_cow(&self) -> bool {
        false
    }
    unsafe fn feed(&self, _: &MainThread, p: *const u8, size: c_uint, pts: i64, es_data: c_int) -> c_char {
        if es_data != ES_VIDEO {
            AUDIO_DROPPED.fetch_add(1, Relaxed);
            return FEED_OK;
        }
        let data = if p.is_null() { &[][..] } else { std::slice::from_raw_parts(p, size as usize) };
        let pts_us = pts / 1_000;
        let queued = with_decoder(|d| {
            let input = d.codec.queue(data, pts_us, false);
            if matches!(input, Some(Input::Queued)) {
                d.last_fed_us = pts_us;
            }
            (input, d.playing)
        });
        match queued {
            Some((Some(Input::Queued), _)) => {
                note_feed(size, pts);
                FEED_OK
            }
            Some((Some(Input::Full), playing)) => {
                if !FULL_LOGGED.swap(true, Relaxed) {
                    let fed = FED.load(Relaxed);
                    log(LOG_INFO, &format!("amc: decoder input full after {fed} video AUs (playing={playing})"));
                }
                FEED_FULL
            }
            Some((Some(Input::TooBig(capacity)), _)) => {
                log(LOG_ERROR, &format!("amc: a {size}-byte AU is over the {capacity}-byte input buffer"));
                FEED_ERROR
            }
            Some((None, _)) | None => FEED_ERROR,
        }
    }
    unsafe fn unload(&self, _owner: &UnloadTicket) {
        teardown();
        if READY.load(Relaxed) {
            UNLOAD_COMPLETED.store(true, Relaxed);
            // UNLOADCOMPLETED, which the engine's release takes as its evidence.
            sf_on_event(EPOCH.load(Relaxed), 23, 0, std::ptr::null());
        }
        LOADED.store(false, Relaxed);
    }
    unsafe fn callback_gate_retire(&self, _: &MainThread) -> c_int {
        GATE_RETIRED.store(true, Relaxed);
        c_int::from(READY.load(Relaxed) && INTERCEPTS.load(Relaxed) != 0)
    }
    unsafe fn callback_intercepts(&self, _: &MainThread) -> u32 {
        INTERCEPTS.load(Relaxed)
    }
    unsafe fn destroy(&self, _: &MainThread) -> c_int {
        let clean = READY.load(Relaxed) && GATE_RETIRED.load(Relaxed) && UNLOAD_COMPLETED.load(Relaxed);
        EPOCH.store(0, Relaxed);
        READY.store(false, Relaxed);
        LOADED.store(false, Relaxed);
        c_int::from(clean)
    }
    unsafe fn quarantine(&self, _: &MainThread) {
        GATE_RETIRED.store(true, Relaxed);
        teardown();
        READY.store(false, Relaxed);
        LOADED.store(false, Relaxed);
    }
    fn window_mode(&self) -> c_int {
        // The exported-window path: the engine skips the ACB bind stages entirely.
        VP_EXPORTED
    }
    fn window_id(&self) -> &'static CStr {
        c"amc-window"
    }
    unsafe fn create_window(&self, _: &MainThread) -> *const c_char {
        c"amc-window".as_ptr()
    }
    unsafe fn place_window(&self, _: &MainThread, _src_w: c_int, _src_h: c_int, _dst_x: c_int,
        _dst_y: c_int, _dst_w: c_int, _dst_h: c_int) -> c_int {
        1
    }
    unsafe fn destroy_window(&self, _: &MainThread) {}
    unsafe fn plane_create(&self, _: &MainThread, _app_id: *const c_char, _player_type: c_int) -> c_long {
        0
    }
    unsafe fn plane_bind(&self, _: &MainThread, _media_id: *const c_char) {}
    unsafe fn plane_send_video_data(&self, _: &MainThread, _source_info: *const c_char) -> c_int {
        -1
    }
    unsafe fn plane_send_atmos(&self, _: &MainThread, _media_id: *const c_char) -> c_int {
        0
    }
    unsafe fn plane_start(&self, _: &MainThread, _x: c_long, _y: c_long, _w: c_long, _h: c_long) {}
    unsafe fn plane_unload(&self, _: &MainThread) {}
    unsafe fn plane_pause(&self, _: &MainThread) {}
    unsafe fn plane_resume(&self, _: &MainThread) {}
}

/// For the label: the decoder's name, then video AUs fed, frames shown and late frames dropped
/// since the last Load.
pub fn stats() -> (String, u64, u64, u64) {
    (lock(&NAME).clone(), FED.load(Relaxed), SHOWN.load(Relaxed), LATE.load(Relaxed))
}
