# Security

## Reporting a problem

Please report security problems privately, not in a public issue:
open the repository's **Security** tab on GitHub and choose **Report a vulnerability**.

Include the Tidings version, your operating system, and the steps or the feed that
shows the problem. You should get an answer within a week.

## Supported versions

Only the newest release gets fixes. Settings → Security in the app shows whether
your copy, yt-dlp and WebKitGTK are up to date.

## What to expect from the downloads

- Releases are built by GitHub Actions from the tagged commit
  (`.github/workflows/build.yml`). Actions are pinned to commits, and a release is
  not published when `cargo audit` or `npm audit` finds a known vulnerability.
- `SHA256SUMS` in each release shows that a download is complete and unchanged
  since the build. It is made by the same workflow, so it does not prove who built
  it. The packages are not signed, and the macOS app is not notarized.

- On Linux, prefer the `.deb`, the `.rpm` or the tarball: they use your system's
  WebKitGTK, which your distribution keeps up to date. The AppImage carries its own
  copy, which changes only with a new Tidings release.

## Known limits

- mpv, IINA and yt-dlp are separate programs. After you click "Watch" or "Listen"
  they make their own connections, which Tidings' local network rule does not cover.
- On Linux and the BSDs the web view's content process is not sandboxed, and audio
  and video are decoded by the system's GStreamer.

## Design

The security design is described in [docs/README.md](docs/README.md#privacy-and-security).
