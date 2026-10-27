//! The Fire TV video sink: the engine's video access units decoded by a `MediaCodec` that renders to
//! the video `SurfaceView`. The engine thread queues AUs, which it and the drain thread move into
//! free input buffers; the drain thread shows each decoded
//! frame at its time against the audio track's clock (or, with no audio played, on a clock that runs
//! from the first frame after play), and reports it to the engine as the position. Audio AUs go to
//! `audiotrack`, and the transport verbs reach it too. Lifecycle answers follow
//! `plx_media::player::ffi_host`'s clock sink, which only a `hostsim` build has.

use std::ffi::{c_char, c_int, c_long, c_uint, CStr};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use plx_base::task::MainThread;
use plx_media::player::sf_on_event;
use plx_platform::tv::sink::{UnloadTicket, VideoSink, VP_EXPORTED};

use crate::audiotrack;
use crate::jni::{Codec, Input, Output};
use crate::skeleton::{log, LOG_ERROR, LOG_INFO};
use crate::video::{self, Action, Au, Clock, Inputs};

pub(crate) const FEED_OK: c_char = b'O' as c_char;
pub(crate) const FEED_FULL: c_char = b'B' as c_char;
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
static FEED_MAX_US: AtomicU64 = AtomicU64::new(0);
/// Audio stream time minus the PTS of the last frame shown against it, and the largest size of that.
static DRIFT_US: AtomicI64 = AtomicI64::new(0);
static DRIFT_MAX_US: AtomicI64 = AtomicI64::new(-1);

