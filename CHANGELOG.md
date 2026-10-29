# Changelog

## [0.2.17] - 2026-10-29

### Added
- The Fire TV CI artifact now carries the unstripped `libmain.so` and `libcurl-plx.so` under `symbols/`, so the frames in a crash dump from a stick can be named. The build fails if `libmain.so` has no symbol table.

## [0.2.16] - 2026-10-28

### Fixed
- Home no longer asks your server for a row it doesn't show, several times a second, for as long as the app is open. The loop slowed the app and loaded the server on both the LG (about 3 requests a second) and Fire TV (about 9). Home still shows only Continue Watching.

### Changed
- Shared code, for the LG check: Home's refresh now keeps only the Continue Watching row and drops the server's other Home rows as soon as they arrive. The LG app gets this change too.

## [0.2.15] - 2026-10-28

### Added
- On Fire TV the saved sign-in is now sealed by the Keystore: the account token, server, profiles and their tokens are encrypted, and the language, settings, Home pins, recent searches and client id are kept as plain JSON beside them. A sign-in saved by an earlier Fire TV build is sealed the first time the app starts. Not yet tried on any stick.
- If the Keystore can't open the sealed part (its key is gone, or it fails for the moment), the Fire TV app starts signed out, with no error, and keeps the language and screen resolution, as the LG app does. Signing in again replaces the sealed part. Anything else saved meanwhile leaves the sealed part untouched, so a Keystore failure that passes doesn't cost the sign-in.
- The Fire TV `--es secure remove` test run now deletes the session's Keystore key even when no earlier `--es secure test` blob exists.

### Changed
- Shared code, for the LG check: on Fire TV only, the session file is read and written through the sealed record, and a failed session write is not followed by the fallback `auth.json` copy. The LG app reads and writes its session as before.

## [0.2.14] - 2026-10-28

### Added
- The Fire TV remote now drives the app: D-pad, OK, BACK, play/pause, rewind and fast-forward, and MENU or holding BACK for about a second for Exit. Search can be typed with the Fire keyboard, and its delete key edits the search. BACK closes the keyboard. Not yet tried on any stick.

### Changed
- Shared code, for the LG check: on Fire TV only, key events and typed text are read where stock SDL puts them, the Fire keyboard's per-letter key events are ignored (its text is used), and a keyboard closed by Fire OS ends editing. The LG app reads keys and text as before.

## [0.2.13] - 2026-10-27

### Added
- The Fire TV app now starts the real PlxNative app when it's opened from the Fire TV home screen, or with `am start` and no extras. The test runs (`clip`, `secure`, `net`) still start from their extras. The remote doesn't reach the app yet, so you can't get past the screen after sign-in, and saved sign-ins aren't sealed by the Keystore yet. Not yet tried on any stick.
- On Fire TV the app keeps its sign-in and settings in a `state` folder inside its own storage, and ships all four of the LG app's fonts (Inter regular and bold, Noto Sans CJK, Noto Emoji), copied into storage after each install or update.
- The Fire TV app shows up on plex.tv and on your server as `PlxNative (Fire TV)` (`PlxNative debug (Fire TV)` for the debug build), on platform Android with the stick's Android version and device `Amazon Fire TV`. The LG app's name and details are unchanged.
- The Fire TV licence notice now covers the three extra fonts, nanosvg and the Rust libraries the app brings in.

## [0.2.12] - 2026-10-27

