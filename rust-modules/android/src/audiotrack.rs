//! The Fire TV audio sink: the engine's audio AUs played by an `AudioTrack`, AC-3 as it is (HDMI
//! passthrough) and AAC after a `MediaCodec` decodes it to PCM. `feed` queues AUs on the engine
//! thread; a writer thread moves them into the track without blocking and publishes which stream
//! time the track is playing, which the video drain times its frames against.

use std::collections::VecDeque;
use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::amediacodec::{now_ns, FEED_FULL, FEED_OK};
use crate::audio::{self, Plan, Position, Source, Timeline, AC3_FRAMES, ENCODING_AC3, ENCODING_DTS, ENCODING_E_AC3, ENCODING_PCM_FLOAT};
use crate::harness::Span;
use crate::jni::{self, Codec, Input, Output, Track};
use crate::entry::{log, LOG_ERROR, LOG_INFO};
use crate::video::Audio;

/// AUs queued ahead of the track: over a second of AC-3 or AAC, on top of the track's own buffer.
const QUEUE: usize = 48;
const IDLE: Duration = Duration::from_millis(4);
const POLL_NS: i64 = 20_000_000;
const LOG_EVERY_NS: i64 = 5_000_000_000;
/// The first few position readings after each play are logged, to show how the track's clock starts.
const LOG_FIRST_POLLS: u32 = 6;
/// AC-3 is opened as 5.1; Java falls back to stereo if the track refuses that.
const AC3_CHANNELS: i32 = 6;

static OUT: Mutex<Option<Out>> = Mutex::new(None);
/// The stream time the track plays and the `CLOCK_MONOTONIC` time it plays it.
static CLOCK: Mutex<Option<(i64, i64)>> = Mutex::new(None);
/// Whether audio is played, so frames wait for its clock.
static EXPECTED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static WRITER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static STATUS: Mutex<String> = Mutex::new(String::new());
static FED: AtomicU64 = AtomicU64::new(0);
static WRITTEN: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
/// How long `feed` waits for `OUT`, and how long the writer holds it per pass.
pub(crate) static FEED_WAIT: Span = Span::new();
pub(crate) static HELD: Span = Span::new();

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish(clock: Option<(i64, i64)>) {
    *lock(&CLOCK) = clock;
}

struct Chunk {
    data: Vec<u8>,
    at: usize,
    pts: i64,
    marked: bool,
}

struct Out {
    plan: Plan,
    decoder: Option<Codec>,
    /// The encoding, rate and channels of the track, from the first AC-3 AU or the decoder's output.
    format: Option<(i32, i32, i32)>,
    track: Option<Track>,
    queue: VecDeque<(Vec<u8>, i64)>,
    chunk: Option<Chunk>,
    timeline: Timeline,
    /// Frames written since the track was opened; for PCM, `bytes` over the frame size.
    frames: i64,
    bytes: i64,
    position: Position,
    source: Option<Source>,
    playing: bool,
    polls: u32,
    polled_ns: i64,
    logged_ns: i64,
}

impl Out {
    fn new(plan: Plan) -> Self {
        Self {
            plan,
            decoder: None,
            format: None,
            track: None,
            queue: VecDeque::new(),
            chunk: None,
            timeline: Timeline::new(1),
            frames: 0,
            bytes: 0,
            position: Position::default(),
            source: None,
            playing: false,
            polls: 0,
            polled_ns: 0,
            logged_ns: 0,
        }
    }

    fn playable(&self) -> bool {
        matches!(self.plan, Plan::Passthrough { .. } | Plan::Aac)
    }

    /// Plays the rest of the clip without sound, the video on its own clock.
    fn fail(&mut self, why: &str) {
        log(LOG_ERROR, &format!("audio: {why}; the clip plays without sound"));
        *lock(&STATUS) = why.into();
        self.plan = Plan::None;
        self.queue.clear();
        self.chunk = None;
        self.close();
        EXPECTED.store(false, Relaxed);
    }

