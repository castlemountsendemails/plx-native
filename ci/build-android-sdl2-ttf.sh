#!/usr/bin/env bash
# Builds SDL2_ttf (ci/dependencies.json `sdl2-ttf`) as libSDL2_ttf.so for the Fire TV APK
# (armeabi-v7a, API 25) against the SDL2 that ci/build-android-sdl2.sh installed in PREFIX, and
# installs it there. FreeType is the copy vendored in the SDL2_ttf tarball, linked in statically;
# HarfBuzz is off.
#
#   ANDROID_NDK_HOME=<ndk> ci/build-android-sdl2-ttf.sh PREFIX
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}"
[ $# -eq 1 ] || { echo "usage: $0 PREFIX" >&2; exit 2; }
PREFIX=$(cd "$1" && pwd)
[ -f "$PREFIX/lib/libSDL2.so" ] || { echo "no libSDL2.so in $PREFIX: run ci/build-android-sdl2.sh first" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
VERSION=$(python3 "$ROOT/ci/deps.py" get sdl2-ttf version)
tar -xzf "$(python3 "$ROOT/ci/deps.py" fetch sdl2-ttf)" -C "$WORK"

# The NDK toolchain confines find_package to its sysroot; SDL2_DIR and the find root let it see PREFIX.
cmake -S "$WORK/SDL2_ttf-$VERSION" -B "$WORK/build" \
  -DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=armeabi-v7a -DANDROID_PLATFORM=android-25 \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DCMAKE_FIND_ROOT_PATH="$PREFIX" -DSDL2_DIR="$PREFIX/lib/cmake/SDL2" \
  -DBUILD_SHARED_LIBS=ON -DSDL2TTF_VENDORED=ON -DSDL2TTF_HARFBUZZ=OFF -DSDL2TTF_SAMPLES=OFF
cmake --build "$WORK/build" -j"$(nproc)"
cmake --install "$WORK/build"

soname=$(readelf -d "$PREFIX/lib/libSDL2_ttf.so" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
[ "$soname" = libSDL2_ttf.so ] || { echo "libSDL2_ttf.so has SONAME '$soname', expected libSDL2_ttf.so" >&2; exit 1; }
