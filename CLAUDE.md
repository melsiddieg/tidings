# Tidings: notes for working on this repo

Tidings is a private RSS/Atom/JSON Feed reader. Tauri 2 (Rust) backend, Svelte 5 UI,
SQLite storage. Targets: macOS, Linux, FreeBSD and OpenBSD.
See docs/README.md for features, the security design and user-facing build steps.

## Commands

```bash
npm ci                                   # JS dependencies
npm run check                            # Svelte/TypeScript type check (must be 0 errors, 0 warnings)
cargo test --manifest-path src-tauri/Cargo.toml                 # offline tests
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored    # + live network test
TIDINGS_LIVE_PLAYER=1 cargo test --manifest-path src-tauri/Cargo.toml live_player -- --ignored  # plays a clip in mpv/IINA
cargo test --manifest-path src-tauri/Cargo.toml live_listen -- --ignored   # yt-dlp finds a clip's audio ("Listen")
npm run tauri dev                        # run the app (Vite on port 1420)
npm run tauri build                      # release packages for the host OS
npm run dev                              # UI only in a browser, with the fake backend in src/lib/mock.ts
scripts/build-linux.sh                   # Linux x86_64 .deb/.rpm/AppImage/tarball via Podman or Docker
scripts/build-bsd.sh                     # on FreeBSD/OpenBSD: native build + portable tarball
```

The `tauri` crate and the `@tauri-apps/api` npm package must share major.minor
(currently 2.11), or `tauri build` refuses to run.

## Layout

- `src-tauri/src/` — `commands.rs` (IPC surface), `db.rs` (all SQL), `feeds.rs` (fetch, parse,
  store, favicons), `sanitize.rs` (ammonia cleaning, tracker removal, image proxy rewrite),
  `protocol.rs` (`tidings://` image proxy and icons), `images.rs` (re-encodes every image
  in Rust before the web view sees it; draws SVG in a helper process), `brief.rs` (AI briefs by a local model), `updates.rs` (security check: versions of Tidings,
  yt-dlp and WebKitGTK, and what to run to update them), `net.rs` (HTTP clients, private-address
  checks), `reader.rs` (reader mode), `opml.rs`, `discover.rs`, `scheduler.rs`, `menu.rs` (macOS menu),
  `feed_types.rs` (podcast/video/code/... detection; `db::detect_feed_type` stores it after each refresh),
  `video.rs` (finds and starts mpv or IINA for video links), `peertube.rs` (PeerTube video
  files through the instance API, and Sepia Search).
- `src-tauri/src/tests.rs` runs every SQL query against a real database. Add a case there when
  you add or change a query.
- `src/lib/store.svelte.ts` holds UI state and actions; components in `src/components/`.
  `src/lib/filters.ts` has the feed type list and the date range presets (`rangeBounds`).
- Article filters all go through `db::where_clause` (scope, read state, search, `DateRange`,
  feed type inside the scope), so the list, "mark all as read" and ID lists always agree.
  Extend it there, not per query.

## Rules that keep the app private and safe

- All network traffic goes through `net.rs` clients. The web view must never load remote
  resources: images use `tidings://localhost/img/<base64url>`; the CSP in `tauri.conf.json`
  blocks everything else. Do not loosen the CSP or add remote URLs to it. `style-src` is
  `'self'` only; inline `style` attributes are allowed through `style-src-attr`, but a
  `<style>` element added at run time is blocked, so do not add libraries that inject one.
- One deliberate exception: `media-src` allows `http://127.0.0.1:*`. WebKitGTK plays media
  through GStreamer, which cannot read `tidings://` URLs, so on Linux podcast audio comes from
  `media_server.rs`: a server bound to 127.0.0.1 on a random port that only answers URLs with
  a random secret token, and that serves through the same `protocol::proxy_media` code.
  macOS keeps using `tidings://`. Do not extend this to other directives or hosts.
