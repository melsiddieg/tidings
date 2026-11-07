# Changelog

All notable changes to Tidings are listed here. Versions follow
[Semantic Versioning](https://semver.org): MAJOR.MINOR.PATCH.

## [Unreleased]

### Added
- A GitHub Actions workflow (`BSD`) that builds and tests Tidings in FreeBSD 15 and
  OpenBSD 8.0 virtual machines and keeps the tarballs as workflow artifacts.

## [0.7.1] - 2026-11-08

### Fixed
- "Watch video" could open mpv and then close it with no video and no error, mostly for
  YouTube. Tidings gave mpv a PATH with the system folders before `~/.local/bin`, so mpv
  ran an old yt-dlp from the distribution (apt) instead of a newer one installed with
  pipx or uv. That old yt-dlp found the video, but its streams did not open.
  - Programs are now looked up in `~/.local/bin` and `~/bin` first, as Debian's and
    Ubuntu's `~/.profile` does.
  - When several copies of yt-dlp are installed, mpv gets the newest, whichever folder
    it is in.
  - A failure after the first 20 seconds is now shown too. Before, Tidings stopped
    watching mpv after that.
  - The error names the stream host that did not open. When the yt-dlp mpv used is more
    than three months old, the error gives its version and path and says to update it.
- mpv opened 4K and vertical videos (YouTube Shorts) in a window as large as the video,
  which ran off the screen. mpv's window now opens at most 60% of the screen's width and
  height; smaller videos keep their size. If your `mpv.conf` sets a window size
  (`autofit`, `geometry` or `window-scale`), Tidings leaves it to that. IINA is unchanged.

## [0.7.0] - 2026-11-08

### Added
- Filter a folder by feed type. When a folder (or All articles, or Starred) holds more
  than one kind of feed, icons above the list show each type with its unread count:
  podcasts, videos, code & releases, discussions, newsletters and articles. Click one to
  see only that type; "Mark all as read" then marks only those articles. Choosing another
  feed or folder goes back to all types.

## [0.6.0] - 2026-11-07

### Added
- Select several feeds and folders in the sidebar with ⌘/Ctrl-click (one by one) or
  Shift-click (a range), then delete them in one step: with the "Delete…" button that
  appears at the bottom of the sidebar, the `Delete` key, or the right-click menu.
- Settings → Archive & data → "Remove all…" deletes every subscription to start over.
  Settings stay.
- Expand or collapse all folders: a button next to "Subscriptions", the sidebar's "…"
  menu, and "Expand/Collapse all inside" on a folder's right-click menu. Option/Alt-click
  on a folder's arrow opens or closes the folder and everything inside it.

### Changed
- Folders created by an OPML import start closed, so a large import does not fill the
  sidebar.

## [0.5.0] - 2026-11-07

### Added
- Watch videos in mpv or IINA. Articles that link to a YouTube, Vimeo, Dailymotion, Odysee
  or PeerTube video, or carry a video file, show a "Watch video" card and a toolbar button
  (shortcut `W`). Clicking a video link inside an article also opens the player. The
  site's own player never loads, so no site JavaScript, cookies or account are involved.
  Tidings finds mpv or IINA by itself, or uses the program set in Settings → Reading. mpv
  needs yt-dlp for these sites. With a proxy set, only mpv and IINA with an `http://` proxy
  can play videos; Tidings refuses otherwise, so videos never bypass a SOCKS proxy such as
  Tor.
- PeerTube videos play inside Tidings. For a PeerTube link, Tidings asks the instance for
  the video's files, picks one up to 720p, and streams it through its media proxy like
  podcast audio. Attached video files play the same way.
- "Find on PeerTube" for YouTube and other videos: searches Sepia Search for the title and
  lists PeerTube videos to play here or in mpv/IINA. It runs only when you click it.

### Changed
- Videos and podcasts buffer less. The media proxy keeps the last 48 MB it fetched in
  memory, in 2 MB blocks, and fetches the next 6 MB in the background while you play.
  Before, every request from the player waited for a new round trip to the host.

## [0.4.2] - 2026-11-07

### Fixed
- The macOS app could crash after running for a while. When the web view cancelled a
  `tidings://` request (an image while you switch articles, or podcast audio while it
  seeks or buffers) just as the answer arrived, WebKit threw an error that the web view
  library normally ignores. Release builds were set to stop on any Rust panic
  (`panic = "abort"`), which kept that error from being handled and ended the app. Release
  builds no longer use that setting. A panic in a background task also no longer closes
  the app on any system.

## [0.4.1] - 2026-11-07

### Security
- Feeds could make Tidings send requests into your local network. Feed refreshes, full
  text, reader tabs and favicons used a network client with no private-address check, so a
  feed could redirect its refresh to an address such as `192.168.1.1`, or link to one from
  an article set to "always load full text". These requests now follow the same rule as
  images: private and local addresses are refused, in the first URL, in every redirect and
  in DNS answers. A feed you add yourself on a private host (a NAS or a self-hosted reader)
  still works, and so do links to that same host.
- With a proxy set, redirects to private addresses and local names are now refused too,
  for images and audio as well as pages. Before, a proxy turned these checks off.
- A refused request now says "Blocked: this address is on a private or local network"
  instead of a generic connection error.

## [0.4.0] - 2026-11-06

### Added
- Feed types. Podcasts, Videos, Code & releases, Discussions, Newsletters and Articles get
  their own views in the sidebar. Tidings works the type out from the feed's address and
  from the media in its recent items, and you can choose it yourself in the feed's
  properties. Existing feeds are sorted when you upgrade.
- A date filter next to the read-state filter: Today, Yesterday, Last 7 days, Last 30 days,
  This year, Older than 30 days or a custom range. "Mark all as read" follows it.

## [0.3.2] - 2026-11-06

### Added
- FreeBSD and OpenBSD. `scripts/build-bsd.sh` builds Tidings natively on either system
  and makes a portable tarball; `install.sh` is now plain POSIX sh so it works there too.
  Podcast audio uses the same 127.0.0.1 server as Linux, since the BSDs also play media
  through WebKitGTK and GStreamer.

### Fixed
- Podcast episodes on Linux stopped after the first minute or two. The audio server
  answered each request with one 2 MB slice, and WebKitGTK took that slice for the whole
  file. It now answers like a normal web server, with one response up to the end of the
  file, filled slice by slice from the audio host. Seeking had the same problem and is
  fixed too.
- After seeking in an episode on Linux, the play button kept showing a spinner while the
  audio was already playing.

## [0.3.1] - 2026-11-06

### Fixed
- Podcast playback on Linux. WebKitGTK plays media through GStreamer, which cannot read
  the app's `tidings://` URLs, so episodes failed with "This episode could not be played".
  On Linux, audio now comes from a small server that listens only on 127.0.0.1, on a
  random port, and only answers URLs that carry a random secret token. It uses the same
  media proxy as before, so tracker stripping and the proxy setting still apply.

## [0.3.0] - 2026-11-06

### Added
- Favorites. Right-click a feed or folder and choose "Add to Favorites"; favorites
  appear in their own sidebar section under All articles and Starred.

### Changed
- Feeds and folders now share one order, so a feed can sit above or between folders.
  Drag any feed or folder onto the top or bottom edge of another row to place it
  before or after; drop on the middle of a folder to move it inside. Your existing
  order is kept when you upgrade.

## [0.2.0] - 2026-11-06

### Added
- Podcast player. Episodes with audio show a Play button; a player bar at the bottom
  of the window has play/pause, back 15 s, forward 30 s, seeking, speed (0.8×–2×) and
  volume. Playback resumes where you stopped, and finished episodes are marked read.
  Shift+P plays or pauses; system media keys work where the web view supports them.
- Audio streams through Tidings' own media proxy in byte ranges, so seeking works,
  podcast hosts never see the web view, and the proxy setting (for example Tor)
  applies to audio too.
- Known podcast tracking redirects (Podtrac, Chartable, Podsights, Magellan, OP3,
  Podscribe and others) are skipped when "Remove tracking parameters" is on.

### Changed
- Feeds up to 50 MB are accepted (large podcast archives such as BSD Now are about 19 MB).
- Feeds are parsed on a background thread, so big feeds do not slow the app down.

## [0.1.0] - 2026-11-06

First release.

### Reading
- RSS 0.9x/1.0/2.0, Atom and JSON Feed.
- Widescreen, normal and combined layouts.
- Articles and linked pages open in tabs; links open in a script-free reader mode.
- Full-text extraction for feeds that only send summaries (per article or per feed).
- Filters (all, unread, new, starred), search, next/previous unread across feeds.
- Keyboard navigation modeled on Akregator (`?` lists the shortcuts).
- Right-to-left text support; light, dark, sepia and system themes; six accent colors.

### Subscriptions
- Nested folders; drag and drop to move feeds and folders, and to reorder folders
  (drop on the top or bottom edge of a folder to place it before or after).
- Feed autodiscovery from a website address.
- OPML import and export.
- Per-feed name, update interval, archive rule, full-text and pause options.

### Privacy and security
- No telemetry, accounts or update checks.
- Feed HTML cleaned in Rust; strict Content Security Policy; navigation locked to the app.
- Images loaded through a local proxy that blocks private network addresses.
- Tracking parameters and known tracking pixels removed.
- Optional HTTP/SOCKS5 proxy (Tor), HTTPS-only mode, generic user agent.
- Local SQLite database with owner-only permissions and `secure_delete`.

### Platforms
- macOS 11+ (universal: Apple silicon and Intel).
- Linux x86_64: `.deb`, `.rpm`, AppImage and a portable tarball (glibc 2.34+).

[Unreleased]: https://github.com/melsiddieg/tidings/compare/v0.7.1...HEAD
[0.7.1]: https://github.com/melsiddieg/tidings/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/melsiddieg/tidings/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/melsiddieg/tidings/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/melsiddieg/tidings/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/melsiddieg/tidings/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/melsiddieg/tidings/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/melsiddieg/tidings/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/melsiddieg/tidings/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/melsiddieg/tidings/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/melsiddieg/tidings/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/melsiddieg/tidings/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/melsiddieg/tidings/releases/tag/v0.1.0