static DECODER: Mutex<Option<Decoder>> = Mutex::new(None);
static DRAIN: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static NAME: Mutex<String> = Mutex::new(String::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn now_ns() -> i64 {
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
    codec: Codec,
    inputs: Inputs,
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
        self.inputs.clear();
        self.pending = None;
        self.clock.reset();
    }

    /// Moves queued AUs into free input buffers; `None` if the decoder failed.
    fn put_inputs(&mut self) -> Option<usize> {
        let codec = &self.codec;
        self.inputs.drain(|au| match codec.queue(&au.data, au.pts_us, au.end)? {
            Input::Queued => Some(true),
            Input::Full => Some(false),
            Input::TooBig(capacity) => {
                log(LOG_ERROR, &format!("amc: a {}-byte AU is over the {capacity}-byte input buffer; skipped", au.data.len()));
                Some(true)
            }
        })
    }

    fn step(&mut self) -> Step {
        if self.put_inputs().is_none() {
            return Step::Failed;
        }
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
        let audio = audiotrack::clock();
        let now = now_ns();
        let render = match self.clock.action(pts, now, audio) {
            Action::Wait => return Step::Idle,
            Action::Show(at) => Some(at),
            Action::Restart(at) => {
                log(LOG_INFO, &format!("amc: frame pts={}ms came late just after the start; clock restarted", pts / 1_000_000));
                Some(at)
            }
            Action::Drop => None,
        };
        self.pending = None;
        if self.codec.release(index, render).is_none() {
            return Step::Failed;
        }
        let drift = render.and_then(|at| audio.drift(pts, at.max(now)));
        if let Some(drift) = drift {
            DRIFT_US.store(drift / 1_000, Relaxed);
            DRIFT_MAX_US.fetch_max((drift / 1_000).abs(), Relaxed);
        }
        let n = if render.is_some() { SHOWN.fetch_add(1, Relaxed) + 1 } else { LATE.fetch_add(1, Relaxed) + 1 };
        if n <= 3 || n.is_multiple_of(LOG_EVERY) {
            let what = if render.is_some() { "shown" } else { "late, dropped" };
            let drift = drift.map_or("-".into(), |drift| format!("{}ms", drift / 1_000_000));
            log(LOG_INFO, &format!("amc: frame {what} #{n} pts={}ms drift={drift} queued={}", pts / 1_000_000, self.inputs.queued()));
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
    audiotrack::unload();
    log(
        LOG_INFO,
        &format!(
            "amc: closed after fed={} shown={} late={} drift max={}us slowest feed={}us",
            FED.load(Relaxed),
            SHOWN.load(Relaxed),
            LATE.load(Relaxed),
            DRIFT_MAX_US.load(Relaxed),
            FEED_MAX_US.load(Relaxed)
        ),
    );
}

fn note_feed(size: c_uint, pts: i64, took_us: u64) {
    let n = FED.fetch_add(1, Relaxed) + 1;
    if n <= 5 || n.is_multiple_of(LOG_EVERY) {
        log(LOG_INFO, &format!("amc: feed video #{n} pts={}ms size={size} took={took_us}us", pts / 1_000_000));
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
        let Some(codec) = Codec::open_video(format.mime, format.width, format.height) else {
            log(LOG_ERROR, &format!("amc: {format:?} did not open"));
            return 0;
        };
        log(LOG_INFO, &format!("amc: {} started for {format:?}", codec.name));
        lock(&NAME).clone_from(&codec.name);
        *lock(&DECODER) = Some(Decoder { codec, inputs: Inputs::default(), pending: None, playing: false, clock: Clock::default(), last_fed_us: 0 });
        for count in [&FED, &SHOWN, &LATE, &FEED_MAX_US] {
            count.store(0, Relaxed);
        }
        DRIFT_US.store(0, Relaxed);
        DRIFT_MAX_US.store(-1, Relaxed);
        audiotrack::load(&payload);
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
        audiotrack::play();
        c_int::from(resumed.is_some())
    }
    unsafe fn pause(&self, _: &MainThread) -> c_int {
        let held = with_decoder(|d| {
            d.playing = false;
            d.clock.reset();
        });
        audiotrack::pause();
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
        audiotrack::flush();
        c_int::from(flushed.flatten().is_some())
    }
    unsafe fn push_eos(&self, _: &MainThread) -> c_int {
        let queued = with_decoder(|d| {
            d.inputs.push(Au { data: Vec::new(), pts_us: d.last_fed_us, end: true });
            d.put_inputs()
        });
        if queued.flatten().is_some() {
            log(LOG_INFO, "amc: end of stream queued");
            1
        } else {
            log(LOG_ERROR, "amc: end of stream not queued: the decoder failed");
            0
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
        let data = if p.is_null() { &[][..] } else { std::slice::from_raw_parts(p, size as usize) };
        if es_data != ES_VIDEO {
            return audiotrack::feed(data, pts);
        }
        let pts_us = pts / 1_000;
        let started = now_ns();
        let queued = with_decoder(|d| {
            if d.inputs.is_full() && d.put_inputs()? == 0 {
                return Some((false, d.playing));
            }
            d.inputs.push(Au { data: data.to_vec(), pts_us, end: false });
            d.last_fed_us = pts_us;
            d.put_inputs()?;
            Some((true, d.playing))
        })
        .flatten();
        let took_us = ((now_ns() - started) / 1_000).max(0) as u64;
        FEED_MAX_US.fetch_max(took_us, Relaxed);
        match queued {
            Some((true, _)) => {
                note_feed(size, pts, took_us);
                FEED_OK
            }
            Some((false, playing)) => {
                if !FULL_LOGGED.swap(true, Relaxed) {
                    let fed = FED.load(Relaxed);
                    log(LOG_INFO, &format!("amc: decoder and its queue full after {fed} video AUs (playing={playing})"));
                }
                FEED_FULL
            }
            None => FEED_ERROR,
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

/// For the label: the last frame's drift from the audio and the largest since the last Load, in
/// microseconds, or `None` before a frame was shown against audio.
pub fn drift() -> Option<(i64, i64)> {
    let max = DRIFT_MAX_US.load(Relaxed);
    (max >= 0).then(|| (DRIFT_US.load(Relaxed), max))
}