- Start feed, page and favicon requests with `Net::get(url, trusted)`, never with a client
  directly. `trusted` is an address on the local network that the user allowed: a feed whose
  `local` column is 1, or the address they confirmed in the Add Feed dialog
  (`net::LOCAL_CONSENT`). It covers only that host and port (`net::same_site`). Pass `None`
  for everything else: reader tabs, article links, videos, and feeds without `local`.
  DNS must never decide trust: a public-looking name that resolves to a private address is
  refused unless the user allowed it. `local` is set only by the user's consent (`add_feed`
  with `allow_local`, or the toggle in the feed's properties), never from a lookup.
- Start image, audio and video requests with `Net::public(url)`. It refuses private and
  local addresses where the request starts, after any rewriting (`media::strip_trackers`,
  cached redirect targets, addresses from yt-dlp or PeerTube). Do not check only in the
  caller: literal IP addresses never reach the DNS resolver that guards names.
- Do not add DNS lookups of your own: when a proxy is set (Tor), every lookup must go
  through it, and `Net::is_private_host` skips DNS then. `socks5://` is stored as
  `socks5h://` for the same reason.
- Settings that cannot be read or validated must never fall back to all defaults
  (`settings::load`): that would drop the proxy. Reset the invalid field, or start offline.
- Every image the web view shows must come out of `images::clean`: `protocol.rs` serves only
  re-encoded bytes, never the downloaded file. Keep the `image` crate to pure-Rust decoders
  (no `avif-native` or other C libraries). SVG is drawn to PNG with resvg; keep its image
  resolver empty (`images::svg`), or an SVG could read files on this computer.
- SVG is drawn only in the helper process: the app starts itself with `--svg-worker`
  (`images::worker_main`, called first thing in `main`) and talks to it through
  `images::draw`. Never call `images::svg` in the app process: usvg and resvg recurse
  on nested elements and on chains of clip paths, masks and `<tspan>`, and a small
  crafted SVG overflows the stack, which ends the process and cannot be caught. Keep
  the helper's time limit and memory limit (`images::Capped`, the global allocator),
  and keep `worker_main` free of anything but reading stdin and writing stdout.
- The media proxy gives the player single files only. Keep `media::is_playlist` on the
  first block of every file, and `is_playlist_type` on every answer: a player that
  gets an HLS or DASH playlist fetches the addresses in it by itself, past `net.rs`,
  the proxy and the private-address checks. Do not add HLS or DASH support by letting
  the player read a playlist.
- Feed and page HTML must pass through `sanitize::clean` before storage and `sanitize::render`
  before display.
- Videos play in an external player (`video.rs`), never in the web view: no iframes, no site
  players. The player is a separate program with its own connections, like "Open in
  browser". Keep `video::args` refusing to start when the player cannot use the proxy
  setting (mpv ignores anything but `http://` proxies), so videos never bypass Tor. Start
  the player with an argument list, never through a shell.
- "Listen" (`video::audio_stream`) runs yt-dlp only to find a video's audio stream; the audio
  itself streams through `mediaUrl(...)` like a podcast. Keep `--ignore-config` (the user's
  yt-dlp config may hold `--cookies-from-browser`), keep passing the proxy, and keep the
  stream to a single http(s) file. Start yt-dlp with an argument list ending in `--` and the URL.
- Videos inside Tidings (PeerTube, attached video files) use a `<video>` element whose
  source is `mediaUrl(...)`, the same media proxy as podcasts. Never point it at a remote URL.
- Sepia Search (`peertube::search`) sends the article title to sepiasearch.org. Call it only
  when the user clicks "Find on PeerTube", never automatically.
- Keep UI permissions in `src-tauri/capabilities/default.json` minimal. File dialogs and file
  access happen in Rust commands, not in the UI.
- AI briefs (`brief.rs`) use only a model server on this computer or the local network.
  Requests to it go through `Net::local_model`, which checks the address
  (`net::check_local_model`: no public hosts; with a proxy, IPs and `localhost` only) and
  never uses the proxy or follows redirects. Do not add cloud AI providers. Send the model
  only titles, feed names, dates and excerpts; keep its answer JSON that cites article
  numbers, drop citations of articles that were not sent, and show its text as plain text.
- If the saved proxy cannot be used at startup, `AppState::init` uses `Net::offline()`.
  Never fall back to a direct connection: it would bypass Tor without the user knowing.
