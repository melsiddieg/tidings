# Tidings

A private, secure RSS and Atom feed reader for macOS, Linux, FreeBSD and OpenBSD.
It follows the feature set of KDE's Akregator: folders, tabs, OPML, archive rules and
keyboard navigation. It has no telemetry and no accounts.

Built with [Tauri 2](https://tauri.app) (Rust backend, system web view) and Svelte 5.
Source: <https://github.com/melsiddieg/tidings>

## Features

**Reading**
- RSS 0.9x/1.0/2.0, Atom and JSON Feed.
- Three layouts, like Akregator: **Widescreen** (three columns), **Normal**
  (list above the article) and **Combined** (full articles in one stream).
- Tabs. Open any article in its own tab (double-click, `O` or `Enter`).
  Links inside articles open in a **reader tab**: Tidings downloads the page,
  extracts the article text and shows it without the site's scripts, ads or cookies.
  ⌘/Ctrl-click opens a background tab, Shift-click opens your browser.
- **Full text** for feeds that only send a summary (per article, or always for a feed).
- Status filters: All, Unread, New (this session), Starred.
- **Feed types**: Podcasts, Videos, Code & releases, Discussions, Newsletters and Articles
  each get a view in the sidebar. The type is worked out on your computer from the feed's
  address (GitHub, YouTube, Reddit, Substack and others) and from what its recent items
  carry (audio means podcast); you can set it yourself in the feed's properties.
  Inside a folder (and in All articles and Starred), icons above the list show one type
  at a time, with unread counts; "Mark all as read" then marks only that type.
- **Date filter**: Today, Yesterday, Last 7 or 30 days, This year, Older than 30 days or
  a custom range. It combines with everything else, and "Mark all as read" follows it,
  so *Older than 30 days* then *Mark all as read* clears a backlog.
- Search over titles, text and authors.
- Next/previous unread across feeds, and Akregator-style keyboard navigation (`?` lists all keys).
- **Podcast player** with resume, seeking, 0.8×–2× speed and volume. Audio streams
  through Tidings' own proxy in byte ranges, and download-tracking redirects
  (Podtrac, Chartable, Podsights, OP3 and others) are skipped.
- **Videos in your own player.** YouTube, Vimeo, Dailymotion, Odysee and PeerTube videos
  play in [mpv](https://mpv.io) or [IINA](https://iina.io) (macOS) with one click or `W`.
  The site's own player never loads, so no site JavaScript, cookies or account are used.
  mpv needs [yt-dlp](https://github.com/yt-dlp/yt-dlp) for these sites. Tidings finds mpv
  or IINA by itself; you can choose another program in Settings → Reading. When several
  copies of yt-dlp are installed (say, one from apt and one from pipx or uv), mpv gets the
  newest. YouTube often breaks older yt-dlp versions, so keep it up to date. mpv's window
  opens at most 60% of the screen in each direction, unless your `mpv.conf` sets a
  window size (`autofit`, `geometry` or `window-scale`).
- **PeerTube videos play inside Tidings.** For PeerTube links and attached video files,
  Tidings picks a plain video file (up to 720p) and streams it through its own media proxy,
  like podcast audio. No PeerTube JavaScript or peer-to-peer traffic.
- **Find on PeerTube**: for a YouTube or other video, one click searches
  [Sepia Search](https://sepiasearch.org) for the title and lists PeerTube videos you can
  play instead. It only runs when you click, since it sends the title to Sepia Search
  (run by Framasoft). Matches are by title, so check the channel and length.
- Right-to-left text (Arabic, Hebrew, Persian) is detected per article.
- Light, dark, sepia and system themes, six accent colors, serif/sans/mono reading font, adjustable text size.

**Subscriptions**
- Folders with any depth. Drag and drop to put any feed or folder anywhere, including
  feeds above folders.
- Favorites: pin feeds and folders to the top of the sidebar.
- Select several feeds and folders with ⌘/Ctrl-click or Shift-click and delete them at
  once. Settings → Archive & data → **Remove all** starts over.
- Collapse or expand all folders with one click; Option/Alt-click a folder's arrow to
  open or close everything inside it. Folders from an OPML import start closed.
- Add a feed by pasting a site address. Tidings finds the feed (autodiscovery).
- **OPML import and export**, compatible with Akregator, NetNewsWire, Feedly and others.
- Per-feed properties: custom name, type, update interval, archive rule, always load full text, pause updates.
- Feed icons, unread counts on folders and on the dock/launcher icon.

**Archive**
- Keep everything, keep the newest N articles, or keep articles for N days; globally or per feed.
- Starred articles are never removed.
- Deleted articles stay deleted and do not come back on the next refresh.

## Privacy and security

| Measure | What it does |
|---|---|
| No telemetry | No analytics, crash reporting, update checks or accounts. The app only contacts the feeds you add and the pages and images of articles you open. |
| HTML sanitizing in Rust | Feed HTML goes through [ammonia](https://github.com/rust-ammonia/ammonia) twice (when stored and when shown). Scripts, styles, event handlers, forms and iframes are removed. Embedded videos become plain links. |
| Strict Content Security Policy | The web view may only run the app's own scripts. It cannot load remote scripts, styles, fonts, frames or media. |
| Image proxy | Remote images load through a built-in `tidings://` handler written in Rust. Image hosts never receive cookies or a referrer, the proxy setting applies to them, and they cannot be used to probe your local network (see below). Images can be turned off; then they load only when you click "Load images". |
| Tracking cleanup | `utm_*`, `fbclid`, `gclid` and similar parameters are removed from links. Known tracking pixels are dropped. |
| Podcast audio | Streamed by Rust in byte ranges with tracking redirects removed. On macOS through `tidings://`; on Linux and the BSDs through a server that listens only on 127.0.0.1 and only answers URLs carrying a random secret token, because WebKitGTK's media engine (GStreamer) cannot read custom URL schemes. |
| Reader mode for links | Linked pages are fetched by Rust and reduced to the article. No site JavaScript runs. |
| Videos | PeerTube videos and video files play in a `<video>` element fed by the same Rust media proxy as podcasts: the video host never talks to the web view. Other videos are played by mpv or IINA, never by an embedded site player. The player makes its own connections. With a proxy set, Tidings passes it to mpv or IINA, and refuses to start a player when the video would bypass the proxy (mpv cannot use SOCKS proxies such as Tor). Video links follow the same local network rule as reader tabs. |
| No navigation | The window can only show the app itself. All other navigation and pop-ups are blocked; links open in a reader tab or your browser. |
| Minimal permissions | The UI has four Tauri permissions (listen to events, drag and maximize the window). File dialogs and file access happen in Rust only. |
| Local network protection | A feed cannot make the app send requests into your local network. Feeds, pages, favicons, images and audio may only reach public addresses; private and local ones are refused in the first URL, in every redirect and in DNS answers. The one exception is a feed you add yourself on a private host (a NAS or a self-hosted reader): Tidings may reach that host for the feed and for links to the same host. With a proxy, the proxy does DNS lookups, so only literal private addresses and local names are refused. |
| Network hardening | rustls TLS, no cookie store, no `Referer` header, size limits on every download, generic user agent, optional HTTPS-only mode, optional HTTP/SOCKS5 proxy (works with Tor: `socks5h://127.0.0.1:9050`). |
| Local data | One SQLite database, file mode `0600` in a `0700` folder. `secure_delete` is on, so removed articles are overwritten on disk. |

Data folder:
- macOS: `~/Library/Application Support/org.tidings.reader/`
- Linux, FreeBSD and OpenBSD: `~/.local/share/org.tidings.reader/`

## Installing on Linux

Linux packages for x86_64 come from the GitHub Actions build (Actions tab, artifact
`tidings-Linux`) or from `scripts/build-linux.sh` (see below).
They need glibc 2.35 or newer, so they work on Ubuntu 22.04 and later, Linux Mint 21+,
Pop!_OS 22.04+, Debian 12+, Fedora 36+, Arch, CachyOS, EndeavourOS and Manjaro.

**Ubuntu, Mint, Pop!_OS, Debian** (installs the dependencies too):

```bash
sudo apt install ./tidings_0.1.0_amd64.deb
```

**CachyOS, Arch, EndeavourOS, Manjaro:** install the system web view once, then use
the portable tarball (installs into `~/.local`, no root needed):

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 xdotool
tar xzf tidings-0.1.0-linux-x86_64.tar.gz
./tidings-0.1.0-linux-x86_64/install.sh
```

Remove it later with `./install.sh --uninstall`.

**Fedora, openSUSE:**

```bash
sudo dnf install ./Tidings-0.1.0-1.x86_64.rpm
```

**Any distribution:** the AppImage carries its own libraries:

```bash
chmod +x Tidings_0.1.0_amd64.AppImage
./Tidings_0.1.0_amd64.AppImage
```

## Installing on FreeBSD and OpenBSD

There are no BSD packages on the releases page. Build the portable tarball on the
machine itself (see "Building on FreeBSD and OpenBSD" below), or take it from the
GitHub Actions `BSD` workflow (artifacts `tidings-FreeBSD` and `tidings-OpenBSD`), then
install it into `~/.local`:

```sh
tar xzf release/freebsd/tidings-0.7.1-freebsd-x86_64.tar.gz
./tidings-0.7.1-freebsd-x86_64/install.sh
```

For podcasts and videos inside Tidings, install the GStreamer plugins listed in the next
section; without them episodes fail with "This episode could not be played", and videos
offer to open in your video player instead.

For "Watch video" (YouTube and other sites in mpv), install mpv and yt-dlp:

```sh
sudo pkg install mpv yt-dlp     # FreeBSD
doas pkg_add mpv yt-dlp         # OpenBSD
```

The yt-dlp package lags behind YouTube's changes. If videos stop opening, install a newer
yt-dlp into `~/.local/bin` (for example with `pipx install yt-dlp`); Tidings gives mpv
the newest copy it finds.

## Building

### Requirements

- Rust 1.89 or newer (`rustup update`)
- Node.js 20 or newer

**macOS:** Xcode Command Line Tools (`xcode-select --install`).

**Linux (Debian/Ubuntu):**

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev librsvg2-dev
```

**Linux (Fedora):**

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libxdo-devel librsvg2-devel
sudo dnf group install "c-development"
```

**Linux (Arch):**

```bash
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl xdotool librsvg
```

### Run in development

```bash
npm install
npm run tauri dev
```

### Build a release

```bash
npm run tauri build
```

### Building on FreeBSD and OpenBSD

Tested on FreeBSD 15.0 (GhostBSD 26.1) and OpenBSD 8.0, amd64. The Tauri CLI has no BSD
binaries, so use `scripts/build-bsd.sh`. It builds the UI, runs `cargo build --release`
with the feature `tauri build` would use, and writes
`release/<os>/tidings-<version>-<os>-x86_64.tar.gz`.

**FreeBSD:**

```sh
sudo pkg install webkit2-gtk_41 gtk3 xdotool pkgconf git node24 npm-node24 \
    gstreamer1-plugins-good gstreamer1-plugins-ugly gstreamer1-libav
curl -sSf https://sh.rustup.rs | sh     # or: sudo pkg install rust
scripts/build-bsd.sh
```

On GhostBSD the base system is split into packages, and the `-dev` parts needed for
linking may be missing. If the build stops with `unable to find library -lexecinfo` or
`Package 'krb5-gssapi' ... not found`, install `GhostBSD-libexecinfo-dev` and
`GhostBSD-kerberos-dev`. If the `rust` package fails with `version FBSD_1.9 required`,
the package repository is newer than your base system; update the system or use rustup.

**OpenBSD:**

```sh
doas pkg_add webkitgtk41 gtk+3 xdotool node rust git \
    gst-plugins-good gst-plugins-ugly gst-libav
scripts/build-bsd.sh
```

On OpenBSD the script raises the per-process memory limit (`ulimit -d`) to the hard
limit, because linking needs more than the 1.5 GB default of the `staff` login class.
It also installs Rolldown's WebAssembly build, since Vite's bundler has no native
OpenBSD binary, and the stylesheet is left unminified because lightningcss has no
OpenBSD binary either. A release build takes about an hour on 2 CPUs; `LTO=false`
makes it much faster.

**CI:** `.github/workflows/bsd.yml` runs the same script plus `cargo test` inside
FreeBSD 15 and OpenBSD 8.0 virtual machines on GitHub's Linux runners
([vmactions](https://github.com/vmactions)). It runs for release tags, from the Actions
tab, and for branch pushes that change the BSD build. The tarballs stay workflow
artifacts; they are not attached to releases, because OpenBSD binaries only run on the
release they were built on.

The output is in `src-tauri/target/release/bundle/`:
- macOS: `macos/Tidings.app` and `dmg/Tidings_0.1.0_*.dmg`
- Linux: `deb/`, `rpm/` and `appimage/`

To build only the macOS app bundle (faster): `npm run tauri build -- --bundles app`.

### Build Linux packages from macOS (or any machine)

`scripts/build-linux.sh` builds the Linux x86_64 packages inside a container with
Podman or Docker. On Apple silicon it cross-compiles natively instead of emulating
x86_64, so it is fast. It uses Ubuntu 22.04 so the result runs on older and newer
distributions alike.

```bash
scripts/build-linux.sh
```

Output in `release/linux/`: `.deb`, `.rpm`, `.AppImage`, a portable `.tar.gz` and
`SHA256SUMS`. If the container VM has little memory, limit the build:

```bash
MEMORY=1100m JOBS=2 LTO=false scripts/build-linux.sh
```

### Releases

Downloads are on the [releases page](https://github.com/melsiddieg/tidings/releases).
Versions follow [Semantic Versioning](https://semver.org); changes are listed in
[CHANGELOG.md](../CHANGELOG.md).

To publish a new version:

```bash
scripts/bump-version.sh 0.2.0      # sets the version in all four files
# add a "## [0.2.0] - YYYY-MM-DD" section to CHANGELOG.md, then:
git commit -am "Release 0.2.0"
git tag v0.2.0 && git push && git push --tags
```

Pushing the tag starts `.github/workflows/build.yml`. It checks that the tag, the
version files and the changelog agree, builds Linux and macOS packages, and publishes
a release with all files and `SHA256SUMS`. You can also start the workflow by hand
from the Actions tab to build without releasing.

The macOS build is not signed. The first time, right-click the app and choose **Open**,
or remove the quarantine flag: `xattr -dr com.apple.quarantine Tidings.app`.

### Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
npm run check
```

The Rust tests cover feed parsing, HTML sanitizing, tracker removal, the image
proxy rules, reader-mode extraction, every database query and an OPML round trip.

To check video playback on your machine (it opens mpv or IINA and plays a 19-second
YouTube clip):

```bash
TIDINGS_LIVE_PLAYER=1 cargo test --manifest-path src-tauri/Cargo.toml live_player -- --ignored
```

### Working on the UI only

`npm run dev` serves the UI at http://localhost:1420 with a fake in-memory backend
(`src/lib/mock.ts`), so you can work on the interface in a normal browser. The fake
backend is only loaded in development and is not part of release builds.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `J` / `↓`, `K` / `↑` | Next / previous article |
| `N` or `+`, `P` or `-` | Next / previous unread (continues into the next feed) |
| `Space` | Scroll the article, then go to the next unread |
| `O` / `Enter` | Open the article in a tab |
| `V` | Open in browser |
| `W` | Watch the video in mpv or IINA |
| `F` | Load the full article |
| `I` | Show or hide images |
| `S` | Star |
| `M` | Mark read / unread |
| `C` | Copy link |
| `Shift+A` | Mark all as read |
| `R` / `Shift+R` | Refresh this feed or folder / refresh all |
| `Delete` | Delete article |
| `/` | Search |
| `⌘/Ctrl+Click`, `Shift+Click` in the sidebar | Select several feeds or folders (`Delete` removes them, `Esc` clears) |
| `Option/Alt+Click` on a folder arrow | Open or close the folder and every folder inside |
| `⌘/Ctrl+W`, `Ctrl+Tab`, `⌘/Ctrl+1…9` | Close tab, next tab, go to tab |
| `⌘/Ctrl+N`, `⌘/Ctrl+,` | Add feed, settings |
| `?` | Show all shortcuts |

## Project layout

```
src/                     Svelte UI
  lib/store.svelte.ts    App state and actions
  lib/api.ts             Typed calls to the Rust commands
  components/            Sidebar, article list, article view, tabs, dialogs
src-tauri/src/
  main.rs                Window setup, navigation lock, command registration
  commands.rs            Commands the UI can call
  db.rs                  SQLite schema and queries
  feeds.rs               Fetching, parsing and storing feeds; favicons
  discover.rs            Feed autodiscovery from a web page
  opml.rs                OPML import and export
  reader.rs              Reader mode (article extraction)
  sanitize.rs            HTML cleaning and tracker removal
  protocol.rs            tidings:// image proxy and feed icons
  net.rs                 HTTP clients and network safety checks
  scheduler.rs           Background refresh and archive cleanup
  menu.rs                macOS menu bar
  video.rs               External video player (mpv, IINA)
  peertube.rs            PeerTube video files and Sepia Search
  media.rs               Audio and video streaming in byte ranges, with read-ahead
```

## Troubleshooting

- **Linux: blank or flickering window.** With NVIDIA's driver, Tidings already turns off
  WebKit's DMA-BUF renderer by itself. On other setups (some Wayland compositors),
  start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1 tidings`.
- **A feed shows a warning icon.** Hover it or open its properties to see the error.
  Some sites block unknown user agents; you can change the user agent in
  Settings → Privacy & network.
- **A page does not open in a reader tab.** Not every page has article text that can be
  extracted. Use "Open in browser".
