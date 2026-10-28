//! SDLActivity enters `SDL_main` here on its SDL thread. With no harness argument it installs the
//! Fire TV port and runs the app. With `secure=`, `net=` or `clip=` it runs that harness instead and
//! draws its lines with SDL2_ttf until SDL quits; a clip plays first and its label replaces the
//! lines until BACK stops it. Everything logs to logcat under the tag `PlxNative`.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::path::PathBuf;

use crate::clip;
use crate::harness::{self, Harness};
use crate::jni;
use crate::keys::{self, Edge};
use crate::netcheck;
use plxnative_modules::KeyEdge;
use crate::secure;

#[allow(dead_code)] // read by SDL only
#[repr(C)]
#[derive(Clone, Copy)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[allow(dead_code)] // read by SDL only
#[repr(C)]
struct Rect {
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
}

// SDL_Event is a 56-byte union; only its leading `type` is read here.
#[repr(C, align(8))]
struct Event {
    kind: u32,
    _rest: [u8; 52],
}

type Handle = *mut c_void;

const SDL_INIT_VIDEO: u32 = 0x20;
const SDL_WINDOW_FULLSCREEN: u32 = 0x1;
const SDL_RENDERER_ACCELERATED: u32 = 0x2;
const SDL_QUIT: u32 = 0x100;
const SDL_GL_ALPHA_SIZE: c_int = 3;
/// A label draw takes about 350 ms on the Stick 4K, and the engine is not pumped meanwhile.
/// ponytail: each draw still stalls the pump once a second, which the ~1.5 s buffered ahead
/// covers. Upgrade: keep each line's texture and re-render only the lines that changed.
const LABEL_EVERY_MS: u32 = 1_000;
pub const LOG_INFO: c_int = 4;
pub const LOG_ERROR: c_int = 6;

extern "C" {
    fn SDL_Init(flags: u32) -> c_int;
    fn SDL_Quit();
    fn SDL_GetError() -> *const c_char;
    fn SDL_SetHint(name: *const c_char, value: *const c_char) -> c_int;
    fn SDL_GL_SetAttribute(attr: c_int, value: c_int) -> c_int;
    fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> Handle;
    fn SDL_DestroyWindow(window: Handle);
    fn SDL_CreateRenderer(window: Handle, index: c_int, flags: u32) -> Handle;
    fn SDL_DestroyRenderer(renderer: Handle);
    fn SDL_GetRendererOutputSize(renderer: Handle, w: *mut c_int, h: *mut c_int) -> c_int;
    fn SDL_SetRenderDrawColor(renderer: Handle, r: u8, g: u8, b: u8, a: u8) -> c_int;
    fn SDL_RenderClear(renderer: Handle) -> c_int;
    fn SDL_RenderCopy(
        renderer: Handle,
        texture: Handle,
        src: *const Rect,
        dst: *const Rect,
    ) -> c_int;
    fn SDL_RenderPresent(renderer: Handle);
    fn SDL_CreateTextureFromSurface(renderer: Handle, surface: Handle) -> Handle;
    fn SDL_QueryTexture(
        texture: Handle,
        format: *mut u32,
        access: *mut c_int,
        w: *mut c_int,
        h: *mut c_int,
    ) -> c_int;
    fn SDL_DestroyTexture(texture: Handle);
    fn SDL_FreeSurface(surface: Handle);
    fn SDL_WaitEventTimeout(event: *mut Event, timeout_ms: c_int) -> c_int;
    fn SDL_GetTicks() -> u32;
    fn TTF_Init() -> c_int;
    fn TTF_Quit();
    fn TTF_OpenFont(file: *const c_char, ptsize: c_int) -> Handle;
    fn TTF_CloseFont(font: Handle);
    fn TTF_RenderUTF8_Blended(font: Handle, text: *const c_char, fg: Color) -> Handle;
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    fn SDL_AndroidGetInternalStoragePath() -> *const c_char;
}

pub fn log(prio: c_int, message: &str) {
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    unsafe { __android_log_write(prio, c"PlxNative".as_ptr(), text.as_ptr()) };
}

