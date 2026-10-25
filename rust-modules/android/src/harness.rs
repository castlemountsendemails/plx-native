//! The video harness: the clip from the intent played through the real engine (`plx_media`) on the
//! SDL thread, the same calls the app makes for a URL-fed playback, with no Plex involved.

use plx_base::task::MainThread;
use plx_media::player::{self, adapter::PlayerAdapter};
use plx_media::route::{self, PlaybackSession};

use crate::amediacodec;
use crate::clip::Clip;
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

impl Harness {
    /// Once per process, on the SDL thread: it mints the engine's main-thread token there.
    pub fn start(clip: Clip) -> Self {
        // The engine's socket sends without MSG_NOSIGNAL; a reset peer must not kill the process.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
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
            player::pump(&mut self.ps, &mut self.pa, now_ms);
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
            log(LOG_INFO, &format!("harness: {pumps} pumps in {elapsed} ms ({} ms each)", elapsed / pumps));
            self.pumps = 0;
            self.pumps_since = now_ms;
        }
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
        ]
    }
}
