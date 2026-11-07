# Tidings: notes for working on this repo

Tidings is a private RSS/Atom/JSON Feed reader. Tauri 2 (Rust) backend, Svelte 5 UI,
SQLite storage. Targets: macOS and Linux today; FreeBSD and OpenBSD are planned.
See README.md for features, the security design and user-facing build steps.

## Commands

```bash
npm ci                                   # JS dependencies
npm run check                            # Svelte/TypeScript type check (must be 0 errors, 0 warnings)
cargo test --manifest-path src-tauri/Cargo.toml                 # offline tests
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored    # + live network test
npm run tauri dev                        # run the app (Vite on port 1420)
npm run tauri build                      # release packages for the host OS
npm run dev                              # UI only in a browser, with the fake backend in src/lib/mock.ts
scripts/build-linux.sh                   # Linux x86_64 .deb/.rpm/AppImage/tarball via Podman or Docker
```

The `tauri` crate and the `@tauri-apps/api` npm package must share major.minor
(currently 2.11), or `tauri build` refuses to run.

## Layout

- `src-tauri/src/` — `commands.rs` (IPC surface), `db.rs` (all SQL), `feeds.rs` (fetch, parse,
  store, favicons), `sanitize.rs` (ammonia cleaning, tracker removal, image proxy rewrite),
  `protocol.rs` (`tidings://` image proxy and icons), `net.rs` (HTTP clients, private-address
  checks), `reader.rs` (reader mode), `opml.rs`, `discover.rs`, `scheduler.rs`, `menu.rs` (macOS menu).
- `src-tauri/src/tests.rs` runs every SQL query against a real database. Add a case there when
  you add or change a query.
- `src/lib/store.svelte.ts` holds UI state and actions; components in `src/components/`.

## Rules that keep the app private and safe

- All network traffic goes through `net.rs` clients. The web view must never load remote
  resources: images use `tidings://localhost/img/<base64url>`; the CSP in `tauri.conf.json`
  blocks everything else. Do not loosen the CSP or add remote URLs to it.
- One deliberate exception: `media-src` allows `http://127.0.0.1:*`. WebKitGTK plays media
  through GStreamer, which cannot read `tidings://` URLs, so on Linux podcast audio comes from
  `media_server.rs`: a server bound to 127.0.0.1 on a random port that only answers URLs with
  a random secret token, and that serves through the same `protocol::proxy_media` code.
  macOS keeps using `tidings://`. Do not extend this to other directives or hosts.
- Feed and page HTML must pass through `sanitize::clean` before storage and `sanitize::render`
  before display.
- Keep UI permissions in `src-tauri/capabilities/default.json` minimal. File dialogs and file
  access happen in Rust commands, not in the UI.
- No telemetry, analytics, crash reporting or update checks.

## Platform-specific code (start here when porting)

| Place | What | BSD note |
|---|---|---|
| `src-tauri/src/main.rs` `linux_webkit_workarounds` | Sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` when `/proc/driver/nvidia/version` exists | Linux-only cfg. On the BSDs the NVIDIA check needs a different probe, or extend the cfg if WebKitGTK shows a blank window. |
| `src-tauri/src/main.rs` window builder | Overlay title bar and traffic-light position | macOS-only cfg. |
| `src-tauri/src/menu.rs` | Native menu bar | macOS only. Other platforms rely on keyboard handling in `src/App.svelte` (`isMac` check). |
| `src/App.svelte` / `src/lib/store.svelte.ts` | `data-platform` comes from `std::env::consts::OS` (`app_info`) | Will be `freebsd` / `openbsd`; CSS only special-cases `macos`. |
| `main.rs` `start_media_server`, `media_server.rs` | Loopback audio server (GStreamer cannot read custom schemes) | Linux-only cfg. The BSDs also use WebKitGTK + GStreamer, so extend the cfg to them; `random_token` reads `/dev/urandom`, which exists there too. |
| `commands::set_badge` | Dock / launcher badge | Errors are ignored; expect a no-op on the BSDs. |
| `db::open`, `state.rs` | `#[cfg(unix)]` file modes 0600/0700 | Works on any Unix. |
| Data folder | Tauri `app_data_dir()` (XDG on Linux) | Check where it resolves on each BSD. |
| TLS | `reqwest` with `rustls-tls-native-roots` | Needs the system CA bundle (`/etc/ssl/cert.pem`; FreeBSD gets it from `ca_root_nss`). |
| Packaging | Tauri bundles deb/rpm/AppImage only | BSDs need a port/pkg recipe; `scripts/linux/install.sh` and `tidings.desktop` show what to install. |

The Tauri stack (`tauri`, `tao`, `wry`, `muda`, `tauri-runtime-wry`) already has
`target_os = "freebsd"` / `"openbsd"` branches that share the GTK/WebKitGTK backend with
Linux, so the port should mostly be about system packages (webkit2gtk 4.1, gtk3, libxdo,
pkg-config) and packaging. Run `cargo test` and the `live_network` test on each target.