- Do not start other programs at startup. The security check runs `yt-dlp --version` only
  when Settings → Security is open; at startup it uses the version it saw then. (The SVG
  helper is the app's own binary, started when the first SVG is shown.)
- No telemetry, analytics, crash reporting or automatic update checks. The security check
  (`updates.rs`) reads versions on this computer at startup; it contacts GitHub and
  webkitgtk.org only when the user clicks "Check now". Keep it that way, and keep its
  requests on `Net::get`.
- Never set `panic = "abort"` in `Cargo.toml`. wry relies on unwinding to catch the
  Objective-C exception WebKit throws when a cancelled `tidings://` request is answered;
  without it the macOS app aborts (see the comment in `[profile.release]`).

## Platform-specific code

`build.rs` sets `cfg(webkitgtk)` on Linux, FreeBSD, OpenBSD, NetBSD and DragonFly: every
platform where Tauri renders with WebKitGTK and plays media through GStreamer. Use it for
WebKitGTK workarounds instead of listing operating systems.

| Place | What | Notes |
|---|---|---|
| `main.rs` `webkitgtk_workarounds` | Sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` with NVIDIA's driver | `cfg(webkitgtk)`. Probes `/proc/driver/nvidia/version` (Linux) and `/dev/nvidiactl` (Linux, FreeBSD). OpenBSD has no NVIDIA driver. |
| `main.rs` (first lines), `images.rs` `worker_command` | Starts the app again as the SVG helper | All platforms. Uses `std::env::current_exe`; on OpenBSD that is `argv[0]`, so a bare name is looked up in `PATH`. If the helper cannot start, SVG images are not shown. |
| `main.rs` `start_media_server`, `media_server.rs` | Loopback audio server (GStreamer cannot read custom schemes) | `cfg(webkitgtk)`. Open-ended requests get one response to the end of the file, filled from 2 MB proxy slices: WebKitGTK takes a short 206 as the whole file. |
| `main.rs` window builder | Overlay title bar and traffic-light position | macOS-only cfg. |
| `menu.rs` | Native menu bar | macOS only. Other platforms rely on keyboard handling in `src/App.svelte` (`isMac` check). |
| `src/App.svelte` / `src/lib/store.svelte.ts` | `data-platform` comes from `std::env::consts::OS` (`app_info`) | `linux`, `freebsd`, `openbsd`; CSS only special-cases `macos`. |
| `commands::set_badge` | Dock / launcher badge | Errors are ignored; a no-op where unsupported. |
| `db::open`, `state.rs` | `#[cfg(unix)]` file modes 0600/0700 | Any Unix. |
| Data folder | Tauri `app_data_dir()` | `~/.local/share/org.tidings.reader` on Linux and the BSDs. |
| TLS | `reqwest` with `rustls-tls-native-roots` | Needs the system CA bundle (FreeBSD: base `certctl`; OpenBSD: `/etc/ssl/cert.pem`). |
| `vite.config.ts` `cssMinify` | lightningcss has no OpenBSD binary | CSS is left unminified on OpenBSD. |
| Packaging | Tauri bundles deb/rpm/AppImage/dmg only | BSDs: `scripts/build-bsd.sh` makes a tarball with the POSIX `scripts/linux/install.sh`. |

### BSD builds

- `scripts/build-bsd.sh` runs natively on FreeBSD or OpenBSD (no Tauri CLI there). It calls
  `cargo build --release --features tauri/custom-protocol`, which is what `tauri build` does.
- OpenBSD: the script raises `ulimit -d` (the `staff` class allows 1.5 GB; the link needs
  more, and so does Rolldown's WebAssembly binding) and installs
  `@rolldown/binding-wasm32-wasi` with `--force`, since Rolldown has no OpenBSD binary.
- GhostBSD splits the base system into packages; linking needs `GhostBSD-libexecinfo-dev`
  and `GhostBSD-kerberos-dev` (for `libsoup-3.0.pc`).
- Keep `scripts/linux/install.sh` plain POSIX sh: no `install -D`, `sed -i` or brace expansion.
- Run `cargo test` and the `live_network` test on each target.
