# Third-party notices — Fire TV APK

This file accompanies the **PlxNative** Fire TV package (`com.beb.plxnative`, and the
`com.beb.plxnative.debug` build), an unofficial native Plex client for Amazon Fire TV sticks.
PlxNative itself is Copyright (c) 2026 Gleb Linnik and is distributed under GPL-3.0-or-later
(see `LICENSE`; the brand reservation is in `TRADEMARKS.md`). The webOS package has its own
notice file, `THIRD-PARTY-NOTICES.md`; this one lists what the APK contains.

Inside the APK this file is `assets/THIRD-PARTY-NOTICES-FIRETV.md` and the licence texts it names
are in `assets/licenses/`. The APK contains exactly these third-party components:

| File in the APK | Contents | Section |
|---|---|---|
| `lib/armeabi-v7a/libavutil-plx.so`, `libavcodec-plx.so`, `libavformat-plx.so`, `libswscale-plx.so` | FFmpeg 9.0.2 | 1 |
| `lib/armeabi-v7a/libSDL2.so` and the SDL Java classes in `classes*.dex` | SDL 2.32.10 | 2 |
| `lib/armeabi-v7a/libSDL2_ttf.so` | SDL2_ttf 2.24.0 with FreeType 2.13.2 | 3 |
| `lib/armeabi-v7a/libcurl-plx.so` | libcurl 8.23.0 with OpenSSL 3.5.9 | 4 |
| `assets/cacert.pem` | The Mozilla CA certificate store | 5 |
| `assets/le-roots.pem` | Four Let's Encrypt root certificates | 5 |
| `lib/armeabi-v7a/libmain.so` | PlxNative, the Rust crates of section 7, the Rust standard library, nanosvg | 7 |
| `assets/appfont.ttf`, `assets/appfont-bold.ttf` | Inter | 6 |
| `assets/appfont-cjk.ttf` | Noto Sans CJK KR | 6 |
| `assets/appfont-emoji.ttf` | Noto Emoji | 6 |

Full licence texts are in `assets/licenses/` and are not repeated inline, except where a licence has
no file there (sections 2, 4 and 6).

---

## 1. FFmpeg — LGPL-2.1, and your right to modify and replace it

The APK ships its own build of FFmpeg **9.0.2**, unmodified, as four shared libraries
(`libavutil-plx.so`, `libavcodec-plx.so`, `libavformat-plx.so`, `libswscale-plx.so`). The `-plx`
suffix keeps them apart from any other FFmpeg on the device; it says nothing about their content,
which is stock upstream. They are covered by the **GNU Lesser General Public License, version 2.1
or later**, supplied as `licenses/LGPL-2.1.txt`.

**Complete corresponding source.** The upstream tarball these were built from —
`ffmpeg-9.0.2.tar.xz`, sha256 `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`,
from <https://ffmpeg.org/releases/> — and the build script `ci/build-ffmpeg.sh` are published beside
every PlxNative Fire TV APK, in the `lgpl-source/` folder of the build artifact. No patches are
applied. From a checkout of the matching PlxNative source, with the Android NDK 27.3.13750724
(`ANDROID_NDK_HOME`), `ANDROID=1 FFMPEG_VERSION=9.0.2
FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
FFMPEG_TARBALL=/path/to/ffmpeg-9.0.2.tar.xz ci/build-ffmpeg.sh` rebuilds them.

**Configuration.** Built with `--disable-everything` plus an explicit component list, **without**
`--enable-gpl`, `--enable-version3` or `--enable-nonfree`, so no GPL-licensed or non-free FFmpeg
component is present. Only demuxers, parsers, bitstream filters and *subtitle* decoders are
enabled (plus libswscale, the MPEG-1 video encoder and the MPEG-TS muxer, which every Android build
enables); video and audio are decoded by the stick's hardware, not by FFmpeg. The one external library
is **zlib**, the device's own `libz.so`, which is not redistributed here.