    fn close(&mut self) {
        if let Some(decoder) = self.decoder.take() {
            decoder.close();
        }
        if let Some(track) = self.track.take() {
            track.close();
        }
        publish(None);
    }

    /// One pass of the writer; false when there was nothing to do.
    fn step(&mut self, now: i64) -> bool {
        let busy = match self.plan {
            Plan::Aac => match self.decode() {
                Ok(busy) => busy,
                Err(why) => {
                    self.fail(why);
                    return false;
                }
            },
            Plan::Passthrough { encoding } => self.next_au(encoding),
            _ => return false,
        };
        if self.track.is_none() {
            if let Some((encoding, rate, channels)) = self.format {
                self.open(encoding, rate, channels);
            }
        }
        let wrote = self.write();
        if self.playing && now - self.polled_ns >= POLL_NS {
            self.poll(now);
        }
        busy || wrote
    }

    fn next_au(&mut self, encoding: i32) -> bool {
        if self.chunk.is_some() {
            return false;
        }
        let Some((data, pts)) = self.queue.pop_front() else {
            return false;
        };
        if self.format.is_none() {
            let rate = audio::ac3_rate(&data).unwrap_or_else(|| {
                log(LOG_ERROR, "audio: the first AU is not an AC-3 syncframe; opening the track at 48 kHz");
                48_000
            });
            self.format = Some((encoding, rate, AC3_CHANNELS));
        }
        self.chunk = Some(Chunk { data, at: 0, pts, marked: false });
        true
    }

