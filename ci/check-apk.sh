#!/usr/bin/env bash
# Fails unless a Fire TV APK has the settings the sticks need: the armeabi-v7a libmain.so, minSdk 25,
# targetSdk 28, extractNativeLibs and allowBackup as the manifest sets them, a Leanback launcher
# entry, the expected package, version and debuggable flag, and exactly one signer with CERT_SHA256.
# Every check runs and reports, so one failure does not hide the next.
#
#   ANDROID_HOME=<sdk> ci/check-apk.sh APK PACKAGE VERSION_CODE VERSION_NAME CERT_SHA256 true|false
set -euo pipefail

: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
[ $# -eq 6 ] || { echo "usage: $0 APK PACKAGE VERSION_CODE VERSION_NAME CERT_SHA256 DEBUGGABLE" >&2; exit 2; }
apk=$1 package=$2 code=$3 name=$4 debuggable=$6
cert=$(tr -d : <<<"$5" | tr A-F a-f)

tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n 1)
badging=$("$tools/aapt2" dump badging "$apk")
manifest=$("$tools/aapt2" dump xmltree --file AndroidManifest.xml "$apk")
signers=$("$tools/apksigner" verify --print-certs "$apk")
files=$(unzip -Z1 "$apk")

failed=0
check() {
  if "${@:2}"; then echo "ok      $1"; else echo "::error::APK check failed: $1"; failed=1; fi
}
has() { grep -qE -- "$2" <<<"$1"; }
lacks() { ! has "$@"; }
# apksigner prints one digest line per signer and scheme ("Signer #1", "V2 Signer", ...).
one_signer() {
  [ "$(grep -oE 'certificate SHA-256 digest: [0-9a-f]+$' <<<"$signers" | sort -u)" = \
    "certificate SHA-256 digest: $cert" ]
}

check "package $package, versionCode $code, versionName $name" \
  has "$badging" "^package: name='$package' versionCode='$code' versionName='$name' "
check "armeabi-v7a libmain.so" has "$files" '^lib/armeabi-v7a/libmain\.so$'
check "minSdk 25" has "$badging" "^(min)?[sS]dkVersion:'25'$"
check "targetSdk 28" has "$badging" "^targetSdkVersion:'28'$"
# aapt2 prints a boolean attribute as true/false, older builds as its raw (type 0x12) value.
check "extractNativeLibs=true" has "$manifest" 'extractNativeLibs\(0x[0-9a-f]+\)=(true|\(type 0x12\)0xffffffff)( |$)'
check "allowBackup=false" has "$manifest" 'allowBackup\(0x[0-9a-f]+\)=(false|\(type 0x12\)0x0)( |$)'
check "Leanback launcher entry" has "$badging" '^leanback-launchable-activity:'
if [ "$debuggable" = true ]; then
  check "debuggable" has "$badging" '^application-debuggable$'
else
  check "not debuggable" lacks "$badging" '^application-debuggable$'
fi
check "signed by $cert only" one_signer

if [ "$failed" != 0 ]; then
  echo "--- aapt2 badging"; echo "$badging"
  echo "--- manifest"; echo "$manifest"
  echo "--- signers"; echo "$signers"
  exit 1
fi
