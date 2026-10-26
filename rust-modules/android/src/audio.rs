//! What the audio sink decides without JNI: how the Load payload's audio is played, the codec setup
//! read from the access units, and which stream time the track is playing. No JNI or SDL in here,
//! so the host unit suite tests it.

use crate::video::string_after;

// android.media.AudioFormat
pub const ENCODING_PCM_16BIT: i32 = 2;
pub const ENCODING_PCM_FLOAT: i32 = 4;
pub const ENCODING_AC3: i32 = 5;
pub const ENCODING_E_AC3: i32 = 6;
pub const ENCODING_DTS: i32 = 7;

/// Every AC-3 syncframe carries 1536 samples per channel.
pub const AC3_FRAMES: i64 = 1536;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plan {
    /// The AUs go to an `AudioTrack` of this encoding, which sends them out over HDMI as they are.
    Passthrough { encoding: i32 },
    /// Decoded to PCM by `MediaCodec` first.
    Aac,
    /// Mapped to its encoding to check against what HDMI reports, but its AUs are dropped.
    NotPlayed { encoding: i32 },
    /// No audio, or a codec with no route here.
    None,
}

/// The payload's `"audio"` is the engine's codec name for the clip's audio.
pub fn plan(payload: &str) -> Plan {
    match string_after(payload, "\"audio\":") {
        Some("AC3") => Plan::Passthrough { encoding: ENCODING_AC3 },
        Some("AAC") => Plan::Aac,
        Some("AC3 PLUS") => Plan::NotPlayed { encoding: ENCODING_E_AC3 },
        Some("DTS") => Plan::NotPlayed { encoding: ENCODING_DTS },
        _ => Plan::None,
    }
}

