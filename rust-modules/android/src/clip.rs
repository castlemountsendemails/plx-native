//! The clip the video harness plays, read from the `key=value` arguments `PlxActivity` builds from
//! its intent extras (`am start ... --es clip <url> [--es vc hevc] [--es ac aac] [--es fps 23.976]`).
//! No JNI or SDL in here, so the host unit suite tests it.

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub url: String,
    /// The route's video codec name: `h264` or `hevc`.
    pub vc: String,
    /// The route's audio codec name. The engine refuses a streamed start unless it is `ac3`,
    /// `eac3`, `aac` or `dts`.
    pub ac: String,
    pub fps: f64,
}

/// `None` without a `clip=` argument, which leaves the skeleton screen as it was. Arguments
/// without `=`, unknown keys and an unparsable `fps` are ignored.
pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Option<Clip> {
    let mut clip = Clip {
        url: String::new(),
        vc: "h264".into(),
        ac: "ac3".into(),
        fps: 23.976,
    };
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            continue;
        };
        match key {
            "clip" => clip.url = value.into(),
            "vc" => clip.vc = value.into(),
            "ac" => clip.ac = value.into(),
            "fps" => {
                if let Ok(fps) = value.parse::<f64>() {
                    if fps > 0.0 {
                        clip.fps = fps;
                    }
                }
            }
            _ => {}
        }
    }
    (!clip.url.is_empty()).then_some(clip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clip_argument_means_the_skeleton_screen() {
        assert_eq!(parse(["PlxNative"]), None);
        assert_eq!(parse(["vc=hevc", "ac=aac"]), None);
        assert_eq!(parse(["clip="]), None);
    }

    #[test]
    fn a_bare_clip_declares_h264_with_ac3() {
        let clip = parse(["PlxNative", "clip=http://192.0.2.1:8000/h264-1080p.mkv"]).unwrap();
        assert_eq!(clip.url, "http://192.0.2.1:8000/h264-1080p.mkv");
        assert_eq!((clip.vc.as_str(), clip.ac.as_str(), clip.fps), ("h264", "ac3", 23.976));
    }

    #[test]
    fn every_extra_is_read_and_a_query_string_survives() {
        let clip = parse([
            "clip=http://192.0.2.1:8000/hevc-4k.mkv?a=1&b=2",
            "vc=hevc",
            "ac=aac",
            "fps=25",
        ])
        .unwrap();
        assert_eq!(clip.url, "http://192.0.2.1:8000/hevc-4k.mkv?a=1&b=2");
        assert_eq!((clip.vc.as_str(), clip.ac.as_str(), clip.fps), ("hevc", "aac", 25.0));
    }

    #[test]
    fn a_bad_fps_keeps_the_default() {
        for fps in ["fps=abc", "fps=0", "fps=-24"] {
            assert_eq!(parse(["clip=http://h/c.mkv", fps]).unwrap().fps, 23.976, "{fps}");
        }
    }
}
