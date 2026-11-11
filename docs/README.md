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
- **Listen to videos.** The Listen button (or `L`) plays only the sound of a YouTube, Vimeo
  or similar video in the podcast player, with speed control, saved position and media keys.
  yt-dlp finds the audio stream; Tidings streams it through its own media proxy, like a
  podcast, so the video site never talks to the web view. It uses much less data than the
  video and works with any proxy, Tor included. Needs yt-dlp (on macOS, the copy inside IINA
  is used when no other is installed).
- **PeerTube videos play inside Tidings.** For PeerTube links and attached video files,
  Tidings picks a plain video file (up to 720p) and streams it through its own media proxy,
  like podcast audio. No PeerTube JavaScript or peer-to-peer traffic.
- **Find on PeerTube**: for a YouTube or other video, one click searches
  [Sepia Search](https://sepiasearch.org) for the title and lists PeerTube videos you can
  play instead. It only runs when you click, since it sends the title to Sepia Search
  (run by Framasoft). Matches are by title, so check the channel and length.
- **AI briefs with a local model**: the sparkles button above the article list writes a
  short brief of what the list shows (a folder, a feed type, unread, a date range): a few
  topics with points that link to their source articles. A language model on this
  computer or your local network writes it (Ollama, LM Studio, llama.cpp), never a cloud
  service, and it only sees titles, feed names, dates and short excerpts. Off until you
  choose a model in Settings → AI briefs.
- **Security check** (Settings → Security): shows whether Tidings, yt-dlp and WebKitGTK
  (Linux, BSD) are up to date, and how to update each one, matched to how it was installed.
  Without going online it checks how old each one is, at every start, and shows a notice
  when something needs an update. "Check now" asks GitHub for the newest releases and
  webkitgtk.org for its latest security advisory, but only when you click it.
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
| No telemetry | No analytics, crash reporting, automatic update checks or accounts. Tidings contacts the feeds you add; once per feed, the feed's website for its icon; the images of the articles in the list and of the article you open (unless images are off); and the pages and media you open. The security check in Settings reads versions on this computer; it asks GitHub and webkitgtk.org for newer versions only when you click "Check now". |
| HTML sanitizing in Rust | Feed HTML goes through [ammonia](https://github.com/rust-ammonia/ammonia) twice (when stored and when shown). Scripts, styles, event handlers, forms and iframes are removed. Embedded videos become plain links. |
| Strict Content Security Policy | The web view may only run the app's own scripts and use the app's own stylesheet. It cannot load remote scripts, styles, fonts, frames or media, and a `<style>` element added at run time is refused. Only `style` attributes are allowed inline, which the UI needs in two places. This needs WebKit from Safari 15.4 or WebKitGTK 2.36 (macOS 12.3, Ubuntu 22.04, Debian 12 and newer). |
| Image proxy | Remote images load through a built-in `tidings://` handler written in Rust. Image hosts never receive cookies or a referrer, the proxy setting applies to them, and they cannot be used to probe your local network (see below). The image host still sees your IP address (or your proxy's) and the time of the request: thumbnails in the article list are fetched when the list shows them, before you open the article. Images can be turned off; then they load only when you click "Load images". |
| AI briefs | Only with a model server on this computer or your local network: Tidings refuses public addresses, and with a proxy set it accepts only IP addresses, so no name is looked up outside the proxy. The model gets the titles, feed names, dates and short excerpts of the articles being briefed, nothing else. Article text cannot steer the brief into links or markup: the model answers in JSON that cites articles by number, Tidings keeps only citations of articles it sent, and the brief is shown as plain text. |
| Image re-encoding | Every image is decoded in Rust, with memory limits, and written out again as a new JPEG, PNG or GIF before the web view sees it. WebKit's image libraries (libwebp, libjpeg-turbo, libpng...) never parse a file from the internet, so a crafted image cannot reach a bug in them. Metadata is dropped and large images are scaled down. SVG is drawn to a PNG in Rust as well (with resvg), so WebKit's SVG code never sees a file from the internet; images that an SVG refers to are not loaded. SVG is drawn in a separate helper process with a time limit and a memory limit: a small crafted SVG can crash the drawing library or keep it busy for minutes, and then only the helper ends and that image is not shown. AVIF is not shown. |
| Tracking cleanup | `utm_*`, `fbclid`, `gclid` and similar parameters are removed from links. Known tracking pixels are dropped, and so are images marked as 0 or 1 pixel wide or high. This cannot catch every counting pixel: with images on, a feed can still tell that you loaded an article. |
| Podcast audio | Streamed by Rust in byte ranges with tracking redirects removed. On macOS through `tidings://`; on Linux and the BSDs through a server that listens only on 127.0.0.1 and only answers URLs carrying a random secret token, because WebKitGTK's media engine (GStreamer) cannot read custom URL schemes. Only single audio and video files are played. A streaming playlist (HLS, DASH and similar) is refused, whatever its address or type says: a playlist is a list of addresses that the media engine would fetch by itself, without the proxy setting and without the local network rule. |
| Reader mode for links | Linked pages are fetched by Rust and reduced to the article. No site JavaScript runs. |
| Videos | PeerTube videos and video files play in a `<video>` element fed by the same Rust media proxy as podcasts: the video host never talks to the web view. Other videos are played by mpv or IINA, never by an embedded site player. The player makes its own connections. With a proxy set, Tidings passes it to mpv or IINA, and refuses to start a player when the video would bypass the proxy (mpv cannot use SOCKS proxies such as Tor). Tidings refuses to give the player an address on your local network, but that check covers only the address in the feed: mpv, IINA and yt-dlp then make their own connections and follow redirects themselves, outside Tidings' local network rule, like a browser would. "Listen" runs yt-dlp only to find a video's audio stream, with your proxy (SOCKS included) and without your yt-dlp configuration, so a `--cookies-from-browser` line there never sends your account cookies; the audio itself streams through Tidings' media proxy. |
| No navigation | The window can only show the app itself. All other navigation and pop-ups are blocked; links open in a reader tab or your browser. |
| Minimal permissions | The UI has four Tauri permissions (listen to events, drag and maximize the window). File dialogs and file access happen in Rust only. |
| Local network protection | A feed cannot make Tidings send requests into your local network. Feeds, pages, favicons, images, audio and video may only reach public addresses; private and local ones are refused in the first URL, after every rewrite (such as removing a podcast tracking prefix), in every redirect and in DNS answers. The one exception is a feed you allow yourself: when an address is on your local network (a NAS or a home server), Tidings asks before it connects and before it subscribes, also when a website names such an address as its feed. That consent covers only the feed's own host and port: the feed itself and its icon. Links in articles, reader tabs, images, podcasts and videos never reach the local network, also not from a local feed; open those in your browser. Where a name resolves plays no part: a public-looking name that points to a private address is refused unless you allowed it. With a proxy, the proxy does the DNS lookups, so only literal private addresses and local names can be refused: Tidings cannot see where the proxy sends a name. This rule covers the requests Tidings makes. It does not cover mpv, IINA and yt-dlp after you click "Watch" or "Listen": they are separate programs that make their own connections (see Videos). |
| Network hardening | rustls TLS, no cookie store, no `Referer` header, size limits on every download, generic user agent, optional HTTPS-only mode, optional HTTP/SOCKS5 proxy (works with Tor: `socks5h://127.0.0.1:9050`). SOCKS proxies always do the DNS lookups themselves: `socks5://` is saved as `socks5h://`. If the saved settings or the proxy in them cannot be used, Tidings starts offline instead of connecting directly. |
| Local data | Feeds, articles, settings and briefs are in one SQLite database, file mode `0600` in a `0700` folder. `secure_delete` is on: when an article is deleted or archived, its text, title, link and author are overwritten, and only its ID stays for 60 days so it does not come back. After you delete articles, a feed, a folder or a brief, the database's write-ahead log is emptied too, so the deleted text is not left there. Outside the database: open tabs, playback positions and view settings are in the web view's local storage, shown images are in its cache, and the window size is in a small file, all in the system's folders for this app. An OPML export is written with mode `0600`; it lists every feed address, including a password if an address has one. Backups (such as Time Machine) can keep older copies of all of these. |

Data folder:
- macOS: `~/Library/Application Support/org.tidings.reader/`
- Linux, FreeBSD and OpenBSD: `~/.local/share/org.tidings.reader/`

### Limits on Linux and the BSDs

- **No web view sandbox.** macOS runs the web view's content in a sandboxed process.
  WebKitGTK can do the same (with bubblewrap), but Tidings does not turn that on yet.
  If a bug in WebKitGTK were reached through an article, the code would run with
  your user's rights. The HTML cleaner, the Content Security Policy and the image
  re-encoding are there so that such a bug is hard to reach.
- **Audio and video are decoded by GStreamer.** Images are re-encoded in Rust; media
  files are not. They are decoded by your system's GStreamer plugins when you press
  play. The security check in Settings covers Tidings, yt-dlp and WebKitGTK, not
  GStreamer: keep it up to date with your distribution's updates.
- **The AppImage carries its own WebKitGTK.** See "Installing on Linux".

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

**Any other distribution:** the AppImage carries its own libraries:

```bash
chmod +x Tidings_0.1.0_amd64.AppImage
./Tidings_0.1.0_amd64.AppImage
```

Use the AppImage only when none of the packages above fits. The `.deb`, the `.rpm`
and the tarball use your system's WebKitGTK, GTK and GStreamer, which get security
fixes with your distribution's updates. The AppImage holds its own copy of WebKitGTK,
GTK, GLib and libsoup, from the day that Tidings release was built. That copy gets
fixes only with a new Tidings release, and it can be older than your system's.
Settings → Security shows the WebKitGTK version in use.

## Installing on FreeBSD and OpenBSD

There are no prebuilt BSD packages yet. Build the portable tarball on the machine
itself (see "Building on FreeBSD and OpenBSD" below), then install it into `~/.local`:

```sh
tar xzf release/freebsd/tidings-0.9.3-freebsd-x86_64.tar.gz
./tidings-0.9.3-freebsd-x86_64/install.sh
```

For podcasts and videos inside Tidings, install the GStreamer plugins listed in the next
section; without them episodes fail with "This episode could not be played", and videos
offer to open in your video player instead.

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
proxy rules, image re-encoding, reader-mode extraction, every database query and an OPML round trip.

To check video playback on your machine (it opens mpv or IINA and plays a 19-second
YouTube clip):

```bash
TIDINGS_LIVE_PLAYER=1 cargo test --manifest-path src-tauri/Cargo.toml live_player -- --ignored
```

To check Listen (yt-dlp finds the audio of the same clip, and the media proxy reads it):

```bash
cargo test --manifest-path src-tauri/Cargo.toml live_listen -- --ignored
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
| `L` | Listen to the video's sound in the podcast player |
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
  images.rs              Re-encodes every image before the web view sees it
  updates.rs             Security check: are Tidings, yt-dlp and WebKitGTK up to date?
  brief.rs               AI briefs written by a local model (Ollama, LM Studio, llama.cpp)
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
