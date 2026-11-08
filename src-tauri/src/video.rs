//! Plays videos in an external player: mpv, or IINA (a macOS app built on mpv).
//! Both use yt-dlp to stream YouTube, Vimeo, PeerTube, Odysee and similar sites.
//! The site's own web player never runs, so no site JavaScript, cookies or account
//! are involved. The web view still never loads anything remote.
//!
//! The player makes its own connections, outside net.rs, like a browser started with
//! "Open in browser". When a proxy is set, Tidings passes it to the player, and it
//! refuses to start a player that cannot use that proxy, so videos never bypass it.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use url::Url;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mpv,
    Iina,
    /// Any other program the user chose. It gets the URL as its only argument.
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    /// Shown in the UI: "mpv", "IINA" or the program's name.
    pub name: String,
    pub path: PathBuf,
    #[serde(skip)]
    pub kind: Kind,
}

/// How long `play` waits for an early failure. A player still running after this
/// is taken to be playing.
pub const STARTUP_WAIT: Duration = Duration::from_secs(20);

const NO_PLAYER: &str = "No video player found. Install mpv or IINA, or choose a player in \
    Settings → Reading. mpv also needs yt-dlp for YouTube and similar sites.";

/// Finds the player to use. `setting` is the user's choice: empty for automatic, a
/// program name looked up like a shell would (`mpv`), or a path (`~/bin/mpv`,
/// `/Applications/VLC.app`).
pub fn find(setting: &str) -> Option<Player> {
    let dirs = search_dirs();
    let setting = setting.trim();
    if setting.is_empty() {
        return auto(&dirs);
    }
    let path = if setting.contains('/') {
        Some(app_executable(expand_home(setting)))
    } else {
        find_in(&dirs, setting)
    };
    path.filter(|p| is_executable(p)).map(player_at)
}

pub fn not_found() -> Error {
    Error::msg(NO_PLAYER)
}

fn auto(dirs: &[PathBuf]) -> Option<Player> {
    let mut candidates = Vec::new();
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications/IINA.app/Contents/MacOS/iina-cli"));
        if let Some(home) = home() {
            candidates.push(home.join("Applications/IINA.app/Contents/MacOS/iina-cli"));
        }
    }
    candidates.extend(dirs.iter().map(|d| d.join("mpv")));
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications/mpv.app/Contents/MacOS/mpv"));
    }
    candidates.into_iter().find(|p| is_executable(p)).map(player_at)
}

fn player_at(path: PathBuf) -> Player {
    let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();
    let (kind, name) = match file.to_ascii_lowercase().as_str() {
        "mpv" => (Kind::Mpv, "mpv".to_string()),
        "iina-cli" | "iina" => (Kind::Iina, "IINA".to_string()),
        _ => (Kind::Other, bundle_name(&path).unwrap_or_else(|| file.clone())),
    };
    // The main IINA binary does not take options; iina-cli next to it does.
    let path = match (kind, path.parent()) {
        (Kind::Iina, Some(dir)) if file != "iina-cli" && dir.join("iina-cli").is_file() => dir.join("iina-cli"),
        _ => path,
    };
    Player { name, path, kind }
}

/// "VLC" for /Applications/VLC.app/Contents/MacOS/VLC.
fn bundle_name(path: &Path) -> Option<String> {
    path.ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .and_then(|a| a.file_stem())
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

/// A macOS .app folder stands for the program inside it. IINA's command-line
/// helper is iina-cli; other apps use their own name (VLC.app → Contents/MacOS/VLC).
fn app_executable(path: PathBuf) -> PathBuf {
    if !(path.extension().is_some_and(|e| e == "app") && path.is_dir()) {
        return path;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let program = if stem.eq_ignore_ascii_case("iina") { "iina-cli".to_string() } else { stem };
    path.join("Contents/MacOS").join(program)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute())
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// Folders searched for programs: PATH, then the usual install folders. Apps started
/// from the macOS Finder get only /usr/bin:/bin:/usr/sbin:/sbin, which misses Homebrew.
/// Relative PATH entries are skipped, so the current folder is never searched.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = home() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin", "/bin", "/usr/pkg/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(dir));
    }
    let mut seen = HashSet::new();
    dirs.retain(|d| d.is_absolute() && seen.insert(d.clone()));
    dirs
}

