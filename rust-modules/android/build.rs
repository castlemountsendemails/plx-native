//! Links the NDK-built SDL2 and SDL2_ttf from `PLX_ANDROID_SDL_PREFIX` (ci/build-android-sdl2*.sh),
//! the system GLES 2 library the app draws with, and `src/svg.c` built with the NDK clang and flags
//! that cargo-ndk names. Host builds (clippy) compile the crate empty and link nothing.

use std::path::Path;

#[allow(dead_code)] // only the nanosvg build is for Android; the rest links host executables
#[path = "../build_support/host_link.rs"]
mod host_link;

fn main() {
    println!("cargo:rerun-if-env-changed=PLX_ANDROID_SDL_PREFIX");
    println!("cargo:rerun-if-changed=../build_support/host_link.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    let prefix = std::env::var("PLX_ANDROID_SDL_PREFIX")
        .expect("PLX_ANDROID_SDL_PREFIX must name the install prefix of ci/build-android-sdl2.sh");
    println!("cargo:rustc-link-search=native={prefix}/lib");
    for lib in ["SDL2", "SDL2_ttf", "GLESv2", "log"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    // cargo-ndk names the NDK's clang in CC_<target> and the target and API level in CFLAGS_<target>.
    let target = std::env::var("TARGET").expect("TARGET");
    let target_env = |name: &str| {
        let keys = [format!("{name}_{target}"), format!("{name}_{}", target.replace('-', "_"))];
        for key in &keys {
            println!("cargo:rerun-if-env-changed={key}");
        }
        keys.iter().find_map(|key| std::env::var(key).ok())
    };
    let cc = target_env("CC").unwrap_or_else(|| panic!("no CC_{target}: build with cargo-ndk"));
    let cflags = target_env("CFLAGS").unwrap_or_default();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the repository root");
    host_link::compile_svg(repo, &cc, &cflags.split_whitespace().collect::<Vec<_>>());
}
