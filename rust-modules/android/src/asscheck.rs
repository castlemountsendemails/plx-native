//! The subtitle harness (`--es ass check`): one SRT line through the real path, `text_ass` to the
//! bundled libass-plx.so and the fonts in the files folder, laid out at 1920x1080. It reports
//! whether a frame with ink came back, or the `Fault` that says what is missing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plx_media::player::ass::{self, Content, Event, Origin, Source};
use plx_media::player::text_ass::{self, TextDialect};
use plx_media::player::text_style::TextStyle;

use crate::entry::{log, LOG_INFO};

const LINE: &str = "<i>Hello</i> from libass";
const WAIT: Duration = Duration::from_secs(10);

pub fn harness(mode: &str) -> Vec<String> {
    if mode != "check" {
        return vec![format!("ass={mode}: only 'check' is known")];
    }
    let style = TextStyle::default();
    let Some(cue) = text_ass::convert(LINE, TextDialect::Srt) else {
        return vec!["ass check: FAIL (the SRT line converted to nothing)".into()];
    };
    let source = Arc::new(Source {
        id: ass::next_source_id(),
        revision: 1,
        origin: Origin::Text(style),
        content: Content::Embedded {
            header: text_ass::header(&style).into_bytes().into(),
            events: Arc::from([Event {
                start_ms: 0,
                duration_ms: 5000,
                payload: text_ass::event_payload(0, cue.align, &cue.body).into_bytes().into(),
            }]),
            fonts: Arc::from([]),
        },
    });
    let started = Instant::now();
    let frame = loop {
        // The worker renders on its own thread; a request returns the last completed frame, or none yet.
        let frame = ass::request(source.clone(), 1000, 1920, 1080, 0, 0);
        if let Some(frame) = frame.filter(|f| f.error.is_some() || !f.rects.is_empty()) {
            break Some(frame);
        }
        if started.elapsed() > WAIT {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let line = match frame {
        None => format!("ass check: FAIL (no frame in {} s)", WAIT.as_secs()),
        Some(frame) => match frame.error {
            Some(fault) => format!("ass check: FAIL ({fault:?})"),
            None => {
                let ink: usize = frame.rects.iter().map(|r| r.rgba.chunks_exact(4).filter(|px| px[3] != 0).count()).sum();
                let (x0, y0, x1, y1) = frame.rects.iter().fold((i32::MAX, i32::MAX, 0, 0), |b, r| {
                    (b.0.min(r.x), b.1.min(r.y), b.2.max(r.x + r.width), b.3.max(r.y + r.height))
                });
                let detail = format!(
                    "{} region(s), {ink} inked pixels, box {x0},{y0}..{x1},{y1} of {}x{}, {} ms",
                    frame.rects.len(),
                    frame.width,
                    frame.height,
                    started.elapsed().as_millis()
                );
                format!("ass check: {} ({detail})", if ink > 0 { "PASS" } else { "FAIL" })
            }
        },
    };
    log(LOG_INFO, &line);
    vec!["ass check".into(), line]
}