**You may modify and replace them.** They are ordinary shared libraries, loaded by name at run time
(`dlopen`) from the app's native library directory; no FFmpeg code is linked into `libmain.so`.
Android installs an APK's libraries from the signed package, so replacing one means building your own
APK: build FFmpeg as above, put the libraries in the Gradle project's native library directory under
the same file names, and build and sign the APK with the Gradle project in `android/` (the `firetv`
job in `.github/workflows/ci.yml` is the exact recipe). The application reads FFmpeg struct fields at
byte offsets fixed for the ABI of version 9.0.2 and refuses to demux if the library reports
different majors, so a replacement built from the same upstream release works and one built from a
different release is declined.

## 2. SDL 2.32.10 — Zlib

`libSDL2.so` and the SDL Java classes (`org.libsdl.app.*`, compiled into `classes*.dex`) are SDL
2.32.10, unmodified, from <https://libsdl.org/>. Copyright (C) 1997-2025 Sam Lantinga
<slouken@libsdl.org>. Licence: **Zlib** (`licenses/Zlib.txt`).

SDL contains third-party code under other licences, which reaches the APK inside `libSDL2.so`:

- **HIDAPI** (game-controller drivers), used under its BSD-style licence (the licence file is
  `src/hidapi/LICENSE-bsd.txt` in the SDL source), reproduced here as that licence requires:

    Copyright (c) 2010, Alan Ott, Signal 11 Software
    All rights reserved.

    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the following conditions are met:

        * Redistributions of source code must retain the above copyright notice,
          this list of conditions and the following disclaimer.
        * Redistributions in binary form must reproduce the above copyright
          notice, this list of conditions and the following disclaimer in the
          documentation and/or other materials provided with the distribution.
        * Neither the name of Signal 11 Software nor the names of its
          contributors may be used to endorse or promote products derived from
          this software without specific prior written permission.

    THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
    AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
    IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
    ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
    LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
    CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
    SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
    INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
    CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
    ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
    POSSIBILITY OF SUCH DAMAGE.

- **yuv2rgb** (`src/video/yuv2rgb`), BSD-3-Clause (`src/video/yuv2rgb/LICENSE` in the SDL source):

    Copyright (c) 2016, Adrien Descamps
    All rights reserved.

    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the following conditions are met:

    * Redistributions of source code must retain the above copyright notice, this
      list of conditions and the following disclaimer.

    * Redistributions in binary form must reproduce the above copyright notice,
      this list of conditions and the following disclaimer in the documentation
      and/or other materials provided with the distribution.

    * Neither the name of yuv2rgb nor the names of its
      contributors may be used to endorse or promote products derived from
      this software without specific prior written permission.

    THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
    AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
    IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
    DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
    FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
    DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
    SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
    CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
    OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
    OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

- **libc++abi and libunwind** from the Android NDK (statically linked into `libSDL2.so`, and
  libunwind also reached from `libmain.so`): **Apache-2.0 WITH LLVM-exception**
  (`licenses/Apache-2.0.txt`, `licenses/LLVM-exception.txt`), Copyright (c) the LLVM Project
  contributors.

## 3. SDL2_ttf 2.24.0 and FreeType 2.13.2

`libSDL2_ttf.so` is SDL2_ttf 2.24.0 (Zlib, `licenses/Zlib.txt`), Copyright (C) 1997-2025 Sam
Lantinga, built from the release tarball with HarfBuzz off. It statically contains the copy of
**FreeType 2.13.2** vendored in that tarball. FreeType is dual-licensed; PlxNative uses it under
the **FreeType License** (`licenses/FreeType.txt`).

*Portions of this software are copyright © The FreeType Project (www.freetype.org). All rights
reserved.*

## 4. libcurl 8.23.0 and OpenSSL 3.5.9

`libcurl-plx.so` is libcurl **8.23.0**, unmodified, from <https://curl.se/>, with **OpenSSL 3.5.9**,
unmodified, from <https://openssl-library.org/>, linked into it statically; no other OpenSSL file is
in the APK. The `-plx` name keeps it apart from any other libcurl on the device. It is built by
`ci/build-android-curl.sh` with the Android NDK 27.3.13750724.

