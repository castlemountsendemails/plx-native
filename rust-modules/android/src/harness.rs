//! The video harness: the clip from the intent played through the real engine (`plx_media`) on the
//! SDL thread, the same calls the app makes for a URL-fed playback, with no Plex involved.
//! Play/Pause pauses and resumes it and D-pad left/right seek it by a fixed step.

use std::sync::atomic::{AtomicI64, Ordering::Relaxed};

use plx_base::task::MainThread;
use plx_media::player::{self, adapter::PlayerAdapter, lifecycle};
use plx_media::route::{self, PlaybackSession};

use crate::amediacodec::{self, now_ns};
use crate::audiotrack;
use crate::clip::Clip;
use crate::jni;
use crate::keys::{Edge, Key, Press};
use crate::skeleton::{log, LOG_ERROR, LOG_INFO};

pub struct Harness {
    clip: Clip,
    ps: PlaybackSession,
    pa: PlayerAdapter,
    active: bool,
    pumps: u32,
    pumps_since: u32,
}

const PUMP_LOG_MS: u32 = 10_000;
const SEEK_STEP_NS: i64 = 10_000_000_000;

/// Time spent in one place between pump reports, from any thread.
pub(crate) struct Span {
    count: AtomicI64,
    total_ns: AtomicI64,
    max_ns: AtomicI64,
}

impl Span {
    pub(crate) const fn new() -> Self {
        Self { count: AtomicI64::new(0), total_ns: AtomicI64::new(0), max_ns: AtomicI64::new(0) }
    }

    pub(crate) fn note(&self, ns: i64) {
        self.count.fetch_add(1, Relaxed);
        self.total_ns.fetch_add(ns, Relaxed);
        self.max_ns.fetch_max(ns, Relaxed);
    }

    pub(crate) fn time<T>(&self, f: impl FnOnce() -> T) -> T {
        let start = now_ns();
        let value = f();
        self.note(now_ns() - start);
        value
    }

    /// `<count>x <total>/<longest> ms`, and starts over.
    fn take(&self) -> String {
        let (count, total, max) = (self.count.swap(0, Relaxed), self.total_ns.swap(0, Relaxed), self.max_ns.swap(0, Relaxed));
        format!("{count}x {}/{} ms", total / 1_000_000, max / 1_000_000)
    }
}

pub(crate) static PUMP: Span = Span::new();
pub(crate) static LABEL: Span = Span::new();
pub(crate) static DRAW: Span = Span::new();

