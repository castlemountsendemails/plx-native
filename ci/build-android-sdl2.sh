#!/usr/bin/env bash
# Builds SDL2 (ci/dependencies.json `sdl2`) as libSDL2.so for the Fire TV APK (armeabi-v7a, API 25)
# and installs it under PREFIX, with SDL's Java sources under PREFIX/java: SDLActivity refuses a
# libSDL2.so from a different release, so both come from the same tarball.
#
#   ANDROID_NDK_HOME=<ndk> ci/build-android-sdl2.sh PREFIX
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}"
[ $# -eq 1 ] || { echo "usage: $0 PREFIX" >&2; exit 2; }
mkdir -p "$1"
PREFIX=$(cd "$1" && pwd)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
VERSION=$(python3 "$ROOT/ci/deps.py" get sdl2 version)
tar -xzf "$(python3 "$ROOT/ci/deps.py" fetch sdl2)" -C "$WORK"
SRC="$WORK/SDL2-$VERSION"

# CMAKE_POLICY_VERSION_MINIMUM: CMake 4 rejects the pre-3.5 minimums SDL's subprojects declare.
cmake -S "$SRC" -B "$WORK/build" \
  -DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=armeabi-v7a -DANDROID_PLATFORM=android-25 \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DSDL_SHARED=ON -DSDL_STATIC=OFF -DSDL_TEST=OFF
cmake --build "$WORK/build" -j"$(nproc)"
cmake --install "$WORK/build"

rm -rf "$PREFIX/java"
mkdir -p "$PREFIX/java"
cp -R "$SRC/android-project/app/src/main/java/org" "$PREFIX/java/"

# The loader resolves DT_NEEDED by file name; a versioned SONAME would never be found in the APK.
soname=$(readelf -d "$PREFIX/lib/libSDL2.so" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
[ "$soname" = libSDL2.so ] || { echo "libSDL2.so has SONAME '$soname', expected libSDL2.so" >&2; exit 1; }
