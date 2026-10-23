//! Links the NDK-built SDL2 and SDL2_ttf from `PLX_ANDROID_SDL_PREFIX` (ci/build-android-sdl2*.sh).
//! Host builds (clippy) compile the crate empty and link nothing.

fn main() {
    println!("cargo:rerun-if-env-changed=PLX_ANDROID_SDL_PREFIX");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    let prefix = std::env::var("PLX_ANDROID_SDL_PREFIX")
        .expect("PLX_ANDROID_SDL_PREFIX must name the install prefix of ci/build-android-sdl2.sh");
    println!("cargo:rustc-link-search=native={prefix}/lib");
    for lib in ["SDL2", "SDL2_ttf", "log"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}