fn sdl_error(call: &str) -> String {
    let detail = unsafe { CStr::from_ptr(SDL_GetError()) };
    format!("{call} failed: {}", detail.to_string_lossy())
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn SDL_main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let args: Vec<String> = (0..argc.max(0) as usize)
        .map(|i| unsafe { *argv.add(i) })
        .filter(|arg| !arg.is_null())
        .map(|arg| unsafe { CStr::from_ptr(arg) }.to_string_lossy().into_owned())
        .collect();
    // The engine's socket sends without MSG_NOSIGNAL; a reset peer must not kill the process.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    if let Some(files) = files_dir() {
        plx_base::paths::set_android_files_dir(files);
    } else {
        log(LOG_ERROR, "SDL_AndroidGetInternalStoragePath failed; app files are looked for nowhere");
    }
    let arg = |key: &str| args.iter().find_map(|arg| arg.strip_prefix(key)?.strip_prefix('='));
    let lines = match (arg("secure"), arg("net")) {
        (Some(mode), _) => Some(secure::harness(mode)),
        (None, Some(_)) => Some(netcheck::harness(arg("server"))),
        (None, None) => None,
    };
    let clip = clip::parse(args.iter().map(String::as_str));
    if clip.is_none() && lines.is_none() {
        if plx_platform::tv::install(&crate::port::PORT).is_err() {
            log(LOG_ERROR, "a port was already installed");
        }
        return plxnative_modules::run_android(push_presses);
    }
    match run(clip, lines) {
        Ok(()) => 0,
        Err(message) => {
            log(LOG_ERROR, &message);
            1
        }
    }
}

fn push_presses() {
    jni::tick();
    for press in jni::take_presses() {
        let edge = match press.edge {
            Edge::Down => KeyEdge::Down,
            Edge::Repeat => KeyEdge::Repeat,
            Edge::Up => KeyEdge::Up,
        };
        if !plxnative_modules::push_remote_key(keys::token(press.key), edge) {
            log(LOG_ERROR, &format!("no app key for {:?}", press.key));
        }
    }
}

fn files_dir() -> Option<PathBuf> {
    let dir = unsafe { SDL_AndroidGetInternalStoragePath() };
    (!dir.is_null()).then(|| PathBuf::from(unsafe { CStr::from_ptr(dir) }.to_string_lossy().into_owned()))
}

