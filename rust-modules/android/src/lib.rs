//! The Fire TV `libmain.so`. SDLActivity enters `SDL_main` in `skeleton`; `PlxActivity` hands remote
//! keys through `jni` to `keys`, the one module that also builds, and is tested, on the host.

pub mod keys;

#[cfg(target_os = "android")]
mod jni;
#[cfg(target_os = "android")]
mod skeleton;
