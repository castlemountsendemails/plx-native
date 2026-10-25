# Changelog

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
