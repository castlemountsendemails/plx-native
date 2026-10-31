//! The Fire TV `libmain.so`. SDLActivity enters `SDL_main` in `entry`, which runs the app with
//! `port` installed. `PlxActivity` hands remote keys through `jni` to `keys`, and a harness's
//! intent extras as `SDL_main`'s arguments: a clip to `clip`, which `harness` plays through the
//! player engine into the `amediacodec` sink, whose audio `audiotrack` plays. `keys`, `clip`,
//! `video` and `audio` also build, and are tested, on the host.

pub mod audio;
pub mod clip;
pub mod keys;
pub mod video;

#[cfg(target_os = "android")]
mod amediacodec;
#[cfg(target_os = "android")]
mod audiotrack;
#[cfg(target_os = "android")]
mod entry;
#[cfg(target_os = "android")]
mod harness;
#[cfg(target_os = "android")]
mod jni;
#[cfg(target_os = "android")]
mod netcheck;
#[cfg(target_os = "android")]
mod port;
#[cfg(target_os = "android")]
mod secure;