libcurl: Copyright (c) 1996 - 2026, Daniel Stenberg, <daniel@haxx.se>, and many contributors.
Licence: **curl** (`licenses/curl.txt`). Two of its files, `lib/curlx/inet_pton.c` and
`lib/curlx/inet_ntop.c`, are from BIND and are used under the **ISC** licence, reproduced here as
that licence requires:

    Copyright (C) 1996-2022  Internet Software Consortium.

    Permission to use, copy, modify, and distribute this software for any
    purpose with or without fee is hereby granted, provided that the above
    copyright notice and this permission notice appear in all copies.

    THE SOFTWARE IS PROVIDED "AS IS" AND INTERNET SOFTWARE CONSORTIUM
    DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL
    IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL
    INTERNET SOFTWARE CONSORTIUM BE LIABLE FOR ANY SPECIAL, DIRECT,
    INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING
    FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
    NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION
    WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

OpenSSL: Copyright (c) 1998-2026 The OpenSSL Project Authors; Copyright (c) 1995-1998 Eric A.
Young, Tim J. Hudson. Licence: **Apache-2.0**
(`licenses/Apache-2.0.txt`).

## 5. CA certificates

**The Mozilla CA certificate store** — `assets/cacert.pem`, the bundle curl publishes at
<https://curl.se/docs/caextract.html> ("Certificate data from Mozilla as of: Tue Oct 20 13:32:40
2026 GMT", sha256 `796f1389bfea13c0ed78f663b765baaff54a508e5483786bd6ece73c5e3e3cbe`), unmodified.
It is the trust store every verified connection of the app uses on Fire TV. Licence: **MPL-2.0**, the
licence of the Mozilla source file it is extracted from (`licenses/MPL-2.0.txt`). Its source form is
`certdata.txt` in Mozilla's NSS, linked from the bundle's own header.

**ISRG Root X1, ISRG Root X2, ISRG Root YR and ISRG Root YE** — `assets/le-roots.pem`, the same file
the webOS package ships (`THIRD-PARTY-NOTICES.md` §2.7): public root certificates of the operator of
Let's Encrypt, copied unmodified from <https://letsencrypt.org/certificates/>, with no copyright
licence of their own. A `*.plex.direct` certificate the Mozilla store cannot verify is retried once
against them, with the chain, its dates and the host name all still checked.

## 6. Fonts

All four are the same files the webOS package ships, and `THIRD-PARTY-NOTICES.md` §2.3 says how
each was cut from its upstream. Licence: **SIL Open Font License 1.1**, shipped as
`licenses/OFL.txt` in the APK assets; each font's copyright statement is also in its own name table.

