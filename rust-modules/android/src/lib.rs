//! The Fire TV `libmain.so`. SDLActivity enters `SDL_main` in `skeleton`; `PlxActivity` hands remote
//! keys through `jni` to `keys`, and the intent's clip as `SDL_main`'s arguments to `clip`, which
//! `harness` plays through the player engine. `keys` and `clip` also build, and are tested, on the
//! host.

pub mod clip;
pub mod keys;

#[cfg(target_os = "android")]
mod harness;
#[cfg(target_os = "android")]
mod jni;
#[cfg(target_os = "android")]
mod log_sink;
#[cfg(target_os = "android")]
mod port;
#[cfg(target_os = "android")]
mod skeleton;
