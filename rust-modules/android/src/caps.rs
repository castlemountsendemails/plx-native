//! The stick's codec capability table, in the JSON shape of webOS's `device_codec_capability_config.json`
//! so `devcaps` parses it by the one set of merge and reject rules. `table` turns the lines
//! `PlxActivity.codecFacts` reads into that JSON; no JNI in it, so the host unit suite tests it.

use crate::audio::{ENCODING_AC3, ENCODING_DTS, ENCODING_E_AC3};

/// `facts` is `PlxActivity.codecFacts`'s lines: `video h264|hevc WIDTH HEIGHT FPS` per sink decoder,
/// `aac CHANNELS` and `hdmi ENCODING...`. A decoder that is not there has no row, and `devcaps` rejects
/// a table without H.264 whole. Passthrough codecs are listed only when HDMI names the encoding: an
/// output that lists none is unknown, and an unknown sink gets its audio converted by the server.
pub fn table(facts: &str) -> String {
    let (mut video, mut aac, mut hdmi) = (Vec::new(), 0, Vec::new());
    for line in facts.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some(kind @ ("video" | "aac" | "hdmi")) => {
                let name = if kind == "video" { words.next() } else { None };
                let numbers: Vec<u32> = words.filter_map(|word| word.parse().ok()).collect();
                match (kind, name, numbers.as_slice()) {
                    ("video", Some("h264"), &[w, h, fps]) => video.push(video_row("H.264", w, h, fps)),
                    ("video", Some("hevc"), &[w, h, fps]) => video.push(video_row("HEVC", w, h, fps)),
                    ("aac", _, &[channels]) => aac = channels,
                    ("hdmi", _, encodings) => hdmi = encodings.to_vec(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let takes = |encoding: i32| hdmi.contains(&(encoding as u32));
    let mut audio = vec![audio_row("AAC", aac)];
    if takes(ENCODING_AC3) {
        audio.push(audio_row("AC3", 0));
        if takes(ENCODING_E_AC3) {
            audio.push(audio_row("EAC3", 0));
        }
    }
    if takes(ENCODING_DTS) {
        audio.push(audio_row("DTS", DTS_CHANNELS));
    }
    format!(r#"{{"videoCodecs":[{}],"audioCodecs":[{}]}}"#, video.join(","), audio.join(","))
}

/// DTS core is 5.1; `devcaps` lists DTS only with a stated limit.
const DTS_CHANNELS: u32 = 6;

fn video_row(name: &str, width: u32, height: u32, fps: u32) -> String {
    format!(r#"{{"name":"{name}","maxWidth":{width},"maxHeight":{height},"maxFrameRate":{fps}}}"#)
}

/// `channels` 0 states no limit.
fn audio_row(name: &str, channels: u32) -> String {
    match channels {
        0 => format!(r#"{{"name":"{name}"}}"#),
        _ => format!(r#"{{"name":"{name}","channels":{channels}}}"#),
    }
}

/// The table as the stick reads it now, for `devcaps::set_source`.
#[cfg(target_os = "android")]
pub fn source() -> Option<String> {
    crate::jni::codec_facts().map(|facts| table(&facts))
}

/// The capability harness (`--es caps dump`): every relevant decoder and the audio outputs to
/// logcat, then the facts, the table and the capabilities derived from them, as label lines.
#[cfg(target_os = "android")]
pub fn harness(mode: &str) -> Vec<String> {
    use crate::entry::{log, LOG_INFO};

    if mode != "dump" {
        return vec![format!("caps={mode}: only 'dump' is known")];
    }
    crate::jni::dump_codecs();
    let facts = crate::jni::codec_facts();
    let json = facts.as_deref().map(table);
    log(LOG_INFO, &format!("caps: facts {facts:?}"));
    log(LOG_INFO, &format!("caps: table {json:?}"));
    let changed = plx_platform::devcaps::refresh();
    let caps = plx_platform::devcaps::caps();
    log(LOG_INFO, &format!("caps: measured {} changed {changed} {caps:?}", plx_platform::devcaps::measured()));
    vec![
        "caps dump".into(),
        format!("measured {}   hevc {}   {}x{}", plx_platform::devcaps::measured(), caps.hevc, caps.hevc_max.0, caps.hevc_max.1),
        format!("rows h264 {:?}   hevc {:?}", caps.h264_row, caps.hevc_row),
        format!("audio {}", caps.audio),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const STICK_4K: &str = "video h264 4096 2304 60\nvideo hevc 4096 2304 60\naac 8\nhdmi 2 5 6 13\n";

    #[test]
    fn a_sink_with_ac3_and_eac3_keeps_both_and_no_dts() {
        assert_eq!(
            table(STICK_4K),
            concat!(
                r#"{"videoCodecs":[{"name":"H.264","maxWidth":4096,"maxHeight":2304,"maxFrameRate":60},"#,
                r#"{"name":"HEVC","maxWidth":4096,"maxHeight":2304,"maxFrameRate":60}],"#,
                r#""audioCodecs":[{"name":"AAC","channels":8},{"name":"AC3"},{"name":"EAC3"}]}"#
            )
        );
    }

    #[test]
    fn a_sink_with_no_ac3_leaves_ac3_and_eac3_out() {
        let table = table("video h264 1920 1088 60\nvideo hevc 1920 1088 60\naac 8\nhdmi 2 4\n");
        assert!(table.contains(r#""audioCodecs":[{"name":"AAC","channels":8}]"#), "{table}");
    }

    #[test]
    fn eac3_needs_ac3_as_well() {
        let table = table("video h264 1920 1088 60\naac 8\nhdmi 2 6\n");
        assert!(!table.contains("EAC3") && !table.contains("AC3"), "{table}");
    }

    #[test]
    fn dts_is_listed_only_where_hdmi_reports_it_and_with_a_channel_limit() {
        let with = table("video h264 1920 1088 60\naac 8\nhdmi 2 5 6 7 8\n");
        assert!(with.contains(r#"{"name":"DTS","channels":6}"#), "{with}");
        assert!(!table(STICK_4K).contains("DTS"));
    }

    #[test]
    fn a_decoder_that_is_missing_has_no_row() {
        let table = table("video h264 1920 1088 60\naac 8\nhdmi 2\n");
        assert!(!table.contains("HEVC"), "{table}");
        let table = super::table("video hevc 1920 1088 60\naac 8\nhdmi 2\n");
        assert!(!table.contains("H.264"), "{table}");
    }

    #[test]
    fn aac_is_always_listed_and_an_unread_limit_is_not_stated() {
        let table = table("video h264 1920 1088 0\n");
        assert!(table.contains(r#""audioCodecs":[{"name":"AAC"}]"#), "{table}");
        assert!(table.contains(r#""maxFrameRate":0"#), "{table}");
    }

    #[test]
    fn junk_lines_are_ignored() {
        let table = table("\nvideo vp9 3840 2160 60\nvideo h264 1920\nbogus 1 2\nvideo h264 1920 1088 30\n");
        assert_eq!(
            table,
            r#"{"videoCodecs":[{"name":"H.264","maxWidth":1920,"maxHeight":1088,"maxFrameRate":30}],"audioCodecs":[{"name":"AAC"}]}"#
        );
    }
}
