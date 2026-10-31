#!/usr/bin/env bash
# Builds libcurl (ci/dependencies.json `curl`) as libcurl-plx.so for the Fire TV APK (armeabi-v7a,
# API 25), with OpenSSL (`openssl`) linked into it statically, and installs it under PREFIX. `net`
# and `curlio` open it by that name at run time; it has no built-in CA file, so the app sets
# CURLOPT_CAINFO itself.
#
#   ANDROID_NDK_HOME=<ndk> ci/build-android-curl.sh PREFIX
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}"
[ $# -eq 1 ] || { echo "usage: $0 PREFIX" >&2; exit 2; }
mkdir -p "$1"
PREFIX=$(cd "$1" && pwd)
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
OPENSSL_VERSION=$(python3 "$ROOT/ci/deps.py" get openssl version)
CURL_VERSION=$(python3 "$ROOT/ci/deps.py" get curl version)
tar -xzf "$(python3 "$ROOT/ci/deps.py" fetch openssl)" -C "$WORK"
tar -xJf "$(python3 "$ROOT/ci/deps.py" fetch curl)" -C "$WORK"

# OpenSSL's android targets read ANDROID_NDK_ROOT (not _HOME) and want the NDK clang on PATH;
# without -D__ANDROID_API__ they target the NDK's newest API instead of the sticks' 25.
OSSL="$WORK/openssl"
(
  cd "$WORK/openssl-$OPENSSL_VERSION"
  export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME" PATH="$TOOLCHAIN/bin:$PATH"
  ./Configure android-arm -D__ANDROID_API__=25 --prefix="$OSSL" --libdir=lib \
    no-shared no-module no-tests
  make -j"$(nproc)" build_libs
  make install_dev
)

# --exclude-libs keeps OpenSSL's symbols out of libcurl-plx.so's exports, so only curl's API is visible.
cmake -S "$WORK/curl-$CURL_VERSION" -B "$WORK/build" \
  -DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=armeabi-v7a -DANDROID_PLATFORM=android-25 \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DCMAKE_SHARED_LINKER_FLAGS="-Wl,--exclude-libs,ALL" \
  -DBUILD_SHARED_LIBS=ON -DBUILD_STATIC_LIBS=OFF -DLIBCURL_OUTPUT_NAME=libcurl-plx \
  -DBUILD_CURL_EXE=OFF -DBUILD_TESTING=OFF -DBUILD_EXAMPLES=OFF \
  -DBUILD_LIBCURL_DOCS=OFF -DBUILD_MISC_DOCS=OFF -DENABLE_CURL_MANUAL=OFF \
  -DCURL_USE_OPENSSL=ON -DOPENSSL_USE_STATIC_LIBS=ON -DOPENSSL_INCLUDE_DIR="$OSSL/include" \
  -DOPENSSL_SSL_LIBRARY="$OSSL/lib/libssl.a" -DOPENSSL_CRYPTO_LIBRARY="$OSSL/lib/libcrypto.a" \
  -DCURL_CA_BUNDLE=none -DCURL_CA_PATH=none -DCURL_USE_PKGCONFIG=OFF \
  -DCURL_ZLIB=OFF -DCURL_BROTLI=OFF -DCURL_ZSTD=OFF -DUSE_NGHTTP2=OFF -DUSE_LIBIDN2=OFF \
  -DCURL_USE_LIBPSL=OFF -DCURL_USE_LIBSSH2=OFF -DCURL_DISABLE_LDAP=ON
cmake --build "$WORK/build" -j"$(nproc)"
cmake --install "$WORK/build"

# The loader matches DT_NEEDED against SONAME and refuses text relocations, so either fault would
# only show as a failed dlopen on a stick; a libssl/libcrypto NEEDED would mean OpenSSL is not inside.
lib="$PREFIX/lib/libcurl-plx.so"
dyn=$("$TOOLCHAIN/bin/llvm-readelf" -d "$lib")
echo "$dyn" | grep -E 'NEEDED|SONAME'
soname=$(sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p' <<<"$dyn")
[ "$soname" = libcurl-plx.so ] || { echo "libcurl-plx.so has SONAME '$soname'" >&2; exit 1; }
if grep -E '\(TEXTREL\)|\(NEEDED\).*lib(ssl|crypto|curl)' <<<"$dyn" >&2; then
  echo "libcurl-plx.so has a text relocation or needs a TLS or curl library" >&2; exit 1
fi
exports=$("$TOOLCHAIN/bin/llvm-readelf" --dyn-syms -W "$lib" | awk '$7 != "UND" {print $8}')
for sym in curl_easy_init curl_multi_wait curl_version_info; do
  grep -qx "$sym" <<<"$exports" || { echo "libcurl-plx.so does not export $sym" >&2; exit 1; }
done
if grep -E '^(SSL|EVP|OPENSSL)_' <<<"$exports" >&2; then
  echo "libcurl-plx.so exports OpenSSL symbols" >&2; exit 1
fi