**Inter** — `assets/appfont.ttf` and `assets/appfont-bold.ttf`, static Regular and Bold
instances. Copyright 2016 The Inter Project Authors (https://github.com/rsms/inter). No Reserved
Font Name is declared.

**Noto Sans CJK KR** — `assets/appfont-cjk.ttf`, a static weight-400 instance with its layout
tables removed. Copyright © 2014-2021 Adobe (https://www.adobe.com/). The Reserved Font Name is
"Source", which this package does not use. "Source is a trademark of Adobe in the United States
and/or other countries."

**Noto Emoji** — `assets/appfont-emoji.ttf`, the monochrome Noto Emoji converted to TrueType.
Copyright 2013-2026 Google LLC. No Reserved Font Name is declared.

## 7. Code statically linked into `libmain.so`

**nanosvg** and **nanosvgrast**, compiled from `src/svg.c` — Copyright (c) 2013-14 Mikko Mononen
<memon@inside.org>. Licence: **Zlib** (`licenses/Zlib.txt`). The same pinned, unaltered headers as
the webOS package, with the same credited derivations (`THIRD-PARTY-NOTICES.md` §2.1).


The rest of `libmain.so` is Rust: the crates below and the Rust standard library (the pinned stable
toolchain's prebuilt `std` for the target, the same release as the webOS package's) are linked in
and redistributed in the file. Where a package offers a choice of licences, our election is stated;
election does not alter your rights under the other arms. Most crates are the ones the webOS
package links, so their rows are the same as in `THIRD-PARTY-NOTICES.md` §2.4; the Android-only
ones are `bytes`, `cesu8`, `combine`, `jni`, `jni-sys`, `log` and `thiserror` (the JNI bridge
to the Java video decoder and what it pulls in).

**Elected MIT** (`licenses/MIT.txt`), with the copyright holders required by that licence:

<!-- BEGIN GENERATED firetv:elected-mit (edit ci/third-party-crates.json, run python3 ci/gen-firetv-notices.py) -->

| Package | Version(s) | Declared licence | Copyright |
|---|---|---|---|
| The Rust standard library (`core`, `alloc`, `std`, `panic_unwind`, `unwind`, and the `rustc-std-workspace-*` shims) | rustc 1.99.1 (58cf8f9c3 2026-10-13) | MIT OR Apache-2.0 | The Rust Project Developers |
| — its in-tree backtrace support (`library/backtrace`) | 0.3.76 | MIT OR Apache-2.0 | 2014 Alex Crichton |
| — its in-tree mpmc channels (`library/std/src/sync/mpmc`) | in-tree | MIT OR Apache-2.0 | 2019 The Crossbeam Project Developers |
| addr2line | 0.27.1 | Apache-2.0 OR MIT | The `addr2line` authors |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | Jonas Schievink, oyvindln |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | David Tolnay |
| bitflags | 2.13.0 | MIT OR Apache-2.0 | 2014 The Rust Project Developers |
| bitstream-io | 4.10.0 | MIT/Apache-2.0 | 2017 Brian Langenberger |
| bytemuck | 1.25.0 | Zlib OR Apache-2.0 OR MIT | 2019 Daniel "Lokathor" Gee |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | 2015 Andrew Gallant |
| bytes | 1.12.1 | MIT | 2018 Carl Lerche |
| cesu8 | 1.1.0 | Apache-2.0/MIT | Eric Kidd (the published crate carries no licence file) |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | 2014 Alex Crichton |
| chardetng | 1.0.0 | Apache-2.0 OR MIT | Mozilla Foundation |
| combine | 4.6.9 | MIT | 2015 Markus Westerlind |
| crc | 3.4.0 | MIT OR Apache-2.0 | 2017 crc-rs Developers |
| crc-catalog | 2.5.0 | MIT OR Apache-2.0 | Akhil Velagapudi |
| crc32fast | 1.5.0 | MIT OR Apache-2.0 | 2018 Sam Rijs, Alex Crichton and contributors |
| either | 1.18.0 | MIT OR Apache-2.0 | Copyright (c) 2015 (the upstream notice names no holder) |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | Mozilla Foundation. Its encoding tables derive from the WHATWG Encoding Standard, whose data files are BSD-3-Clause (`licenses/WHATWG-BSD-3-Clause.txt`) |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | The image-rs Developers |
| flate2 | 1.1.9 | MIT OR Apache-2.0 | 2014-2026 Alex Crichton |
| gimli | 0.34.0 | MIT OR Apache-2.0 | The `gimli` authors |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | Amanieu d'Antras |
| image | 0.25.10 | MIT OR Apache-2.0 | The image-rs Developers |
| itoa | 1.0.18 | MIT OR Apache-2.0 | David Tolnay |
| jni | 0.21.1 | MIT/Apache-2.0 | 2016 Prevoty, Inc. and jni-rs contributors |
| jni-sys | 0.4.1 and 0.3.1 | MIT OR Apache-2.0 | 2015 The rust-jni-sys Developers |
| libc | 0.2.190 and 0.2.189 | MIT OR Apache-2.0 | The Rust Project Developers |
| log | 0.4.34 | MIT OR Apache-2.0 | 2014 The Rust Project Developers |
| memchr | 2.8.3 and 2.8.2 | Unlicense OR MIT | 2015 Andrew Gallant |
| miniz_oxide | 0.9.1 and 0.8.9 | MIT OR Zlib OR Apache-2.0 | 2013-2014 RAD Game Tools and Valve Software; 2010-2014 Rich Geldreich and Tenacious Software LLC; (c) 2017 Frommi; (c) 2017-2024 oyvindln |
| no_std_io2 | 0.9.4 | Apache-2.0 OR MIT | 2020-2021 Brendan Molloy |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | 2014 The Rust Project Developers |
| object | 0.39.1 | Apache-2.0 OR MIT | The `object` authors |
| png | 0.18.1 | MIT OR Apache-2.0 | 2015 nwin |
| rustc-demangle | 0.1.28 | MIT/Apache-2.0 | Alex Crichton |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers |
| serde | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | Copyright (c) 2018 The Servo Project Developers |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | Copyright (c) 2017 Robert Grosse |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | David Tolnay (the licence file names no holder) |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | 2019 Daniel "Lokathor" Gee |
| zune-core | 0.5.1 | MIT OR Apache-2.0 OR Zlib | The zune-image developers |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | The zune-image developers |

<!-- END GENERATED firetv:elected-mit -->

**MIT only — no alternative arm** (`licenses/MIT.txt`):

<!-- BEGIN GENERATED firetv:mit-only (edit ci/third-party-crates.json, run python3 ci/gen-firetv-notices.py) -->

| Package | Version | Copyright |
|---|---|---|
| bitvec | 1.1.1 | Copyright (c) 2018 myrrlyn (Alexander Payne) |
| bitvec_helpers | 4.0.2 | Copyright (c) 2026 quietvoid |
| core_maths | 0.1.1 | Copyright (c) 2024 Robert Bastian |
| dolby_vision | 3.4.0 | Copyright (c) 2026 quietvoid. Parses and rewrites Dolby Vision RPUs for the Profile 7 to 8.1 conversion (`plx_media`'s `ff.rs`); used unmodified, with no optional features |
| funty | 2.0.0 | Copyright (c) 2020 myrrlyn (Alexander Payne) |
| libm | 0.2.16 | Copyright (c) 2018 Jorge Aparicio and the third-party copyright notices preserved verbatim in `licenses/libm.txt` |
| qrcodegen | 1.8.0 | Copyright (c) Project Nayuki. The complete upstream MIT notice is preserved in `licenses/qrcodegen.txt` |
| radium | 0.7.0 | Copyright (c) 2019 kneecaw (Nika Layzell) |
| simd-adler32 | 0.3.9 | (c) 2021 Marvin Countryman |
| tap | 1.0.1 | Copyright (c) 2017 Elliot Linder |
| wyz | 0.5.1 | Copyright (c) 2018 myrrlyn (Alexander Payne) |
| zmij | 1.0.21 | David Tolnay (a Rust port of Victor Zverovich's C++ `zmij`) |
| rust-lang/libm — compiled *inside* `compiler_builtins` and exposed as intrinsics on this target | as vendored in rustc 1.99.1 | (c) 2018 Jorge Aparicio; musl libc (c) 2005-2020 Rich Felker, et al.; CORE-MATH; and the fdlibm-derived notice: (c) 1993, 2004 Sun Microsystems; (c) 2003-2011 David Schultz; (c) 2003-2009 Steven G. Kargl; (c) 2003-2009 Bruce D. Evans; (c) 2008 Stephen L. Moshier; (c) 2017-2018 Arm Limited |

<!-- END GENERATED firetv:mit-only -->

**Apache-2.0** (`licenses/Apache-2.0.txt`), the sole declared licence of `calendrical_calculations`:

<!-- BEGIN GENERATED firetv:apache (edit ci/third-party-crates.json, run python3 ci/gen-firetv-notices.py) -->

| Package | Version | Declared licence | Copyright |
|---|---|---|---|
| calendrical_calculations | 0.2.4 | Apache-2.0 | Copyright 2023 The Unicode Consortium |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | (c) Radzivon Bartoshyk |
| pxfm | 0.1.29 | BSD-3-Clause OR Apache-2.0 | (c) Radzivon Bartoshyk |

<!-- END GENERATED firetv:apache -->

**ICU4X code and compiled locale data — Unicode-3.0** (`licenses/ICU4X.txt`), which reproduces
the upstream licence files verbatim:

> Copyright © 2020-2024 Unicode, Inc.
>
> Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
> ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.

<!-- BEGIN GENERATED firetv:icu (edit ci/third-party-crates.json, run python3 ci/gen-firetv-notices.py) -->

| Package | Version |
|---|---|
| fixed_decimal | 0.7.2 |
| icu_calendar | 2.1.1 |
| icu_calendar_data | 2.1.1 |
| icu_collections | 2.1.1 |
| icu_datetime | 2.1.1 |
| icu_datetime_data | 2.1.2 |
| icu_decimal | 2.1.1 |
| icu_decimal_data | 2.1.1 |
| icu_locale | 2.1.1 |
| icu_locale_core | 2.3.0 |
| icu_locale_data | 2.1.2 |
| icu_pattern | 0.4.2 |
| icu_plurals | 2.1.1 |
| icu_plurals_data | 2.1.1 |
| icu_provider | 2.3.1 |
| icu_time | 2.1.1 |
| icu_time_data | 2.1.1 |
| ixdtf | 0.6.6 |
| litemap | 0.8.3 |
| potential_utf | 0.1.6 |
| tinystr | 0.8.4 |
| writeable | 0.6.4 |
| yoke | 0.8.3 |
| zerofrom | 0.1.8 |
| zerotrie | 0.2.5 |
| zerovec | 0.11.8 |

<!-- END GENERATED firetv:icu -->

`calendrical_calculations` implements algorithms from *Calendrical Calculations* by Reingold &
Dershowitz, Cambridge University Press, 4th edition (2018), released as Lisp code under
Apache-2.0. The registry `libm` package is an ICU4X dependency; `licenses/libm.txt` preserves its
complete upstream licence and attributions.

**Conjunctive licence — no election possible:** `compiler_builtins`, declared `MIT AND Apache-2.0
WITH LLVM-exception AND (MIT OR Apache-2.0)`. `licenses/MIT.txt`, `licenses/Apache-2.0.txt` and
`licenses/LLVM-exception.txt` together apply. It contains code derived from LLVM's compiler-rt:
work derived before 2019-01-19 is used under MIT with the copyright "Copyright (c) 2009-2016 by
the contributors listed in CREDITS.TXT"
(https://github.com/llvm/llvm-project/blob/main/compiler-rt/CREDITS.TXT); work derived after that
date is used under Apache-2.0 with the LLVM exception.

**Unicode Character Database tables in Rust `core`** (statically linked): Copyright © 1991-2024
Unicode, Inc. Licence: **UNICODE LICENSE V3** (`licenses/Unicode-3.0.txt`).

The tables above are generated, not typed: `ci/gen-firetv-notices.py` takes the versions from
`rust-modules/Cargo.lock` and `cargo tree --target armv7-linux-androideabi -p plx_android` and the
licence and copyright columns from `ci/third-party-crates.json`, and CI fails when this section
disagrees with the Android graph. Build-time machinery (proc-macro crates, build and dev
dependencies) is not linked and has no row.

## 8. Not in the APK

The APK carries no AndroidX or Android Support libraries, no Sentry or other crash-reporting
code, no libass, and none of the webOS package's subtitle fonts (`fonts/subs/`). The Android system libraries it
loads at run time (`libc`, `libm`, `libdl`, `libz`, `liblog`, OpenGL ES, OpenSL ES, AAudio) belong to the
device and are not redistributed here.