fn find_in(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(name)).find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The player's arguments for one video. `proxy` is the user's proxy setting.
pub fn args(player: &Player, url: &Url, proxy: &str) -> Result<Vec<String>> {
    let proxy = proxy.trim();
    if !proxy.is_empty() {
        // mpv only supports http:// proxies (it ignores others without a word), and
        // there is no general way to give a proxy to an unknown program.
        let http = Url::parse(proxy).is_ok_and(|p| p.scheme() == "http");
        if !http || player.kind == Kind::Other {
            return Err(Error::msg(format!(
                "{} cannot use your proxy, so the video would bypass it. The player can only \
                 use an http:// proxy, and only when it is mpv or IINA. Remove the proxy setting \
                 or open the video in your browser.",
                player.name
            )));
        }
    }
    let mut args = Vec::new();
    let mpv_option = |name: &str, value: &str| match player.kind {
        Kind::Iina => format!("--mpv-{name}={value}"),
        _ => format!("--{name}={value}"),
    };
    match player.kind {
        Kind::Mpv | Kind::Iina => {
            if player.kind == Kind::Iina {
                // Tidings gives the player no input. Without this, iina-cli takes that
                // for a piped file, starts IINA with --stdin and ignores the URL.
                args.push("--no-stdin".to_string());
            }
            if player.kind == Kind::Mpv {
                // Show the window at once, while yt-dlp is still looking up the video.
                args.push("--force-window=immediate".to_string());
                // Only errors on stderr, so `play` can report them.
                args.push("--msg-level=all=error".to_string());
            }
            if !proxy.is_empty() {
                args.push(mpv_option("http-proxy", proxy));
                args.push(mpv_option("ytdl-raw-options-append", &format!("proxy={proxy}")));
            }
        }
        Kind::Other => {}
    }
    // The URL always starts with http:// or https://, so no player can read it as an option.
    args.push(url.to_string());
    Ok(args)
}

/// Starts the player and waits up to `wait` for an early failure, such as a missing
/// yt-dlp or an unavailable video. A player still running after that is playing.
pub async fn play(player: &Player, url: &Url, proxy: &str, wait: Duration) -> Result<()> {
    let args = args(player, url, proxy)?;
    // The player writes errors to a private temporary file, not to a pipe. A pipe
    // closes when Tidings quits, and the player would then be killed (SIGPIPE) the
    // next time it writes an error, in the middle of a video.
    let (log_path, log) = error_log()?;
    let mut command = Command::new(&player.path);
    command.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log.try_clone()?);
    // Let mpv find yt-dlp, and yt-dlp find its JavaScript runtime, even when Tidings
    // was started from the Finder with a short PATH.
    if let Ok(path) = std::env::join_paths(search_dirs()) {
        command.env("PATH", path);
    }
    let result = match command.spawn() {
        Err(e) => Err(Error::msg(format!("Could not start {}: {e}", player.name))),
        Ok(mut child) => {
            let (done_tx, done_rx) = tokio::sync::oneshot::channel();
            // This thread also reaps the player when it exits after `wait`.
            std::thread::spawn(move || {
                let _ = done_tx.send(child.wait());
            });
            match tokio::time::timeout(wait, done_rx).await {
                Ok(Ok(Ok(status))) => check_exit(player, status, &last_lines(log)),
                // Still running (playing), or the wait itself failed: nothing to report.
                _ => Ok(()),
            }
        }
    };
    // A player still running keeps writing to its open handle; only the name goes.
    let _ = std::fs::remove_file(&log_path);
    result
}

/// Creates an empty file only this user can read, under a name no one else can claim.
fn error_log() -> Result<(PathBuf, File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = format!("tidings-player-{}-{}.log", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let path = std::env::temp_dir().join(name);
    let mut options = OpenOptions::new();
    // create_new fails if the name exists, so a planted file or link is never used.
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    Ok((path, file))
}

/// The last 20 non-empty lines of the error log, each at most 400 characters.
fn last_lines(mut file: File) -> Vec<String> {
    const MAX: u64 = 16 * 1024;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(len.saturating_sub(MAX))).is_ok() {
        let _ = file.take(MAX).read_to_end(&mut buf);
    }
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.trim().chars().take(400).collect::<String>())
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(20)..].to_vec()
}

