//! A video sink that decodes nothing. It accepts every access unit, logs a sample of them with their
//! PTS, and reports a presentation position that runs at real time, clamped to the video fed so far,
//! so the engine's feed-ahead throttle keeps the demuxer reading. Lifecycle answers follow
//! `plx_media::player::ffi_host`'s clock sink, which only a `hostsim` build has.

use std::ffi::{c_char, c_int, c_long, c_uint, CStr};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use plx_base::task::MainThread;
use plx_media::player::sf_on_event;
use plx_platform::tv::sink::{UnloadTicket, VideoSink, VP_EXPORTED};

use crate::skeleton::{log, LOG_INFO};

const FEED_OK: c_char = b'O' as c_char;
const ES_VIDEO: c_int = 1;
const TICK: Duration = Duration::from_millis(100);
/// Each lane logs its first few access units, then one in this many.
const LOG_EVERY: u64 = 100;

static EPOCH: AtomicU32 = AtomicU32::new(0);
static READY: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);
static GATE_RETIRED: AtomicBool = AtomicBool::new(false);
static UNLOAD_COMPLETED: AtomicBool = AtomicBool::new(false);
static INTERCEPTS: AtomicU32 = AtomicU32::new(0);
static PLAYING: AtomicBool = AtomicBool::new(false);
static BASE_NS: AtomicI64 = AtomicI64::new(0);
static RESUMED_AT_MS: AtomicI64 = AtomicI64::new(0);
static FED_MAX_NS: AtomicI64 = AtomicI64::new(i64::MIN);
static TICKING: AtomicBool = AtomicBool::new(false);
static FED: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static LAST_PTS: [AtomicI64; 2] = [AtomicI64::new(i64::MIN), AtomicI64::new(i64::MIN)];

fn now_ms() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

fn event(ty: c_int, num: i64, text: Option<&CStr>) {
    let epoch = EPOCH.load(Relaxed);
    if epoch != 0 && !GATE_RETIRED.load(Relaxed) {
        INTERCEPTS.fetch_add(1, Relaxed);
        sf_on_event(epoch, ty, num, text.map_or(std::ptr::null(), CStr::as_ptr));
    }
}

/// Never ahead of the video fed: a starved feed stops the clock, as a real pipeline would.
fn position_ns() -> i64 {
    let base = BASE_NS.load(Relaxed);
    let fed = FED_MAX_NS.load(Relaxed);
    if fed == i64::MIN {
        return base;
    }
    let elapsed_ms = if PLAYING.load(Relaxed) {
        (now_ms() - RESUMED_AT_MS.load(Relaxed)).max(0)
    } else {
        0
    };
    base.saturating_add(elapsed_ms.saturating_mul(1_000_000)).min(fed)
}

fn hold() {
    let at = position_ns();
    if PLAYING.swap(false, Relaxed) {
        BASE_NS.store(at, Relaxed);
    }
}

fn resume() {
    if !PLAYING.swap(true, Relaxed) {
        RESUMED_AT_MS.store(now_ms(), Relaxed);
    }
    if !TICKING.swap(true, Relaxed) {
        let spawned = std::thread::Builder::new().name("logsink".into()).spawn(|| loop {
            std::thread::sleep(TICK);
            if LOADED.load(Relaxed) && PLAYING.load(Relaxed) {
                event(0, position_ns(), None);
            }
        });
        if spawned.is_err() {
            TICKING.store(false, Relaxed);
            log(LOG_INFO, "logsink: no position thread; the clock will not report");
        }
    }
}

fn rewind(base_ns: i64) {
    PLAYING.store(false, Relaxed);
    BASE_NS.store(base_ns, Relaxed);
    FED_MAX_NS.store(i64::MIN, Relaxed);
    for last in &LAST_PTS {
        last.store(i64::MIN, Relaxed);
    }
}