### Added
- The Fire TV app carries its own libcurl 8.23.0 with OpenSSL 3.5.9, and checks server certificates against the Mozilla CA list it ships with (plus the Let's Encrypt roots it already used as a fallback for `*.plex.direct` servers). The first time the app starts after each install or update, it copies both certificate files into its own storage. The LG app is unchanged.
- A Fire TV test run of the network (`--es net check --es server <your server's plex.direct address>`) fetches plex.tv, your server's `/identity` and a site with an expired certificate. It shows a pass or fail line for each on screen and in logcat. It's the only use so far, since sign-in isn't wired up yet, and it hasn't been tried on any stick.
- The Fire TV licence notice now covers libcurl, OpenSSL and the CA list. The curl and MPL-2.0 licence texts are added; they also ship in the LG package, unused there.

## [0.2.11] - 2026-10-27

### Added
- The Fire TV app can now seal a secret with a key held in the Android Keystore, open it again and delete the key. It is not yet reachable from sign-in: only a test run of the app can use it (`--es secure test`, then `open`, then `remove`), which shows a pass or fail line for each step on screen and in logcat. Not yet tried on any stick.
- A saved session can now be marked as sealed by the Android Keystore. The LG app doesn't use this and still opens its own sessions as before.

## [0.2.10] - 2026-10-27

### Fixed
- AAC audio with 7.1 channel configuration 12 (as written by newer ffmpeg) plays with sound instead of silently. The stream's header is now written as the equivalent 8-channel configuration 7. Not yet tried on any stick.

## [0.2.9] - 2026-10-26

### Fixed
- On the Fire TV 4K HEVC test clip, the picture and sound start again after a pause or a jump. The stick's 4K decoder holds only about half a second of video, less than the player waits for before playing again, so the video side now keeps up to about 2 s of video queued in front of the decoder. Not yet tried on any stick.

## [0.2.8] - 2026-10-26

### Fixed
- The Fire TV test clip no longer plays with a frozen picture and stuttering sound. The on-screen label takes about a third of a second to draw and the player is not fed meanwhile, so it is now redrawn once a second instead of four times. Not yet tried on any stick.
- When the clip's sound has all played, the audio clock stops at its end instead of running on past it.

### Added
- Every 10 s the clip harness logs how long the player, drawing the label and handing audio to the track took.

## [0.2.7] - 2026-10-26

### Added
- The Fire TV test clip now plays with sound: AC-3 goes to the TV or receiver as it is (HDMI passthrough) and AAC is decoded on the stick, and the picture is timed against the audio so they stay in step. E-AC-3 and DTS are recognised but still play silent, as does AC-3 on a sink that reports it can't take it. Not yet tried on any stick.
- In the clip harness, Play/Pause pauses and resumes, and D-pad left/right jump 10 s back or forward. A new label line shows how the audio is played and how far the picture is from it (last and largest).
- At each clip start the app logs the audio formats each HDMI output reports, plus the stick's AAC decoders.

## [0.2.6] - 2026-10-25

### Added
- The Fire TV APK now carries its own third-party notice, `THIRD-PARTY-NOTICES-FIRETV.md`, and the licence texts it names, in the app's assets. It covers FFmpeg, SDL2, SDL2_ttf with FreeType, the Inter font, and the Rust crates the APK links. CI generates the crate tables from the Android build and fails a build whose APK is missing the notice or a licence text it names.

## [0.2.5] - 2026-10-25

### Updated
- The Fire TV clip harness takes the clip's own size (`--es size 1920x804`) and tells the player engine, as a real Plex item's size does, so a 1080p H.264 clip is declared at 1080p and not at the widest raster the engine knows.

## [0.2.4] - 2026-10-25

### Updated
- The Fire TV clip decoder now caps the size it opens to what the stick's H.264 decoder reports it supports, instead of always asking for the clip's largest raster (3840x2160). Aimed at the HD and 3rd gen sticks, where the decoder refused to open; not yet confirmed on either stick.

## [0.2.3] - 2026-10-25

### Added
- The Fire TV test clip is now decoded and shown: the stick's own hardware decoder plays the clip's video under the on-screen label, which now names the decoder and counts the frames fed, shown and dropped as late. There is still no sound, and BACK stops it as before.

## [0.2.2] - 2026-10-24

### Added
- The Fire TV APK can be started on a test clip over adb (`am start -S ... --es clip <url>`): the app's own player engine streams it from a plain-http server and shows the clip, its resolution, the position and how much it has read on screen; BACK stops it and returns to the skeleton screen. Nothing is decoded or shown yet; the engine's log goes to logcat. Started without a clip, the app is the skeleton as before.

## [0.2.1] - 2026-10-24

### Added
- The Fire TV APK now carries its own FFmpeg, built for the sticks by CI, and CI fails a build whose APK is missing it. The FFmpeg source and its build script come in the same download as the APK. Nothing in the app uses FFmpeg yet.

## [0.2.0] - 2026-10-23

### Added
- The Fire TV APK is now signed: CI names it `plxnative-0.10.0-dev-firetv-debug.apk` and fails the build unless the APK has the armeabi-v7a library, minSdk 25, targetSdk 28, native libraries extracted at install, backup off, a Leanback launcher entry and the expected signing key. Still not installed on any stick.
- A manual CI run can also build a release-signed, non-debuggable skeleton under `com.beb.plxnative` with a chosen versionCode, for the downgrade test.

## [0.1.9] - 2026-10-23

### Added
- The Fire TV skeleton reacts to the remote: it shows the last key pressed on screen and logs it, mapped as the port's key table says (MENU, or BACK held for a second, is Exit). Still not installed on any stick.

## [0.1.8] - 2026-10-23

### Added
- A Fire TV skeleton APK, built by CI's new `firetv` job: it launches and draws a line of text on a fixed 1920×1080 surface. It has no Plex features yet and is not installed on any stick.

## [0.1.7] - 2026-10-22

### Added
- A favorite whose server hasn't answered yet this run stays in the rail at its saved place, drawn from its saved name. It dims once its server is found offline or refusing access. OK on it does nothing until its server answers.

### Fixed
- Reordering favorites no longer drops the favorites of a server that hasn't answered from the saved order (they used to move to the end when the server came back).

## [0.1.6] - 2026-10-22

### Fixed
- The rail's "Hold OK to reorder" hint still appears after a card's item menu has been opened in the same run.

## [0.1.5] - 2026-10-22

### Fixed
- A library deleted on its server now drops out of the rail, More and the favorites order the next time that server's library list is re-read (e.g. Check shares), rather than lingering until the app restarts.

### Updated
- Moving a favorite in the rail is cancelled, not saved, if the library list is rebuilt while it is being moved.

### Added
- Home's empty Continue Watching reads "Nothing in progress" / "Pick a library from the left." instead of the old discovery-hub wording, with no action — focus stays on the rail.
- A library whose server has refused this account's access (share revoked or grant gone stale) reads "{server} isn't shared with you right now" with a Check shares action, told apart from a server that is simply unreachable.
- A favorite or More group on an unreachable or access-refused server draws dimmed, and More's group header names why ("{server} · offline" / "{server} · not shared").

## [0.1.4] - 2026-10-21

### Added
- Hold OK on a favorite in the nav rail to reorder it: Up/Down move it, OK saves the new order, Back cancels.
- A one-time hint teaches the hold-to-reorder gesture, shown once per install.

## [0.1.3] - 2026-10-21

### Fixed
- The developer frame counter in the top-right corner is gone from this build.
- Home's focused Continue Watching card, and its caption, no longer sit partly behind the collapsed rail.

## [0.1.2] - 2026-10-21

### Added
- A More item on the rail, below the favorites. OK opens a list of every library, grouped by server, in place; OK again closes it. OK on a library opens it, favorite or not.
- Each library in More has a star at its right. Right moves to the star, OK stars or unstars the library, Left goes back, and Right again returns to the page.
- The rail keeps the favorites in a stored order for each profile. A new favorite, from More, Settings or the first-run picks, goes to the end.

### Updated
- Back on the rail closes More's list first.
- A library that is not a favorite marks its row in More as the current page, and Back from it lands there.
- OK on the current page's rail item moves focus back into that page, on Search, Watchlist and library pages too.
- Unstarring the library you are viewing no longer switches the page to another library.

## [0.1.1] - 2026-10-21

### Fixed
- With many favorite libraries, the rail scrolls its library list, while Home, Search, Watchlist and Profile stay put, instead of piling the items on top of each other.
- Back at the end of the chain closes the app on TVs that refuse to show their launcher (such as webOS 4.4), instead of doing nothing.

## [0.1.0] - 2026-10-21

### Added
- A nav rail down the left edge of Home, Search, Watchlist and the library pages: Home, Search, Watchlist, one item per favorite library, and Profile at the bottom (opens the account menu).
- The rail widens with labels while it has focus and pushes the page aside. Left from a page's left edge enters it, and Right returns to where you were.

### Updated
- Home shows only Continue Watching. Focus starts on its first card, or on the rail's Home item when nothing is in progress.
- Back steps onto the rail before leaving Search, Watchlist or a library, then goes to Home. Back from the rail on Home hands the screen back to the TV.
- The account menu opens beside the rail's Profile item.
- Up from the top of a page stays on the page.

### Removed
- The top tab strip and the profile chip.
- Home's hero banner and discovery shelves.
- Library names and "+N" in the library page header (libraries are picked from the rail).
