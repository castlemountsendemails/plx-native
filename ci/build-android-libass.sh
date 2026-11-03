#!/bin/sh
# Builds the pinned libass font stack and the plx_ass_* facade as libass-plx.so for the Fire TV APK
# (armeabi-v7a, API 25), into vendor/libass-prefix-android/lib. The recipe is ci/build-libass.py,
# the same one the webOS and simulator libraries come from.
#
#   ANDROID_NDK_HOME=<ndk> ci/build-android-libass.sh
set -eu
ANDROID=1 exec python3 "$(dirname "$0")/build-libass.py" "$@"