    /// Feeds queued AUs to the AAC decoder and takes its next decoded buffer.
    fn decode(&mut self) -> Result<bool, &'static str> {
        if self.decoder.is_none() {
            let Some((au, _)) = self.queue.front() else {
                return Ok(false);
            };
            let adts = audio::adts(au).ok_or("the first AAC AU has no ADTS header")?;
            let decoder = Codec::open_aac(adts.rate, adts.channels, &adts.csd).ok_or("no AAC decoder opened")?;
            log(LOG_INFO, &format!("audio: {} started for {adts:?}", decoder.name));
            self.decoder = Some(decoder);
        }
        let Some(decoder) = self.decoder.as_ref() else {
            return Ok(false);
        };
        let mut busy = false;
        while let Some((au, pts)) = self.queue.front() {
            let raw = audio::adts(au).map_or(&au[..], |adts| &au[adts.header_len..]);
            match decoder.queue(raw, pts / 1_000, false).ok_or("the AAC decoder failed")? {
                Input::Queued => {}
                Input::Full => break,
                Input::TooBig(capacity) => log(LOG_ERROR, &format!("audio: a {}-byte AU is over the {capacity}-byte input buffer", raw.len())),
            }
            self.queue.pop_front();
            busy = true;
        }
        if self.chunk.is_some() {
            return Ok(busy);
        }
        match decoder.dequeue().ok_or("the AAC decoder failed")? {
            Output::None => {}
            Output::FormatChanged => {
                let (rate, channels, encoding) = decoder.audio_format().ok_or("the AAC output format was unreadable")?;
                let format = Some((encoding, rate, channels));
                if format != self.format {
                    log(LOG_INFO, &format!("audio: decoding to {rate} Hz, {channels} channels, encoding {encoding}"));
                    if let Some(track) = self.track.take() {
                        track.close();
                    }
                    self.format = format;
                }
                busy = true;
            }
            Output::Frame { index, pts_us, end } => {
                let data = decoder.output(index);
                decoder.release(index, None).ok_or("the AAC decoder failed")?;
                if let Some(data) = data.filter(|data| !data.is_empty() && !end) {
                    self.chunk = Some(Chunk { data, at: 0, pts: pts_us * 1_000, marked: false });
                }
                busy = true;
            }
        }
        Ok(busy)
    }

    fn open(&mut self, encoding: i32, rate: i32, channels: i32) {
        let Some(track) = Track::open(encoding, rate, channels) else {
            let why = if encoding == ENCODING_AC3 { "the AC-3 track did not open" } else { "the PCM track did not open" };
            self.fail(why);
            return;
        };
        if self.playing {
            track.play();
        }
        self.track = Some(track);
        self.timeline = Timeline::new(rate);
        self.frames = 0;
        self.bytes = 0;
        self.position.reset();
        self.polls = 0;
        log(LOG_INFO, &format!("audio: track open, encoding {encoding}, {rate} Hz, {channels} channels"));
    }

    fn frame_bytes(&self) -> i64 {
        match self.format {
            Some((ENCODING_PCM_FLOAT, _, channels)) => 4 * i64::from(channels),
            Some((_, _, channels)) => 2 * i64::from(channels),
            None => 1,
        }
    }

    fn write(&mut self) -> bool {
        let pcm = self.plan == Plan::Aac;
        let frame_bytes = self.frame_bytes();
        let (Some(track), Some(chunk)) = (self.track.as_ref(), self.chunk.as_mut()) else {
            return false;
        };
        if !chunk.marked {
            self.timeline.mark(self.frames, chunk.pts);
            chunk.marked = true;
        }
        let Some(n) = track.write(&chunk.data[chunk.at..]) else {
            log(LOG_ERROR, &format!("audio: the track refused the AU at {}ms; it is skipped", chunk.pts / 1_000_000));
            self.chunk = None;
            return true;
        };
        chunk.at += n;
        if pcm {
            self.bytes += n as i64;
            self.frames = self.bytes / frame_bytes;
        }
        if chunk.at < chunk.data.len() {
            return n > 0;
        }
        if !pcm {
            self.frames += AC3_FRAMES;
        }
        self.chunk = None;
        WRITTEN.fetch_add(1, Relaxed);
        true
    }

    fn poll(&mut self, now: i64) {
        self.polled_ns = now;
        let Some(track) = self.track.as_ref() else {
            return;
        };
        let stamp = track.timestamp().flatten();
        let head = track.head().unwrap_or(0);
        if let Some((frame, at, source)) = self.position.sample(stamp, head, self.frames, now) {
            if let Some(pts) = self.timeline.pts_at(frame) {
                publish(Some((pts, at)));
                if self.source != Some(source) {
                    log(LOG_INFO, &format!("audio: clock from {source:?}"));
                    self.source = Some(source);
                }
            }
        }
        self.polls += 1;
        if self.polls <= LOG_FIRST_POLLS || now - self.logged_ns >= LOG_EVERY_NS {
            self.logged_ns = now;
            let clock = lock(&CLOCK).map(|(pts, at)| format!("{}ms at now{:+}ms", pts / 1_000_000, (at - now) / 1_000_000));
            log(
                LOG_INFO,
                &format!(
                    "audio: poll #{} stamp={stamp:?} head={head} written={} frames clock={} underruns={}",
                    self.polls,
                    self.frames,
                    clock.unwrap_or_else(|| "-".into()),
                    track.underruns().unwrap_or(-1)
                ),
            );
        }
    }
}

fn run() {
    while RUNNING.load(Relaxed) {
        let busy = {
            let mut out = lock(&OUT);
            let start = now_ns();
            let busy = out.as_mut().is_some_and(|out| out.step(start));
            HELD.note(now_ns() - start);
            busy
        };
        if !busy {
            std::thread::sleep(IDLE);
        }
    }
}

fn encoding_name(encoding: i32) -> &'static str {
    match encoding {
        ENCODING_AC3 => "AC-3",
        ENCODING_E_AC3 => "E-AC-3",
        ENCODING_DTS => "DTS",
        _ => "?",
    }
}

fn listed(takes: i32) -> &'static str {
    match takes {
        1 => "yes",
        0 => "no",
        _ => "unknown",
    }
}