/// The sample rate an AC-3 syncframe declares (`fscod`), or `None` for a reserved code or a short AU.
pub fn ac3_rate(au: &[u8]) -> Option<i32> {
    if au.len() < 5 || au[0] != 0x0b || au[1] != 0x77 {
        return None;
    }
    match au[4] >> 6 {
        0 => Some(48_000),
        1 => Some(44_100),
        2 => Some(32_000),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub struct Adts {
    pub header_len: usize,
    pub rate: i32,
    pub channels: i32,
    /// The two-byte AudioSpecificConfig the decoder takes as `csd-0`.
    pub csd: [u8; 2],
}

const ADTS_RATES: [i32; 13] = [96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000, 7_350];

/// Reads the ADTS header the engine puts on each AAC frame. The decoder is given the raw frame and
/// this config instead, the way a demuxer hands it AAC, since an ADTS-only start without `csd-0`
/// is not something every Android AAC decoder takes.
pub fn adts(au: &[u8]) -> Option<Adts> {
    if au.len() < 7 || au[0] != 0xff || au[1] & 0xf6 != 0xf0 {
        return None;
    }
    let header_len = if au[1] & 1 == 1 { 7 } else { 9 };
    let object = (au[2] >> 6) + 1;
    let freq = (au[2] >> 2) & 0x0f;
    let config = ((au[2] & 1) << 2) | (au[3] >> 6);
    let rate = *ADTS_RATES.get(usize::from(freq))?;
    let channels = match config {
        1..=6 => i32::from(config),
        7 => 8,
        _ => return None,
    };
    let csd = [(object << 3) | (freq >> 1), ((freq & 1) << 7) | (config << 3)];
    (au.len() >= header_len).then_some(Adts { header_len, rate, channels, csd })
}

/// Which stream time each frame written to the track carries: the PTS of every AU or decoded
/// buffer at the frame count where its first frame went in, counted from the track's start.
pub struct Timeline {
    rate: i64,
    marks: std::collections::VecDeque<(i64, i64)>,
}

/// Marks older than this many are dropped; the track's own buffer holds far fewer.
const MARKS: usize = 512;

impl Timeline {
    pub fn new(rate: i32) -> Self {
        Self { rate: i64::from(rate.max(1)), marks: std::collections::VecDeque::new() }
    }

    pub fn mark(&mut self, frame: i64, pts_ns: i64) {
        if self.marks.len() == MARKS {
            self.marks.pop_front();
        }
        self.marks.push_back((frame, pts_ns));
    }

    /// The stream time of `frame`, from the last mark at or before it.
    pub fn pts_at(&self, frame: i64) -> Option<i64> {
        let &(start, pts) = self.marks.iter().rev().find(|(start, _)| *start <= frame)?;
        Some(pts + (frame - start) * 1_000_000_000 / self.rate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// `AudioTrack.getTimestamp`: a frame and the time it reached the output.
    Stamp,
    /// `getPlaybackHeadPosition` read now, which leaves out the output's latency.
    Head,
}

/// Picks the track position to trust after a play. A track's first reading after `play` can be the
/// one it held while paused, so a source counts only once its frame has moved past its first reading.
#[derive(Default)]
pub struct Position {
    stamp_from: Option<i64>,
    head_from: Option<i64>,
}

impl Position {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A frame and the `CLOCK_MONOTONIC` time it is heard, or `None` while neither source has moved.
    pub fn sample(&mut self, stamp: Option<(i64, i64)>, head: i64, now_ns: i64) -> Option<(i64, i64, Source)> {
        match stamp {
            Some((frame, at_ns)) => {
                let from = *self.stamp_from.get_or_insert(frame);
                (frame > from).then_some((frame, at_ns, Source::Stamp))
            }
            None => {
                let from = *self.head_from.get_or_insert(head);
                (head > from).then_some((head, now_ns, Source::Head))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"{"codec":{"video":"H264","audio":"AC3"},"maxWidth":1920}"#;

    #[test]
    fn the_payload_picks_how_audio_is_played() {
        assert_eq!(plan(PAYLOAD), Plan::Passthrough { encoding: ENCODING_AC3 });
        assert_eq!(plan(&PAYLOAD.replace("AC3", "AAC")), Plan::Aac);
        assert_eq!(plan(&PAYLOAD.replace("AC3", "AC3 PLUS")), Plan::NotPlayed { encoding: ENCODING_E_AC3 });
        assert_eq!(plan(&PAYLOAD.replace("AC3", "DTS")), Plan::NotPlayed { encoding: ENCODING_DTS });
        assert_eq!(plan(&PAYLOAD.replace("AC3", "OPUS")), Plan::None);
        assert_eq!(plan(""), Plan::None);
    }

    #[test]
    fn an_ac3_syncframe_names_its_rate() {
        assert_eq!(ac3_rate(&[0x0b, 0x77, 0, 0, 0x14]), Some(48_000));
        assert_eq!(ac3_rate(&[0x0b, 0x77, 0, 0, 0x54]), Some(44_100));
        assert_eq!(ac3_rate(&[0x0b, 0x77, 0, 0, 0xd4]), None);
        assert_eq!(ac3_rate(&[0x0b, 0x76, 0, 0, 0x14]), None);
        assert_eq!(ac3_rate(&[0x0b, 0x77]), None);
    }

    /// The header `ff.rs`'s `adts_header` writes, for LC with no CRC.
    fn header(freq: u8, config: u8, len: u16) -> [u8; 7] {
        let len = len + 7;
        [
            0xff,
            0xf1,
            (1 << 6) | (freq << 2) | (config >> 2),
            ((config & 3) << 6) | (len >> 11) as u8,
            (len >> 3) as u8,
            ((len & 7) << 5) as u8 | 0x1f,
            0xfc,
        ]
    }

    #[test]
    fn adts_gives_the_raw_frame_and_its_config() {
        // The 4K clip's header: 24 kHz core, channel config 7.
        let adts = adts(&header(6, 7, 100)).unwrap();
        assert_eq!((adts.header_len, adts.rate, adts.channels), (7, 24_000, 8));
        // AAC LC (2), index 6, config 7: 00010 0110 0111 000
        assert_eq!(adts.csd, [0x13, 0x38]);
        let stereo = super::adts(&header(3, 2, 10)).unwrap();
        assert_eq!((stereo.rate, stereo.channels, stereo.csd), (48_000, 2, [0x11, 0x90]));
    }

    #[test]
    fn a_crc_lengthens_the_header_and_junk_is_refused() {
        let mut crc = header(3, 2, 10).to_vec();
        crc[1] = 0xf0;
        crc.extend([0, 0]);
        assert_eq!(adts(&crc).unwrap().header_len, 9);
        assert_eq!(adts(&header(15, 2, 10)), None, "reserved rate index");
        assert_eq!(adts(&header(3, 0, 10)), None, "config in a PCE");
        assert_eq!(adts(&[0x0b, 0x77, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn a_frame_takes_the_time_of_the_mark_it_follows() {
        let mut timeline = Timeline::new(48_000);
        assert_eq!(timeline.pts_at(0), None);
        timeline.mark(0, 5_000_000_000);
        timeline.mark(AC3_FRAMES, 5_032_000_000);
        // A gap in the stream: the next AU is 100 ms on, not 32.
        timeline.mark(2 * AC3_FRAMES, 5_164_000_000);
        assert_eq!(timeline.pts_at(0), Some(5_000_000_000));
        assert_eq!(timeline.pts_at(480), Some(5_010_000_000));
        assert_eq!(timeline.pts_at(AC3_FRAMES + 480), Some(5_042_000_000));
        assert_eq!(timeline.pts_at(2 * AC3_FRAMES + 48), Some(5_165_000_000));
    }

    #[test]
    fn a_reading_counts_only_once_it_moves() {
        let mut position = Position::default();
        // The reading held over from before a pause.
        assert_eq!(position.sample(Some((9600, 100)), 0, 1_000), None);
        assert_eq!(position.sample(Some((9600, 100)), 0, 2_000), None);
        assert_eq!(position.sample(Some((10_080, 3_000)), 0, 3_500), Some((10_080, 3_000, Source::Stamp)));
        position.reset();
        assert_eq!(position.sample(Some((10_080, 3_000)), 0, 4_000), None);
    }

    #[test]
    fn with_no_timestamp_the_head_position_is_read_now() {
        let mut position = Position::default();
        assert_eq!(position.sample(None, 0, 1_000), None);
        assert_eq!(position.sample(None, 480, 2_000), Some((480, 2_000, Source::Head)));
    }
}
