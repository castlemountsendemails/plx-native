//! The Fire TV skeleton: SDLActivity enters `SDL_main` on its SDL thread, and this draws two lines
//! of text with SDL2_ttf until SDL quits, logging to logcat under the tag `PlxNative`.
#![cfg(target_os = "android")]

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::{null, null_mut};

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
const LOG_INFO: c_int = 4;
const LOG_ERROR: c_int = 6;

extern "C" {
    fn SDL_Init(flags: u32) -> c_int;
    fn SDL_Quit();
    fn SDL_GetError() -> *const c_char;
    fn SDL_CreateWindow(title: *const c_char, x: c_int, y: c_int, w: c_int, h: c_int, flags: u32) -> Handle;
    fn SDL_DestroyWindow(window: Handle);
    fn SDL_CreateRenderer(window: Handle, index: c_int, flags: u32) -> Handle;
    fn SDL_DestroyRenderer(renderer: Handle);
    fn SDL_GetRendererOutputSize(renderer: Handle, w: *mut c_int, h: *mut c_int) -> c_int;
    fn SDL_SetRenderDrawColor(renderer: Handle, r: u8, g: u8, b: u8, a: u8) -> c_int;
    fn SDL_RenderClear(renderer: Handle) -> c_int;
    fn SDL_RenderCopy(renderer: Handle, texture: Handle, src: *const Rect, dst: *const Rect) -> c_int;
    fn SDL_RenderPresent(renderer: Handle);
    fn SDL_CreateTextureFromSurface(renderer: Handle, surface: Handle) -> Handle;
    fn SDL_QueryTexture(texture: Handle, format: *mut u32, access: *mut c_int, w: *mut c_int, h: *mut c_int) -> c_int;
    fn SDL_DestroyTexture(texture: Handle);
    fn SDL_FreeSurface(surface: Handle);
    fn SDL_WaitEventTimeout(event: *mut Event, timeout_ms: c_int) -> c_int;
    fn TTF_Init() -> c_int;
    fn TTF_Quit();
    fn TTF_OpenFont(file: *const c_char, ptsize: c_int) -> Handle;
    fn TTF_CloseFont(font: Handle);
    fn TTF_RenderUTF8_Blended(font: Handle, text: *const c_char, fg: Color) -> Handle;
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

fn log(prio: c_int, message: &str) {
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    unsafe { __android_log_write(prio, c"PlxNative".as_ptr(), text.as_ptr()) };
}

fn sdl_error(call: &str) -> String {
    let detail = unsafe { CStr::from_ptr(SDL_GetError()) };
    format!("{call} failed: {}", detail.to_string_lossy())
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    match run() {
        Ok(()) => 0,
        Err(message) => {
            log(LOG_ERROR, &message);
            1
        }
    }
}

fn run() -> Result<(), String> {
    unsafe {
        if SDL_Init(SDL_INIT_VIDEO) != 0 {
            return Err(sdl_error("SDL_Init"));
        }
        if TTF_Init() != 0 {
            return Err(sdl_error("TTF_Init"));
        }
        // Android sizes the window to the surface, which PlxActivity fixes at 1920x1080.
        let window = SDL_CreateWindow(c"PlxNative".as_ptr(), 0, 0, 1920, 1080, SDL_WINDOW_FULLSCREEN);
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
        log(LOG_INFO, "skeleton started");

        let mut size = (0, 0);
        let mut event = Event { kind: 0, _rest: [0; 52] };
        loop {
            let (mut w, mut h) = (0, 0);
            SDL_GetRendererOutputSize(renderer, &mut w, &mut h);
            if (w, h) != size {
                size = (w, h);
                log(LOG_INFO, &format!("surface {w}x{h}"));
            }
            let size_line = CString::new(format!("surface {w}x{h}")).unwrap_or_default();
            draw(renderer, font, w, &[c"PlxNative for Fire TV: skeleton", size_line.as_c_str()]);
            if SDL_WaitEventTimeout(&mut event, 500) == 1 && event.kind == SDL_QUIT {
                break;
            }
        }

        log(LOG_INFO, "skeleton quitting");
        TTF_CloseFont(font);
        SDL_DestroyRenderer(renderer);
        SDL_DestroyWindow(window);
        TTF_Quit();
        SDL_Quit();
    }
    Ok(())
}

// Textures are rebuilt every frame, so a GL context lost while the app is paused costs nothing.
fn draw(renderer: Handle, font: Handle, width: c_int, lines: &[&CStr]) {
    let gold = Color { r: 0xe5, g: 0xa0, b: 0x0d, a: 0xff };
    unsafe {
        SDL_SetRenderDrawColor(renderer, 0x1f, 0x1f, 0x1f, 0xff);
        SDL_RenderClear(renderer);
        let mut y = 420;
        for line in lines {
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
            let dst = Rect { x: (width - w) / 2, y, w, h };
            SDL_RenderCopy(renderer, texture, null(), &dst);
            SDL_DestroyTexture(texture);
            y += h + 24;
        }
        SDL_RenderPresent(renderer);
    }
}
