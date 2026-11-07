# Changelog

All notable changes to Tidings are listed here. Versions follow
[Semantic Versioning](https://semver.org): MAJOR.MINOR.PATCH.

## [Unreleased]

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

[Unreleased]: https://github.com/melsiddieg/tidings/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/melsiddieg/tidings/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/melsiddieg/tidings/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/melsiddieg/tidings/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/melsiddieg/tidings/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/melsiddieg/tidings/releases/tag/v0.1.0