fn note_feed(es: c_int, size: c_uint, pts: i64) {
    let lane = usize::from(es != ES_VIDEO);
    let n = FED[lane].fetch_add(1, Relaxed) + 1;
    let last = LAST_PTS[lane].swap(pts, Relaxed);
    if n <= 5 || n.is_multiple_of(LOG_EVERY) {
        let step = if last == i64::MIN { "-".to_string() } else { format!("{:+}", (pts - last) / 1_000) };
        let name = if lane == 0 { "video" } else { "audio" };
        log(
            LOG_INFO,
            &format!("logsink: feed {name} #{n} pts={}ms step={step}us size={size}", pts / 1_000_000),
        );
    }
}

pub struct LogSink;

impl VideoSink for LogSink {
    unsafe fn load(&self, payload: *const c_char, epoch: u32) -> c_int {
        if epoch == 0 || READY.load(Relaxed) {
            return 0;
        }
        if !payload.is_null() {
            log(LOG_INFO, &format!("logsink: load epoch={epoch} {}", CStr::from_ptr(payload).to_string_lossy()));
        }
        rewind(0);
        for lane in &FED {
            lane.store(0, Relaxed);
        }
        EPOCH.store(epoch, Relaxed);
        INTERCEPTS.store(0, Relaxed);
        GATE_RETIRED.store(false, Relaxed);
        UNLOAD_COMPLETED.store(false, Relaxed);
        READY.store(true, Relaxed);
        LOADED.store(true, Relaxed);
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
        resume();
        1
    }
    unsafe fn pause(&self, _: &MainThread) -> c_int {
        hold();
        1
    }
    unsafe fn set_volume(&self, _: &MainThread, _volume: c_int) -> c_int {
        1
    }
    unsafe fn flush(&self, _: &MainThread) -> c_int {
        rewind(0);
        1
    }
    unsafe fn push_eos(&self, _: &MainThread) -> c_int {
        log(LOG_INFO, "logsink: end of stream");
        1
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
    unsafe fn seek(&self, _: &MainThread, ms: i64) -> c_int {
        if ms < 0 {
            return 0;
        }
        rewind(ms.saturating_mul(1_000_000));
        1
    }
    fn feed_abi_is_cow(&self) -> bool {
        false
    }
    unsafe fn feed(&self, _: &MainThread, _p: *const u8, size: c_uint, pts: i64, es_data: c_int) -> c_char {
        if es_data == ES_VIDEO {
            FED_MAX_NS.fetch_max(pts, Relaxed);
        }
        note_feed(es_data, size, pts);
        FEED_OK
    }
    unsafe fn unload(&self, _owner: &UnloadTicket) {
        if READY.load(Relaxed) {
            UNLOAD_COMPLETED.store(true, Relaxed);
            // UNLOADCOMPLETED, which the engine's release takes as its evidence.
            sf_on_event(EPOCH.load(Relaxed), 23, 0, std::ptr::null());
        }
        LOADED.store(false, Relaxed);
        rewind(0);
        let fed = [FED[0].load(Relaxed), FED[1].load(Relaxed)];
        log(LOG_INFO, &format!("logsink: unload after video={} audio={} access units", fed[0], fed[1]));
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
        rewind(0);
        c_int::from(clean)
    }
    unsafe fn quarantine(&self, _: &MainThread) {
        GATE_RETIRED.store(true, Relaxed);
        READY.store(false, Relaxed);
        LOADED.store(false, Relaxed);
        rewind(0);
    }
    fn window_mode(&self) -> c_int {
        // The exported-window path: the engine skips the ACB bind stages entirely.
        VP_EXPORTED
    }
    fn window_id(&self) -> &'static CStr {
        c"logsink-window"
    }
    unsafe fn create_window(&self, _: &MainThread) -> *const c_char {
        c"logsink-window".as_ptr()
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

/// Video and audio access units fed since the last Load.
pub fn fed_counts() -> (u64, u64) {
    (FED[0].load(Relaxed), FED[1].load(Relaxed))
}
