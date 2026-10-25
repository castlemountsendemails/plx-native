//! What the decoder sink decides without JNI: the decoder to open from the engine's Load payload,
//! and when each decoded frame is shown. No JNI or SDL in here, so the host unit suite tests it.

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

fn string_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text[text.find(key)? + key.len()..].strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

fn number_after(text: &str, key: &str) -> Option<i32> {
    let rest = &text[text.find(key)? + key.len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok().filter(|n| *n > 0)
}

/// How far ahead of now the first frame after a start, resume or seek is shown.
const LEAD_NS: i64 = 20_000_000;
/// A frame is handed to the display at most this long before its time, so the decoder's output
/// buffers, not the display queue, hold the backlog.
const EARLY_NS: i64 = 50_000_000;
/// A frame this late is dropped rather than shown.
const LATE_NS: i64 = 40_000_000;

/// Maps frame PTS to `CLOCK_MONOTONIC` render times (the clock of `releaseOutputBuffer`'s
/// timestamp). It anchors on the first frame after a reset and runs at real time from there.
#[derive(Default)]
pub struct Clock {
    anchor: Option<(i64, i64)>,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Wait,
    Show(i64),
    Drop,
}

impl Clock {
    pub fn reset(&mut self) {
        self.anchor = None;
    }

    pub fn action(&mut self, pts_ns: i64, now_ns: i64) -> Action {
        let (at_ns, anchor_pts) = *self.anchor.get_or_insert((now_ns + LEAD_NS, pts_ns));
        let render_ns = at_ns + (pts_ns - anchor_pts);
        if render_ns - now_ns > EARLY_NS {
            Action::Wait
        } else if now_ns - render_ns > LATE_NS {
            Action::Drop
        } else {
            Action::Show(render_ns)
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
        assert_eq!(clock.action(5_000_000_000, now), Action::Show(now + LEAD_NS));
        // 25 ms later in the stream is 25 ms later on the display, whatever time it is now.
        assert_eq!(clock.action(5_025_000_000, now), Action::Show(now + LEAD_NS + 25_000_000));
        assert_eq!(clock.action(5_041_700_000, now), Action::Wait);
        let next = now + 41_700_000;
        assert_eq!(clock.action(5_041_700_000, next), Action::Show(next + LEAD_NS));
        assert_eq!(clock.action(5_000_000_000, now + LEAD_NS + LATE_NS + 1), Action::Drop);
    }

    #[test]
    fn a_reset_anchors_on_the_next_frame() {
        let mut clock = Clock::default();
        clock.action(0, 0);
        clock.reset();
        assert_eq!(clock.action(9_000_000_000, 500), Action::Show(500 + LEAD_NS));
    }
}