impl Harness {
    /// Once per process, on the SDL thread: it mints the engine's main-thread token there.
    pub fn start(clip: Clip) -> Self {
        // The engine's socket sends without MSG_NOSIGNAL; a reset peer must not kill the process.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
        jni::audio_report();
        log(
            LOG_INFO,
            &format!(
                "harness: current_exe={:?} /tmp is_dir={}",
                std::env::current_exe(),
                std::path::Path::new("/tmp").is_dir()
            ),
        );
        if plx_platform::tv::install(&crate::port::PORT).is_err() {
            log(LOG_ERROR, "harness: a port was already installed");
        }
        let mut pa = PlayerAdapter::new(unsafe { MainThread::assume() });
        player::acb_init(pa.mt());
        plx_media::ff::boot();

        let mut ps = PlaybackSession::default();
        let declared = route::set_stream_declaration(
            &mut ps,
            &clip.vc,
            &clip.ac,
            clip.fps,
            plx_data::metadata::Dovi::default(),
            false,
        );
        if let Some((w, h)) = clip.size {
            route::set_stream_source_raster(&mut ps, w, h);
        }
        let active = if declared {
            route::set_url(&mut ps, &clip.url);
            player::start_bufferfeed(&mut ps, &mut pa)
        } else {
            false
        };
        log(
            if active { LOG_INFO } else { LOG_ERROR },
            &format!(
                "harness: {} {} vc={} ac={} fps={}",
                if active { "started" } else { "could not start" },
                clip.url,
                clip.vc,
                clip.ac,
                clip.fps
            ),
        );
        Self { clip, ps, pa, active, pumps: 0, pumps_since: 0 }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Every loop iteration, playing or not: a stop's off-frame Unload is settled here.
    pub fn frame(&mut self, now_ms: u32) {
        self.pa.poll_release();
        if self.active && player::is_started() {
            PUMP.time(|| player::pump(&mut self.ps, &mut self.pa, now_ms));
            self.note_pump(now_ms);
        }
    }

    /// Logs the pump rate every [`PUMP_LOG_MS`]: the engine feeds the sink only when pumped.
    fn note_pump(&mut self, now_ms: u32) {
        if self.pumps_since == 0 {
            self.pumps_since = now_ms;
            return;
        }
        self.pumps += 1;
        let elapsed = now_ms.wrapping_sub(self.pumps_since);
        if elapsed >= PUMP_LOG_MS {
            let pumps = self.pumps;
            log(
                LOG_INFO,
                &format!(
                    "harness: {pumps} pumps in {elapsed} ms ({} ms each); pump {}, label {}, draw {}, audio feed waited {}, writer held {}",
                    elapsed / pumps,
                    PUMP.take(),
                    LABEL.take(),
                    DRAW.take(),
                    audiotrack::FEED_WAIT.take(),
                    audiotrack::HELD.take()
                ),
            );
            self.pumps = 0;
            self.pumps_since = now_ms;
        }
    }

    /// Play/Pause, Play and Pause set the transport the way the app's player does; a D-pad
    /// left/right press seeks from the current position. Repeats and other keys do nothing.
    pub fn keys(&mut self, presses: &[Press]) {
        if !self.active || !player::is_started() {
            return;
        }
        for press in presses.iter().filter(|press| press.edge == Edge::Down) {
            match press.key {
                Key::PlayPause => self.transport(None),
                Key::Play => self.transport(Some(true)),
                Key::Pause => self.transport(Some(false)),
                Key::Left { alt: false } => self.seek(-SEEK_STEP_NS),
                Key::Right { alt: false } => self.seek(SEEK_STEP_NS),
                _ => {}
            }
        }
    }

    fn transport(&mut self, play: Option<bool>) {
        let paused = lifecycle::transport_target(play, lifecycle::viewer_paused());
        let accepted = lifecycle::set_transport_paused(&mut self.pa, paused);
        let what = if paused { "pause" } else { "resume" };
        log(LOG_INFO, &format!("harness: {what} at {}ms, accepted={accepted}", player::playpos_ns() / 1_000_000));
    }

    fn seek(&mut self, by_ns: i64) {
        let from = player::playpos_ns();
        let mut to = (from + by_ns).max(0);
        let duration = player::duration_ns();
        if duration > SEEK_STEP_NS {
            to = to.min(duration - SEEK_STEP_NS);
        }
        player::request_seek(to);
        log(LOG_INFO, &format!("harness: seek from {}ms to {}ms", from / 1_000_000, to / 1_000_000));
    }

    pub fn stop(&mut self) {
        if self.active {
            player::stop_bufferfeed(&mut self.ps, &mut self.pa);
            self.active = false;
            log(LOG_INFO, "harness: stopped");
        }
    }

    pub fn label(&self) -> Vec<String> {
        let (w, h) = player::video_raster();
        let secs = player::playpos_ns().max(0) / 1_000_000_000;
        let (decoder, fed, shown, late) = amediacodec::stats();
        let decoder = if decoder.is_empty() { "-".into() } else { decoder };
        vec![
            self.clip.url.clone(),
            format!("decoder {decoder}   declared {}/{}", self.clip.vc, self.clip.ac),
            format!(
                "{w}x{h}   {}:{:02}   {:?}",
                secs / 60,
                secs % 60,
                player::state(&self.ps)
            ),
            format!("fed {fed}, shown {shown}, late {late}"),
            match amediacodec::drift() {
                Some((drift, max)) => format!("audio {}   drift {}ms, max {}ms", audiotrack::status(), drift / 1_000, max / 1_000),
                None => format!("audio {}", audiotrack::status()),
            },
        ]
    }
}