/// Decides from the Load payload how its audio is played and starts the writer if it is.
pub fn load(payload: &str) {
    unload();
    for count in [&FED, &WRITTEN, &DROPPED] {
        count.store(0, Relaxed);
    }
    let mut plan = audio::plan(payload);
    let status = match plan {
        Plan::Passthrough { encoding } => {
            let takes = jni::sink_takes(encoding);
            if takes == 0 {
                plan = Plan::None;
                format!("sink has no {}", encoding_name(encoding))
            } else {
                format!("{} passthrough (HDMI lists it: {})", encoding_name(encoding), listed(takes))
            }
        }
        Plan::Aac => "AAC to PCM".into(),
        Plan::NotPlayed { encoding } => {
            format!("{} not played (HDMI lists it: {})", encoding_name(encoding), listed(jni::sink_takes(encoding)))
        }
        Plan::None => "none".into(),
    };
    log(LOG_INFO, &format!("audio: {status}"));
    lock(&STATUS).clone_from(&status);
    let out = Out::new(plan);
    if !out.playable() {
        return;
    }
    *lock(&OUT) = Some(out);
    EXPECTED.store(true, Relaxed);
    RUNNING.store(true, Relaxed);
    match std::thread::Builder::new().name("amc-audio".into()).spawn(run) {
        Ok(writer) => *lock(&WRITER) = Some(writer),
        Err(e) => {
            log(LOG_ERROR, &format!("audio: no writer thread ({e}); the clip plays without sound"));
            unload();
        }
    }
}

pub fn feed(data: &[u8], pts: i64) -> c_char {
    let mut out = FEED_WAIT.time(|| lock(&OUT));
    let Some(out) = out.as_mut().filter(|out| out.playable()) else {
        DROPPED.fetch_add(1, Relaxed);
        return FEED_OK;
    };
    if out.queue.len() >= QUEUE {
        return FEED_FULL;
    }
    out.queue.push_back((data.to_vec(), pts));
    let n = FED.fetch_add(1, Relaxed) + 1;
    if n <= 3 {
        log(LOG_INFO, &format!("audio: feed #{n} pts={}ms size={}", pts / 1_000_000, data.len()));
    }
    FEED_OK
}

pub fn play() {
    if let Some(out) = lock(&OUT).as_mut() {
        out.playing = true;
        out.position.reset();
        out.polls = 0;
        if let Some(track) = out.track.as_ref() {
            track.play();
        }
    }
    publish(None);
}

pub fn pause() {
    if let Some(out) = lock(&OUT).as_mut() {
        out.playing = false;
        if let Some(track) = out.track.as_ref() {
            track.pause();
        }
    }
    publish(None);
}

/// Drops everything queued and the track itself, which the writer opens again for the next AU:
/// a new track counts its frames from zero, which is what the timeline is marked in. A new AAC
/// decoder starts from the next AU's header too.
pub fn flush() {
    if let Some(out) = lock(&OUT).as_mut() {
        out.queue.clear();
        out.chunk = None;
        out.close();
    }
}

pub fn unload() {
    RUNNING.store(false, Relaxed);
    if let Some(writer) = lock(&WRITER).take() {
        let _ = writer.join();
    }
    let underruns = lock(&OUT).take().map(|mut out| {
        let underruns = out.track.as_ref().and_then(Track::underruns);
        out.close();
        underruns
    });
    EXPECTED.store(false, Relaxed);
    publish(None);
    if let Some(underruns) = underruns {
        log(
            LOG_INFO,
            &format!(
                "audio: closed after fed={} written={} dropped={} underruns={}",
                FED.load(Relaxed),
                WRITTEN.load(Relaxed),
                DROPPED.load(Relaxed),
                underruns.unwrap_or(-1)
            ),
        );
    }
}

/// The audio position frames are timed against.
pub fn clock() -> Audio {
    if !EXPECTED.load(Relaxed) {
        return Audio::None;
    }
    match *lock(&CLOCK) {
        Some((pts_ns, at_ns)) => Audio::At { pts_ns, at_ns },
        None => Audio::Pending,
    }
}

/// For the label: how the clip's audio is played and where its clock comes from.
pub fn status() -> String {
    let status = lock(&STATUS).clone();
    match lock(&OUT).as_ref().and_then(|out| out.source) {
        Some(source) => format!("{status}, clock {source:?}"),
        None => status,
    }
}
