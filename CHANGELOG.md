# Changelog

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
