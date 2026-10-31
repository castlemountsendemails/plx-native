//! The clip the video harness plays, read from the `key=value` arguments `PlxActivity` builds from
//! its intent extras (`am start ... --es clip <url> [--es vc hevc] [--es ac aac] [--es fps 23.976] [--es size 1920x804]`).
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
    /// The clip's own raster, as the item's dimensions are in a real playback. The engine sizes the
    /// decoder's declaration from it; without it the declaration is the widest raster, which the
    /// HD and 3rd gen H.264 decoders refuse.
    pub size: Option<(u16, u16)>,
}

/// `None` without a `clip=` argument, which starts the app unless another harness was asked for.
/// Arguments without `=`, unknown keys and an unparsable `fps` are ignored.
pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Option<Clip> {
    let mut clip = Clip {
        url: String::new(),
        vc: "h264".into(),
        ac: "ac3".into(),
        fps: 23.976,
        size: None,
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
            "size" => {
                clip.size = value.split_once('x').and_then(|(w, h)| {
                    Some((w.parse().ok().filter(|w| *w > 0)?, h.parse().ok().filter(|h| *h > 0)?))
                });
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
    fn no_clip_argument_means_no_clip() {
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
    fn size_is_a_positive_wxh_and_anything_else_is_unset() {
        let size = |s: &str| parse(["clip=http://h/c.mkv", s]).unwrap().size;
        assert_eq!(size("size=1920x804"), Some((1920, 804)));
        for bad in ["size=1920", "size=0x804", "size=axb", "size=1920x-1"] {
            assert_eq!(size(bad), None, "{bad}");
        }
        assert_eq!(parse(["clip=http://h/c.mkv"]).unwrap().size, None);
    }

    #[test]
    fn a_bad_fps_keeps_the_default() {
        for fps in ["fps=abc", "fps=0", "fps=-24"] {
            assert_eq!(parse(["clip=http://h/c.mkv", fps]).unwrap().fps, 23.976, "{fps}");
        }
    }
}
