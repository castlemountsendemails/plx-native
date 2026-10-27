//! What the decoder sink decides without JNI: the decoder to open from the engine's Load payload,
//! the access units queued ahead of it, and when each decoded frame is shown. No JNI or SDL in
//! here, so the host unit suite tests it.

use std::collections::VecDeque;

/// The decoder `PlxActivity.openVideoDecoder` opens: a MIME type and the largest raster it must take.
#[derive(Debug, PartialEq)]
pub struct Format {
    pub mime: &'static str,
    pub width: i32,
    pub height: i32,
}

/// The payload is the engine's webOS BUFFERSTREAM JSON: `"video":"H264"|"H265"` names the codec and
/// `"maxWidth"`/`"maxHeight"` the largest raster. Other codecs have no decoder here.
pub fn format(payload: &str) -> Option<Format> {
    let mime = match string_after(payload, "\"video\":")? {
        "H264" => "video/avc",
        "H265" => "video/hevc",
        _ => return None,
    };
    Some(Format {
        mime,
        width: number_after(payload, "\"maxWidth\":")?,
        height: number_after(payload, "\"maxHeight\":")?,
    })
}

pub(crate) fn string_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text[text.find(key)? + key.len()..].strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

fn number_after(text: &str, key: &str) -> Option<i32> {
    let rest = &text[text.find(key)? + key.len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok().filter(|n| *n > 0)
}

/// AUs accepted past the decoder's input buffers, about 2 s at 24 fps. Paused, a 4K HEVC decoder
/// takes only about 0.55 s, short of the 0.7 s the engine waits for before it plays again.
const QUEUE: usize = 48;

/// An access unit waiting for a decoder input buffer; `end` marks end of stream.
pub struct Au {
    pub data: Vec<u8>,
    pub pts_us: i64,
    pub end: bool,
}

#[derive(Default)]
pub struct Inputs {
    queue: VecDeque<Au>,
}

impl Inputs {
    pub fn is_full(&self) -> bool {
        self.queue.len() >= QUEUE
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Unbounded: the caller checks `is_full`, so end of stream can always be queued.
    pub fn push(&mut self, au: Au) {
        self.queue.push_back(au);
    }

    /// Hands queued AUs to `put` in order until it answers `Some(false)` (no free input buffer) or
    /// `None` (the decoder failed, which keeps that AU). Returns how many it took.
    pub fn drain(&mut self, mut put: impl FnMut(&Au) -> Option<bool>) -> Option<usize> {
        let mut taken = 0;
        while let Some(au) = self.queue.front() {
            if !put(au)? {
                break;
            }
            self.queue.pop_front();
            taken += 1;
        }
        Some(taken)
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

/// How far ahead of now the first frame after a start, resume or seek is shown.
const LEAD_NS: i64 = 20_000_000;
/// A frame is handed to the display at most this long before its time, so the decoder's output
/// buffers, not the display queue, hold the backlog.
const EARLY_NS: i64 = 50_000_000;
/// A frame this late is dropped rather than shown.
const LATE_NS: i64 = 40_000_000;
/// For this long after a reset, a late frame restarts the clock instead: a decoder can stall for
/// most of a second after its first frames, and dropping them would skip that stretch of video.
const SETTLE_NS: i64 = 1_000_000_000;
/// How long frames wait for an audio clock that is expected but has not started before they run
/// on their own clock.
const AUDIO_WAIT_NS: i64 = 1_500_000_000;

/// What the audio track says about time when a frame is due.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Audio {
    /// No audio is played: frames run on their own clock.
    None,
    /// Audio is played but its position has not started moving yet.
    Pending,
    /// The stream time the track plays at `at_ns` (`CLOCK_MONOTONIC`).
    At { pts_ns: i64, at_ns: i64 },
}

impl Audio {
    /// Audio stream time minus the PTS of a frame on screen from `shown_ns`.
    pub fn drift(self, pts_ns: i64, shown_ns: i64) -> Option<i64> {
        match self {
            Audio::At { pts_ns: audio, at_ns } => Some(audio + (shown_ns - at_ns) - pts_ns),
            _ => None,
        }
    }
}

/// Maps frame PTS to `CLOCK_MONOTONIC` render times (the clock of `releaseOutputBuffer`'s
/// timestamp). With an audio position each frame is timed against it; without one the clock
/// anchors on the first frame after a reset and runs at real time from there.
#[derive(Default)]
pub struct Clock {
    anchor: Option<(i64, i64)>,
    started_ns: Option<i64>,
    waiting_since: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Wait,
    Show(i64),
    /// Shown at this time, with the clock restarted on this frame.
    Restart(i64),
    Drop,
}

impl Clock {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn action(&mut self, pts_ns: i64, now_ns: i64, audio: Audio) -> Action {
        match audio {
            Audio::At { pts_ns: audio_pts, at_ns } => {
                let render_ns = at_ns + (pts_ns - audio_pts);
                return if render_ns - now_ns > EARLY_NS {
                    Action::Wait
                } else if now_ns - render_ns <= LATE_NS {
                    Action::Show(render_ns)
                } else {
                    Action::Drop
                };
            }
            Audio::Pending if now_ns - *self.waiting_since.get_or_insert(now_ns) < AUDIO_WAIT_NS => {
                return Action::Wait;
            }
            _ => {}
        }
        let (at_ns, anchor_pts) = *self.anchor.get_or_insert((now_ns + LEAD_NS, pts_ns));
        let started_ns = *self.started_ns.get_or_insert(at_ns);
        let render_ns = at_ns + (pts_ns - anchor_pts);
        if render_ns - now_ns > EARLY_NS {
            Action::Wait
        } else if now_ns - render_ns <= LATE_NS {
            Action::Show(render_ns)
        } else if now_ns - started_ns < SETTLE_NS {
            self.anchor = Some((now_ns + LEAD_NS, pts_ns));
            Action::Restart(now_ns + LEAD_NS)
        } else {
            Action::Drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H264: &str = r#"{"args":[{"option":{"externalStreamingInfo":{"contents":{"codec":{"video":"H264","audio":"AC3"}}},"adaptiveStreaming":{"audioOnly":false,"maxWidth":1920,"maxHeight":1080,"maxFrameRate":30}}}]}"#;

    #[test]
    fn the_payload_names_the_decoder_and_its_raster() {
        assert_eq!(format(H264), Some(Format { mime: "video/avc", width: 1920, height: 1080 }));
        let hevc = H264.replace("H264", "H265").replace("1920", "3840").replace("1080", "2160");
        assert_eq!(format(&hevc), Some(Format { mime: "video/hevc", width: 3840, height: 2160 }));
    }

    #[test]
    fn an_unknown_codec_or_a_missing_raster_opens_nothing() {
        assert_eq!(format(&H264.replace("H264", "VP9")), None);
        assert_eq!(format(&H264.replace("\"maxHeight\":1080", "")), None);
        assert_eq!(format(""), None);
    }

    #[test]
    fn frames_run_at_real_time_from_the_first() {
        let mut clock = Clock::default();
        let now = 1_000_000_000;
        assert_eq!(clock.action(5_000_000_000, now, Audio::None), Action::Show(now + LEAD_NS));
        // 25 ms later in the stream is 25 ms later on the display, whatever time it is now.
        assert_eq!(clock.action(5_025_000_000, now, Audio::None), Action::Show(now + LEAD_NS + 25_000_000));
        assert_eq!(clock.action(5_041_700_000, now, Audio::None), Action::Wait);
        let next = now + 41_700_000;
        assert_eq!(clock.action(5_041_700_000, next, Audio::None), Action::Show(next + LEAD_NS));
        let after_settling = now + LEAD_NS + SETTLE_NS;
        assert_eq!(clock.action(5_000_000_000, after_settling, Audio::None), Action::Drop);
    }

    #[test]
    fn a_stall_just_after_the_start_restarts_the_clock_instead_of_dropping() {
        let mut clock = Clock::default();
        clock.action(0, 0, Audio::None);
        // The decoder's third frame (83 ms) arrives 700 ms after the first.
        let stalled = 700_000_000;
        assert_eq!(clock.action(83_000_000, stalled, Audio::None), Action::Restart(stalled + LEAD_NS));
        assert_eq!(clock.action(108_000_000, stalled, Audio::None), Action::Show(stalled + LEAD_NS + 25_000_000));
    }

    #[test]
    fn a_restart_does_not_extend_the_settling_window() {
        let mut clock = Clock::default();
        clock.action(0, 0, Audio::None);
        assert_eq!(clock.action(83_000_000, 900_000_000, Audio::None), Action::Restart(900_000_000 + LEAD_NS));
        // Late again 1.5 s after the first start: dropped, though only 0.6 s after the restart.
        assert_eq!(clock.action(125_000_000, 1_500_000_000, Audio::None), Action::Drop);
    }

    #[test]
    fn with_audio_each_frame_is_timed_against_it() {
        let mut clock = Clock::default();
        let now = 1_000_000_000;
        // The track plays stream time 5.000 s at `now`.
        let audio = Audio::At { pts_ns: 5_000_000_000, at_ns: now };
        assert_eq!(clock.action(5_030_000_000, now, audio), Action::Show(now + 30_000_000));
        assert_eq!(clock.action(5_100_000_000, now, audio), Action::Wait);
        assert_eq!(clock.action(4_970_000_000, now, audio), Action::Show(now - 30_000_000));
        // Well behind the audio: dropped, even just after a start.
        assert_eq!(clock.action(4_900_000_000, now, audio), Action::Drop);
        assert_eq!(audio.drift(5_030_000_000, now + 30_000_000), Some(0));
        assert_eq!(audio.drift(4_970_000_000, now), Some(30_000_000));
        assert_eq!(Audio::Pending.drift(0, 0), None);
    }

    #[test]
    fn frames_wait_for_expected_audio_then_run_alone() {
        let mut clock = Clock::default();
        assert_eq!(clock.action(0, 0, Audio::Pending), Action::Wait);
        assert_eq!(clock.action(0, AUDIO_WAIT_NS - 1, Audio::Pending), Action::Wait);
        assert_eq!(clock.action(0, AUDIO_WAIT_NS, Audio::Pending), Action::Show(AUDIO_WAIT_NS + LEAD_NS));
        clock.reset();
        assert_eq!(clock.action(0, 5 * AUDIO_WAIT_NS, Audio::Pending), Action::Wait);
    }

    fn au(pts_us: i64) -> Au {
        Au { data: vec![0; 8], pts_us, end: false }
    }

    #[test]
    fn a_full_decoder_still_leaves_the_engine_room_to_prime() {
        // The engine plays again only once 0.7 s past the shown frame is accepted.
        let mut inputs = Inputs::default();
        let frame_us = 41_708;
        let mut n = 0;
        while !inputs.is_full() {
            inputs.push(au(n * frame_us));
            n += 1;
        }
        assert!((n - 1) * frame_us >= 700_000, "only {n} AUs of 24 fps video fit");
        assert_eq!(inputs.drain(|_| Some(false)), Some(0));
        assert_eq!(inputs.queued() as i64, n);
    }

    #[test]
    fn queued_aus_reach_the_decoder_in_order_as_it_frees_buffers() {
        let mut inputs = Inputs::default();
        for pts in [0, 83_000, 41_000] {
            inputs.push(au(pts));
        }
        let mut taken = Vec::new();
        let mut room = 1;
        assert_eq!(
            inputs.drain(|au| {
                if room == 0 {
                    return Some(false);
                }
                room -= 1;
                taken.push(au.pts_us);
                Some(true)
            }),
            Some(1)
        );
        assert_eq!(inputs.drain(|au| Some(taken.push(au.pts_us) == ())), Some(2));
        assert_eq!(taken, [0, 83_000, 41_000]);
    }

    #[test]
    fn end_of_stream_is_taken_when_full_and_reaches_the_decoder_last() {
        let mut inputs = Inputs::default();
        while !inputs.is_full() {
            inputs.push(au(0));
        }
        inputs.push(Au { data: Vec::new(), pts_us: 0, end: true });
        let mut ends = Vec::new();
        inputs.drain(|au| Some(ends.push(au.end) == ()));
        assert_eq!(ends.last(), Some(&true));
        assert_eq!(ends.iter().filter(|end| **end).count(), 1);
    }

    #[test]
    fn a_failed_decoder_keeps_the_au_and_a_flush_drops_them_all() {
        let mut inputs = Inputs::default();
        inputs.push(au(0));
        inputs.push(au(41_000));
        assert_eq!(inputs.drain(|_| None), None);
        assert_eq!(inputs.queued(), 2);
        inputs.clear();
        assert_eq!(inputs.queued(), 0);
    }

    #[test]
    fn a_reset_anchors_on_the_next_frame() {
        let mut clock = Clock::default();
        clock.action(0, 0, Audio::None);
        clock.reset();
        assert_eq!(clock.action(9_000_000_000, 500, Audio::None), Action::Show(500 + LEAD_NS));
    }
}