/// `harness_lines` are what a `secure=` or `net=` harness reported; without them (a clip that has
/// stopped) the screen shows the latest key press.
fn run(clip: Option<clip::Clip>, harness_lines: Option<Vec<String>>) -> Result<(), String> {
    unsafe {
        if SDL_Init(SDL_INIT_VIDEO) != 0 {
            return Err(sdl_error("SDL_Init"));
        }
        if TTF_Init() != 0 {
            return Err(sdl_error("TTF_Init"));
        }
        // An alpha channel that SurfaceFlinger honours, so the video SurfaceView under this one
        // shows wherever the UI clears to transparent.
        SDL_SetHint(c"SDL_VIDEO_EGL_ALLOW_TRANSPARENCY".as_ptr(), c"1".as_ptr());
        SDL_GL_SetAttribute(SDL_GL_ALPHA_SIZE, 8);
        // Android sizes the window to the surface, which PlxActivity fixes at 1920x1080.
        let window = SDL_CreateWindow(
            c"PlxNative".as_ptr(),
            0,
            0,
            1920,
            1080,
            SDL_WINDOW_FULLSCREEN,
        );
        if window.is_null() {
            return Err(sdl_error("SDL_CreateWindow"));
        }
        let renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_ACCELERATED);
        if renderer.is_null() {
            return Err(sdl_error("SDL_CreateRenderer"));
        }
        // A relative path falls through to the APK assets in SDL_RWFromFile.
        let font = TTF_OpenFont(c"appfont.ttf".as_ptr(), 48);
        if font.is_null() {
            return Err(sdl_error("TTF_OpenFont"));
        }
        log(LOG_INFO, "harness screen started");
        let mut harness = clip.map(Harness::start);

        let mut size = (0, 0);
        let mut shown: Vec<String> = Vec::new();
        let mut redraw = true;
        let mut labelled_at = 0;
        let mut event = Event {
            kind: 0,
            _rest: [0; 52],
        };
        loop {
            jni::tick();
            let back = jni::take_back();
            let presses = jni::take_presses();
            if let Some(harness) = harness.as_mut() {
                if back {
                    harness.stop();
                }
                harness.keys(&presses);
                harness.frame(SDL_GetTicks());
            }
            let playing = harness.as_ref().is_some_and(Harness::active);
            let (mut w, mut h) = (0, 0);
            SDL_GetRendererOutputSize(renderer, &mut w, &mut h);
            if (w, h) != size {
                size = (w, h);
                log(LOG_INFO, &format!("surface {w}x{h}"));
                redraw = true;
            }
            // The label's counters change every frame; re-rendering it every loop slows the loop, and
            // the engine feeds the decoder only once per loop.
            let label_due = SDL_GetTicks().wrapping_sub(labelled_at) >= LABEL_EVERY_MS;
            let lines = match harness.as_ref().filter(|_| playing) {
                Some(_) if !label_due && !redraw => shown.clone(),
                Some(harness) => {
                    labelled_at = SDL_GetTicks();
                    harness::LABEL.time(|| harness.label())
                }
                None if harness_lines.is_some() => harness_lines.clone().unwrap_or_default(),
                None => {
                    let readout = jni::readout();
                    let key = if readout.is_empty() { "press a key".into() } else { readout };
                    vec!["PlxNative for Fire TV: harness".into(), format!("surface {w}x{h}"), key]
                }
            };
            if lines != shown {
                shown = lines;
                redraw = true;
            }
            if redraw {
                harness::DRAW.time(|| draw(renderer, font, w, &shown, playing));
                redraw = false;
            }
            // Short, so a held BACK turns into Exit on time; shorter while playing, because the
            // engine feeds the sink only when it is pumped. Any SDL event (resume, resize) redraws.
            if SDL_WaitEventTimeout(&mut event, if playing { 10 } else { 50 }) == 1 {
                if event.kind == SDL_QUIT {
                    break;
                }
                redraw = true;
            }
        }
        if let Some(harness) = harness.as_mut() {
            harness.stop();
        }

        log(LOG_INFO, "harness screen quitting");
        TTF_CloseFont(font);
        SDL_DestroyRenderer(renderer);
        SDL_DestroyWindow(window);
        TTF_Quit();
        SDL_Quit();
    }
    Ok(())
}

// Textures are rebuilt on every draw and resuming sends SDL events that trigger one, so a GL context
// lost while the app is paused costs nothing. While a clip plays, the screen is cleared to transparent
// and the label sits at the top, over the video.
fn draw(renderer: Handle, font: Handle, width: c_int, lines: &[String], playing: bool) {
    let gold = Color {
        r: 0xe5,
        g: 0xa0,
        b: 0x0d,
        a: 0xff,
    };
    unsafe {
        if playing {
            SDL_SetRenderDrawColor(renderer, 0, 0, 0, 0);
        } else {
            SDL_SetRenderDrawColor(renderer, 0x1f, 0x1f, 0x1f, 0xff);
        }
        SDL_RenderClear(renderer);
        let mut y = if playing { 40 } else { 420 };
        for line in lines {
            let line = CString::new(line.replace('\0', " ")).unwrap_or_default();
            let surface = TTF_RenderUTF8_Blended(font, line.as_ptr(), gold);
            if surface.is_null() {
                continue;
            }
            let texture = SDL_CreateTextureFromSurface(renderer, surface);
            SDL_FreeSurface(surface);
            if texture.is_null() {
                continue;
            }
            let (mut w, mut h) = (0, 0);
            SDL_QueryTexture(texture, null_mut(), null_mut(), &mut w, &mut h);
            let dst = Rect {
                x: (width - w) / 2,
                y,
                w,
                h,
            };
            SDL_RenderCopy(renderer, texture, null(), &dst);
            SDL_DestroyTexture(texture);
            y += h + 24;
        }
        SDL_RenderPresent(renderer);
    }
}