fn check_exit(player: &Player, status: ExitStatus, lines: &[String]) -> Result<()> {
    // mpv exits with 1, 2 or 3 when it could not play; 0 and 4 mean the user quit.
    let failed = match player.kind {
        Kind::Mpv => matches!(status.code(), Some(1..=3)),
        _ => status.code().is_some_and(|c| c != 0),
    };
    if !failed {
        return Ok(());
    }
    let reason = lines
        .iter()
        .rev()
        .find_map(|l| l.split_once("ERROR: ").map(|(_, rest)| rest.to_string()))
        .or_else(|| lines.last().cloned());
    let mut message = match reason {
        Some(reason) => format!("{} could not play this video: {reason}", player.name),
        None => format!("{} could not play this video (exit code {}).", player.name, status.code().unwrap_or(-1)),
    };
    if player.kind == Kind::Mpv && find_in(&search_dirs(), "yt-dlp").is_none() {
        message.push_str(" mpv needs yt-dlp for YouTube and similar sites, and it was not found.");
    }
    Err(Error::msg(message))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn player(kind: Kind) -> Player {
        let name = match kind {
            Kind::Mpv => "mpv",
            Kind::Iina => "IINA",
            Kind::Other => "VLC",
        };
        Player { name: name.into(), path: PathBuf::from("/x"), kind }
    }

    fn video() -> Url {
        Url::parse("https://www.youtube.com/watch?v=abc").unwrap()
    }

    #[test]
    fn arguments() {
        let url = video();
        assert_eq!(
            args(&player(Kind::Mpv), &url, "").unwrap(),
            ["--force-window=immediate", "--msg-level=all=error", "https://www.youtube.com/watch?v=abc"]
        );
        assert_eq!(args(&player(Kind::Iina), &url, "").unwrap(), ["--no-stdin", "https://www.youtube.com/watch?v=abc"]);
        assert_eq!(args(&player(Kind::Other), &url, " ").unwrap(), ["https://www.youtube.com/watch?v=abc"]);

        let mpv = args(&player(Kind::Mpv), &url, "http://proxy:3128").unwrap();
        assert!(mpv.contains(&"--http-proxy=http://proxy:3128".to_string()));
        assert!(mpv.contains(&"--ytdl-raw-options-append=proxy=http://proxy:3128".to_string()));
        let iina = args(&player(Kind::Iina), &url, "http://proxy:3128").unwrap();
        assert!(iina.contains(&"--mpv-http-proxy=http://proxy:3128".to_string()));
        assert_eq!(iina.last().unwrap(), url.as_str());

        // A proxy the player cannot use must stop playback, not be skipped.
        assert!(args(&player(Kind::Mpv), &url, "socks5h://127.0.0.1:9050").is_err());
        assert!(args(&player(Kind::Mpv), &url, "https://proxy:3128").is_err());
        assert!(args(&player(Kind::Other), &url, "http://proxy:3128").is_err());
    }

    #[test]
    fn player_names() {
        let p = player_at(PathBuf::from("/opt/homebrew/bin/mpv"));
        assert_eq!((p.kind, p.name.as_str()), (Kind::Mpv, "mpv"));
        let p = player_at(PathBuf::from("/Applications/IINA.app/Contents/MacOS/iina-cli"));
        assert_eq!((p.kind, p.name.as_str()), (Kind::Iina, "IINA"));
        let p = player_at(PathBuf::from("/Applications/VLC.app/Contents/MacOS/VLC"));
        assert_eq!((p.kind, p.name.as_str()), (Kind::Other, "VLC"));
        let p = player_at(PathBuf::from("/usr/bin/celluloid"));
        assert_eq!((p.kind, p.name.as_str()), (Kind::Other, "celluloid"));
        assert!(search_dirs().iter().all(|d| d.is_absolute()));
    }

    /// Plays a 19-second YouTube video in the player found on this machine. It opens a
    /// window and plays sound, so it only runs when asked for:
    /// `TIDINGS_LIVE_PLAYER=1 cargo test live_player -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live_player() {
        if std::env::var_os("TIDINGS_LIVE_PLAYER").is_none() {
            eprintln!("skipped: set TIDINGS_LIVE_PLAYER=1 to open a real player");
            return;
        }
        let p = find("").expect("no player found: install mpv or IINA");
        eprintln!("playing in {} ({})", p.name, p.path.display());
        let url = Url::parse("https://www.youtube.com/watch?v=jNQXAC9IVRw").unwrap();
        play(&p, &url, "", Duration::from_secs(15)).await.unwrap();
    }

    /// Writes a shell script named `name` that stands in for a player.
    fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn launching() {
        let dir = std::env::temp_dir().join(format!("tidings-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("args.txt");
        let url = video();

        // A custom player found by path gets the URL, and success is reported.
        let path = fake(&dir, "myplayer", &format!("printf '%s\\n' \"$@\" > '{}'", out.display()));
        let p = find(path.to_str().unwrap()).expect("custom player");
        assert_eq!(p.kind, Kind::Other);
        play(&p, &url, "", Duration::from_secs(5)).await.unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), url.as_str());

        // An early failure comes back with the player's error line.
        let path = fake(&dir, "mpv", "echo '[ytdl_hook] ERROR: [youtube] abc: Video unavailable' >&2\nexit 2");
        let p = find(path.to_str().unwrap()).unwrap();
        assert_eq!(p.kind, Kind::Mpv);
        let err = play(&p, &url, "", Duration::from_secs(5)).await.unwrap_err().to_string();
        assert!(err.starts_with("mpv could not play this video: [youtube] abc: Video unavailable"), "{err}");

        // mpv quit by the user (exit 4) is not an error.
        let path = fake(&dir, "mpv", "exit 4");
        play(&player_at(path), &url, "", Duration::from_secs(5)).await.unwrap();

        // A player still running after the wait counts as playing.
        let path = fake(&dir, "slow", "sleep 3");
        play(&player_at(path), &url, "", Duration::from_millis(300)).await.unwrap();

        // The error logs are gone, also for the player that is still running.
        let prefix = format!("tidings-player-{}-", std::process::id());
        let left = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .count();
        assert_eq!(left, 0);

        // Missing or non-executable programs are not offered.
        assert!(find(dir.join("args.txt").to_str().unwrap()).is_none());
        assert!(find("/nonexistent/mpv").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
