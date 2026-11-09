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
  in Rust before the web view sees it), `net.rs` (HTTP clients, private-address
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
  blocks everything else. Do not loosen the CSP or add remote URLs to it.
- One deliberate exception: `media-src` allows `http://127.0.0.1:*`. WebKitGTK plays media
  through GStreamer, which cannot read `tidings://` URLs, so on Linux podcast audio comes from
  `media_server.rs`: a server bound to 127.0.0.1 on a random port that only answers URLs with
  a random secret token, and that serves through the same `protocol::proxy_media` code.
  macOS keeps using `tidings://`. Do not extend this to other directives or hosts.
- Start feed, page and favicon requests with `Net::get(url, trusted)`, never with a client
  directly. `trusted` is the address the user chose (the feed the request belongs to, or the
  address they typed); pass `None` for anything else. Private addresses are allowed only on
  the trusted host, and only when that host is itself private. Images and audio use
  `net.image`, behind `protocol::decode_target`. Do not add DNS lookups of your own: when a
  proxy is set (Tor), every lookup must go through it, and `Net::is_private_host` skips DNS then.
- Every image the web view shows must come out of `images::clean`: `protocol.rs` serves only
  re-encoded bytes, never the downloaded file. Keep the `image` crate to pure-Rust decoders
  (no `avif-native` or other C libraries).
- Feed and page HTML must pass through `sanitize::clean` before storage and `sanitize::render`
  before display.
- Videos play in an external player (`video.rs`), never in the web view: no iframes, no site
  players. The player is a separate program with its own connections, like "Open in
  browser". Keep `video::args` refusing to start when the player cannot use the proxy
  setting (mpv ignores anything but `http://` proxies), so videos never bypass Tor. Start
  the player with an argument list, never through a shell.
- Videos inside Tidings (PeerTube, attached video files) use a `<video>` element whose
  source is `mediaUrl(...)`, the same media proxy as podcasts. Never point it at a remote URL.
- Sepia Search (`peertube::search`) sends the article title to sepiasearch.org. Call it only
  when the user clicks "Find on PeerTube", never automatically.
- Keep UI permissions in `src-tauri/capabilities/default.json` minimal. File dialogs and file
  access happen in Rust commands, not in the UI.
- No telemetry, analytics, crash reporting or update checks.
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
