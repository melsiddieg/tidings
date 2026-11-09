//! Plays videos in an external player: mpv, or IINA (a macOS app built on mpv).
//! Both use yt-dlp to stream YouTube, Vimeo, PeerTube, Odysee and similar sites.
//! The site's own web player never runs, so no site JavaScript, cookies or account
//! are involved. The web view still never loads anything remote.
//!
//! The player makes its own connections, outside net.rs, like a browser started with
//! "Open in browser". When a proxy is set, Tidings passes it to the player, and it
//! refuses to start a player that cannot use that proxy, so videos never bypass it.
//!
//! Most failures come from an old yt-dlp, since YouTube changes often. When several
//! copies are installed, mpv gets the newest, and an error names the copy mpv used
//! when it is months old.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use chrono::NaiveDate;
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

pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute())
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// Folders searched for programs, in this order: the user's own folders, PATH, then
/// the usual install folders. The user's folders come first, as Debian's and Ubuntu's
/// ~/.profile puts them: a yt-dlp installed there (with pipx, uv or pip) is usually
/// newer than the system's. Apps started from the macOS Finder, or from some desktop
/// sessions, get a short PATH without these folders and without Homebrew.
/// Relative PATH entries are skipped, so the current folder is never searched.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
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

/// The largest window mpv opens, as a share of the screen's width and height. Without
/// a limit, mpv opens a window as large as the video: a 4K or vertical video then runs
/// off the screen. A single percentage (60%) would limit only the width. Smaller
/// videos keep their own size.
const WINDOW_FIT: &str = "60%x60%";

/// Options in mpv.conf that set the window size. When one is there, Tidings leaves the
/// window size to it.
const WINDOW_OPTIONS: [&str; 5] = ["autofit", "autofit-larger", "autofit-smaller", "geometry", "window-scale"];

/// The player's arguments for one video. `proxy` is the user's proxy setting.
/// `fit_window` limits mpv's window to `WINDOW_FIT`; IINA sizes its own windows.
pub fn args(player: &Player, url: &Url, proxy: &str, fit_window: bool) -> Result<Vec<String>> {
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
                if fit_window {
                    args.push(format!("--autofit-larger={WINDOW_FIT}"));
                }
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

/// Whether the user's or the system's mpv.conf sets the window size.
fn mpv_conf_sizes_window() -> bool {
    let mut dirs = Vec::new();
    // The folders mpv reads its configuration from, user folders first.
    if let Some(dir) = std::env::var_os("MPV_HOME") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        dirs.push(PathBuf::from(dir).join("mpv"));
    }
    if let Some(home) = home() {
        dirs.push(home.join(".config/mpv"));
        dirs.push(home.join(".mpv"));
    }
    for dir in ["/etc/mpv", "/usr/local/etc/mpv", "/opt/homebrew/etc/mpv", "/usr/pkg/etc/mpv"] {
        dirs.push(PathBuf::from(dir));
    }
    dirs.iter()
        .filter(|d| d.is_absolute())
        .any(|d| std::fs::read_to_string(d.join("mpv.conf")).is_ok_and(|text| sets_window_size(&text)))
}

/// Whether mpv.conf text sets one of `WINDOW_OPTIONS`, in any profile. Comments do not count.
fn sets_window_size(conf: &str) -> bool {
    conf.lines().any(|line| {
        let line = line.trim().trim_start_matches("--");
        let name = line.split(['=', ' ', '\t']).next().unwrap_or("");
        WINDOW_OPTIONS.contains(&name)
    })
}

/// Starts the player and waits up to `wait` for an early failure, such as a missing
/// yt-dlp or an unavailable video. A player still running after that is playing. If
/// it fails later (mpv can take longer than `wait` to find that a stream does not
/// open), the error goes to `late` instead.
pub async fn play(
    player: &Player,
    url: &Url,
    proxy: &str,
    wait: Duration,
    late: impl FnOnce(Error) + Send + 'static,
) -> Result<()> {
    let fit_window = player.kind == Kind::Mpv && !mpv_conf_sizes_window();
    let args = args(player, url, proxy, fit_window)?;
    let player = player.clone();
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
    // The player runs on its own thread until it exits, which also reaps it.
    std::thread::spawn(move || {
        let result = run(&player, &args);
        // The send fails once `play` has stopped waiting.
        if let Err(Err(e)) = done_tx.send(result) {
            late(e);
        }
    });
    match tokio::time::timeout(wait, &mut done_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Ok(()),
        Err(_) => {
            // Still running, so playing. After `close`, a result can no longer be sent
            // here and goes to `late`; one sent just before is read now.
            done_rx.close();
            done_rx.try_recv().unwrap_or(Ok(()))
        }
    }
}

/// Runs the player until it exits. Blocks.
fn run(player: &Player, args: &[String]) -> Result<()> {
    // Let mpv find yt-dlp, and yt-dlp find its JavaScript runtime, even when Tidings
    // was started from the Finder with a short PATH.
    let dirs = player_dirs(player.kind);
    // The player writes errors to a private temporary file, not to a pipe. A pipe
    // closes when Tidings quits, and the player would then be killed (SIGPIPE) the
    // next time it writes an error, in the middle of a video.
    let (log_path, log) = error_log()?;
    let mut command = Command::new(&player.path);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log.try_clone()?);
    if let Ok(path) = std::env::join_paths(&dirs) {
        command.env("PATH", path);
    }
    let spawned = command.spawn();
    // The player writes to its own handle and Tidings reads through `log`, so the
    // name can go now. Nothing is left behind, even if Tidings quits.
    let _ = std::fs::remove_file(&log_path);
    let mut child = spawned.map_err(|e| Error::msg(format!("Could not start {}: {e}", player.name)))?;
    let status = child.wait()?;
    check_exit(player, status, &last_lines(log), &dirs)
}

/// yt-dlp releases older than this often fail on YouTube, which changes often.
pub(crate) const OLD_YT_DLP_DAYS: i64 = 90;

/// How long `yt-dlp --version` may take.
const VERSION_WAIT: Duration = Duration::from_secs(10);

/// The folders for the player's PATH. IINA brings its own yt-dlp; for other players,
/// see `newest_yt_dlp_first`.
fn player_dirs(kind: Kind) -> Vec<PathBuf> {
    let dirs = search_dirs();
    if kind == Kind::Iina {
        return dirs;
    }
    newest_yt_dlp_first(dirs)
}

/// When there are several copies of yt-dlp, moves the folder of the newest one to the
/// front, so mpv uses it. A system package can be months older than a copy the user
/// installed, and an old yt-dlp finds the video but returns streams that do not open.
/// On a tie, the search order decides.
fn newest_yt_dlp_first(mut dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let copies: Vec<PathBuf> = dirs.iter().map(|d| d.join("yt-dlp")).filter(|p| is_executable(p)).collect();
    if copies.len() < 2 {
        return dirs;
    }
    // max_by_key returns the last of equal maximums, so search from the end.
    let newest = copies
        .iter()
        .rev()
        .max_by_key(|p| yt_dlp_version(p, &dirs))
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);
    if let Some(dir) = newest {
        dirs.retain(|d| *d != dir);
        dirs.insert(0, dir);
    }
    dirs
}

/// The release date that `yt-dlp --version` prints. Each copy runs once, and again
/// after its file changes (an update).
pub(crate) fn yt_dlp_version(path: &Path, dirs: &[PathBuf]) -> Option<NaiveDate> {
    type Seen = HashMap<(PathBuf, SystemTime), Option<NaiveDate>>;
    static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Default::default);
    let real = std::fs::canonicalize(path).ok()?;
    let key = (real.clone(), std::fs::metadata(&real).and_then(|m| m.modified()).ok()?);
    if let Some(version) = SEEN.lock().ok().and_then(|seen| seen.get(&key).copied()) {
        return version;
    }
    let version = version_output(path, dirs).as_deref().and_then(parse_version);
    if let Ok(mut seen) = SEEN.lock() {
        seen.insert(key, version);
    }
    version
}

/// Runs `<path> --version` with the player's PATH and returns what it prints.
/// yt-dlp makes no connections for this.
fn version_output(path: &Path, dirs: &[PathBuf]) -> Option<String> {
    let mut command = Command::new(path);
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Ok(env) = std::env::join_paths(dirs) {
        command.env("PATH", env);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + VERSION_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.take(256).read_to_string(&mut out).ok()?;
    Some(out)
}

/// "2026.09.16", or "2026.09.16.232802" for a nightly build.
pub(crate) fn parse_version(text: &str) -> Option<NaiveDate> {
    let mut parts = text.trim().split('.');
    let mut next = || parts.next()?.parse::<u32>().ok();
    let (year, month, day) = (next()?, next()?, next()?);
    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)
}

/// Advice for a failed video when the yt-dlp that `user` (mpv, Tidings) ran is old.
fn old_yt_dlp_hint(user: &str, path: &Path, version: NaiveDate, today: NaiveDate) -> Option<String> {
    let days = (today - version).num_days();
    (days > OLD_YT_DLP_DAYS).then(|| {
        format!(
            "{user} used yt-dlp {} from {}, which is {} months old. YouTube often breaks older \
             versions: update yt-dlp and try again.",
            version.format("%Y.%m.%d"),
            path.display(),
            days / 30
        )
    })
}

/// "Failed to open https://host/long?token" becomes "it could not open the stream
/// from host". The rest of the address is long, and means nothing to the user.
fn stream_failure(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("Failed to open ")?;
    let url = Url::parse(rest.split_whitespace().next()?).ok()?;
    Some(format!("it could not open the stream from {}.", url.host_str()?))
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

/// Turns the player's exit into an error message, if it failed. `dirs` is the PATH the
/// player had.
fn check_exit(player: &Player, status: ExitStatus, lines: &[String], dirs: &[PathBuf]) -> Result<()> {
    // mpv exits with 1, 2 or 3 when it could not play; 0 and 4 mean the user quit.
    let failed = match player.kind {
        Kind::Mpv => matches!(status.code(), Some(1..=3)),
        _ => status.code().is_some_and(|c| c != 0),
    };
    if !failed {
        return Ok(());
    }
    // yt-dlp's own errors say the most, then a stream that did not open.
    let reason = lines
        .iter()
        .rev()
        .find_map(|l| l.split_once("ERROR: ").map(|(_, rest)| rest.to_string()))
        .or_else(|| lines.iter().rev().find_map(|l| stream_failure(l)))
        .or_else(|| lines.last().cloned());
    let mut message = match reason {
        Some(reason) => format!("{} could not play this video: {reason}", player.name),
        None => format!("{} could not play this video (exit code {}).", player.name, status.code().unwrap_or(-1)),
    };
    if player.kind == Kind::Mpv {
        if !message.ends_with('.') {
            message.push('.');
        }
        match find_in(dirs, "yt-dlp") {
            None => message.push_str(" mpv needs yt-dlp for YouTube and similar sites, and it was not found."),
            Some(path) => {
                let today = chrono::Local::now().date_naive();
                let hint = yt_dlp_version(&path, dirs).and_then(|v| old_yt_dlp_hint("mpv", &path, v, today));
                if let Some(hint) = hint {
                    message.push(' ');
                    message.push_str(&hint);
                }
            }
        }
    }
    Err(Error::msg(message))
}

// Listening: the audio of a video in Tidings' own podcast player ---------------------

/// An audio-only stream of a video, found by yt-dlp. Tidings plays it through its
/// media proxy (net.rs), like a podcast; yt-dlp only finds the address.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    /// A direct http(s) address of the audio file. YouTube's expire after some hours
    /// and only work from the IP address that asked for them, so the UI asks again
    /// each time it starts playing.
    pub url: String,
    pub mime: String,
    pub duration: Option<f64>,
}

/// How long yt-dlp may take to find the stream.
const LOOKUP_WAIT: Duration = Duration::from_secs(60);

/// Audio only, as one file read in byte ranges (not HLS or DASH pieces). AAC in MP4
/// first, since every web view plays it.
const AUDIO_FORMAT: &str =
    "bestaudio[ext=m4a][protocol=https]/bestaudio[protocol=https]/bestaudio[protocol=http]";

const NO_YT_DLP: &str = "Listening needs yt-dlp, and it was not found. Install it \
    (with Homebrew, your package manager or pipx) and try again.";

/// The yt-dlp to run for listening, and the PATH to give it. The newest copy on
/// PATH and in the usual folders wins; on macOS, the copy inside IINA is the last
/// choice.
pub(crate) fn find_yt_dlp() -> Option<(PathBuf, Vec<PathBuf>)> {
    let dirs = newest_yt_dlp_first(search_dirs());
    if let Some(path) = find_in(&dirs, "yt-dlp") {
        return Some((path, dirs));
    }
    if cfg!(target_os = "macos") {
        let mut apps = vec![PathBuf::from("/Applications")];
        apps.extend(home().map(|h| h.join("Applications")));
        // IINA ships yt-dlp under its old name.
        let bundled = apps.iter().map(|a| a.join("IINA.app/Contents/MacOS/youtube-dl")).find(|p| is_executable(p));
        return bundled.map(|path| (path, dirs));
    }
    None
}

/// yt-dlp's arguments. `--ignore-config` keeps out the user's yt-dlp settings: a
/// `--cookies-from-browser` line there would send their YouTube account cookies.
/// yt-dlp understands every proxy Tidings accepts, socks5h:// (Tor) included, and
/// with socks5h:// it leaves DNS to the proxy, as net.rs does.
fn audio_args(page: &Url, proxy: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "--ignore-config",
        "--no-playlist",
        "--no-warnings",
        "--no-progress",
        "--dump-single-json",
        "--socket-timeout",
        "20",
        "--format",
        AUDIO_FORMAT,
    ]
    .map(String::from)
    .to_vec();
    let proxy = proxy.trim();
    if !proxy.is_empty() {
        args.push("--proxy".into());
        args.push(proxy.into());
    }
    // After "--", nothing can be read as an option.
    args.push("--".into());
    args.push(page.to_string());
    args
}

/// Finds an audio-only stream for a video page (YouTube, Vimeo...) with yt-dlp.
pub async fn audio_stream(page: &Url, proxy: &str) -> Result<AudioStream> {
    // Picking the newest yt-dlp may run `yt-dlp --version`.
    let found = tokio::task::spawn_blocking(find_yt_dlp).await.ok().flatten();
    let (path, dirs) = found.ok_or_else(|| Error::msg(NO_YT_DLP))?;
    let mut command = tokio::process::Command::new(&path);
    command
        .args(audio_args(page, proxy))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Ok(env) = std::env::join_paths(&dirs) {
        command.env("PATH", env);
    }
    let output = tokio::time::timeout(LOOKUP_WAIT, command.output())
        .await
        .map_err(|_| Error::msg("yt-dlp took too long to find the audio of this video."))?
        .map_err(|e| Error::msg(format!("Could not start yt-dlp: {e}")))?;
    if output.status.success() {
        if let Some(stream) = parse_audio(&output.stdout) {
            return Ok(stream);
        }
    }
    let errors = String::from_utf8_lossy(&output.stderr);
    let mut message = if errors.contains("Requested format is not available") || output.status.success() {
        "This video has no audio-only stream that Tidings can play.".to_string()
    } else {
        let reason = errors
            .lines()
            .rev()
            .find_map(|l| l.split_once("ERROR: ").map(|(_, rest)| rest.trim().chars().take(400).collect::<String>()));
        match reason {
            Some(reason) => format!("yt-dlp could not find the audio: {reason}"),
            None => format!("yt-dlp could not find the audio (exit code {}).", output.status.code().unwrap_or(-1)),
        }
    };
    if !message.ends_with('.') {
        message.push('.');
    }
    let today = chrono::Local::now().date_naive();
    let version = tokio::task::spawn_blocking({
        let (path, dirs) = (path.clone(), dirs.clone());
        move || yt_dlp_version(&path, &dirs)
    })
    .await
    .ok()
    .flatten();
    if let Some(hint) = version.and_then(|v| old_yt_dlp_hint("Tidings", &path, v, today)) {
        message.push(' ');
        message.push_str(&hint);
    }
    Err(Error::msg(message))
}

/// Reads yt-dlp's JSON. Only a single file over http(s) with sound is accepted.
fn parse_audio(json: &[u8]) -> Option<AudioStream> {
    let info: serde_json::Value = serde_json::from_slice(json).ok()?;
    // The chosen format is in requested_downloads; older yt-dlp puts it at the top.
    let chosen = info.get("requested_downloads").and_then(|d| d.get(0)).unwrap_or(&info);
    let field = |name: &str| chosen.get(name).or_else(|| info.get(name)).and_then(|v| v.as_str());
    let url = Url::parse(field("url")?).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !matches!(field("protocol")?, "http" | "https") {
        return None;
    }
    if field("acodec").is_some_and(|c| c == "none") {
        return None;
    }
    let mime = match field("ext").unwrap_or("") {
        "m4a" | "mp4" => "audio/mp4",
        "webm" => "audio/webm",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        _ => "audio/mp4",
    };
    let duration = info.get("duration").and_then(|d| d.as_f64()).filter(|d| d.is_finite() && *d > 0.0);
    Some(AudioStream { url: url.to_string(), mime: mime.into(), duration })
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
            args(&player(Kind::Mpv), &url, "", true).unwrap(),
            [
                "--force-window=immediate",
                "--msg-level=all=error",
                "--autofit-larger=60%x60%",
                "https://www.youtube.com/watch?v=abc"
            ]
        );
        // mpv.conf sets the window size: Tidings leaves it alone.
        assert!(!args(&player(Kind::Mpv), &url, "", false).unwrap().iter().any(|a| a.starts_with("--autofit")));
        // IINA sizes its own windows.
        assert_eq!(args(&player(Kind::Iina), &url, "", true).unwrap(), ["--no-stdin", "https://www.youtube.com/watch?v=abc"]);
        assert_eq!(args(&player(Kind::Other), &url, " ", true).unwrap(), ["https://www.youtube.com/watch?v=abc"]);

        let mpv = args(&player(Kind::Mpv), &url, "http://proxy:3128", true).unwrap();
        assert!(mpv.contains(&"--http-proxy=http://proxy:3128".to_string()));
        assert!(mpv.contains(&"--ytdl-raw-options-append=proxy=http://proxy:3128".to_string()));
        let iina = args(&player(Kind::Iina), &url, "http://proxy:3128", true).unwrap();
        assert!(iina.contains(&"--mpv-http-proxy=http://proxy:3128".to_string()));
        assert_eq!(iina.last().unwrap(), url.as_str());

        // A proxy the player cannot use must stop playback, not be skipped.
        assert!(args(&player(Kind::Mpv), &url, "socks5h://127.0.0.1:9050", true).is_err());
        assert!(args(&player(Kind::Mpv), &url, "https://proxy:3128", true).is_err());
        assert!(args(&player(Kind::Other), &url, "http://proxy:3128", true).is_err());
    }

    #[test]
    fn window_size_in_mpv_conf() {
        assert!(sets_window_size("hwdec=vaapi\nautofit=60%x60%\n"));
        assert!(sets_window_size("[youtube]\n  geometry=50%:50%"));
        assert!(sets_window_size("--autofit-larger=90%x90%"));
        assert!(sets_window_size("window-scale 0.5"));
        assert!(!sets_window_size("hwdec=vaapi\n# autofit=60%x60%\nautofit-something=1\n"));
        assert!(!sets_window_size(""));
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
        play(&p, &url, "", Duration::from_secs(15), |e| eprintln!("failed late: {e}")).await.unwrap();
    }

    #[test]
    fn listening() {
        let page = video();
        let args = audio_args(&page, " socks5h://127.0.0.1:9050 ");
        assert_eq!(args[0], "--ignore-config");
        assert!(args.windows(2).any(|w| w == ["--proxy", "socks5h://127.0.0.1:9050"]));
        assert_eq!(&args[args.len() - 2..], ["--", page.as_str()]);
        assert!(!audio_args(&page, "").contains(&"--proxy".to_string()));

        let json = br#"{"title": "T", "duration": 19.0, "requested_downloads": [
            {"url": "https://rr2.googlevideo.com/videoplayback?x=1", "protocol": "https",
             "ext": "m4a", "acodec": "mp4a.40.2"}]}"#;
        assert_eq!(
            parse_audio(json),
            Some(AudioStream {
                url: "https://rr2.googlevideo.com/videoplayback?x=1".into(),
                mime: "audio/mp4".into(),
                duration: Some(19.0)
            })
        );
        // Older yt-dlp: the format at the top level.
        let top = br#"{"url": "https://a.example/x.webm", "protocol": "https", "ext": "webm", "acodec": "opus"}"#;
        assert_eq!(parse_audio(top).unwrap().mime, "audio/webm");
        // Pieces (HLS), a video without sound, other schemes and broken JSON are refused.
        assert!(parse_audio(br#"{"url": "https://a.example/x.m3u8", "protocol": "m3u8_native"}"#).is_none());
        assert!(parse_audio(br#"{"url": "https://a.example/x", "protocol": "https", "acodec": "none"}"#).is_none());
        assert!(parse_audio(br#"{"url": "file:///etc/passwd", "protocol": "https"}"#).is_none());
        assert!(parse_audio(b"not json").is_none());
    }

    /// Asks the yt-dlp on this machine for the audio of a 19-second YouTube video:
    /// `cargo test live_listen -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live_listen() {
        let page = Url::parse("https://www.youtube.com/watch?v=jNQXAC9IVRw").unwrap();
        let stream = audio_stream(&page, "").await.unwrap();
        eprintln!("{} {:?} from {}", stream.mime, stream.duration, Url::parse(&stream.url).unwrap().host_str().unwrap());
        assert!(stream.url.starts_with("https://"));
    }

    #[test]
    fn yt_dlp_versions() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(parse_version("2026.09.16\n"), Some(date(2026, 9, 16)));
        assert_eq!(parse_version("2026.09.16.232802"), Some(date(2026, 9, 16)));
        assert_eq!(parse_version("yt-dlp 1"), None);
        assert_eq!(parse_version("2026.13.01"), None);

        let path = Path::new("/usr/bin/yt-dlp");
        let hint = old_yt_dlp_hint("mpv", path, date(2026, 3, 17), date(2026, 11, 8)).unwrap();
        assert!(hint.starts_with("mpv used yt-dlp 2026.03.17 from /usr/bin/yt-dlp, which is 7 months old."), "{hint}");
        assert_eq!(old_yt_dlp_hint("mpv", path, date(2026, 9, 16), date(2026, 11, 8)), None);

        assert_eq!(
            stream_failure("Failed to open https://rr11---sn-abc.googlevideo.com/videoplayback?expire=1&sig=2").as_deref(),
            Some("it could not open the stream from rr11---sn-abc.googlevideo.com.")
        );
        assert_eq!(stream_failure("No video or audio streams selected."), None);
    }

    #[test]
    fn newest_yt_dlp_wins() {
        let root = std::env::temp_dir().join(format!("tidings-ytdlp-{}", std::process::id()));
        let dir = |name: &str, version: &str| {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            fake(&d, "yt-dlp", &format!("echo {version}"));
            d
        };
        let user = dir("user", "2026.03.17");
        let system = dir("system", "2026.09.16");
        let broken = dir("broken", "not-a-version");
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        // The newer copy's folder moves to the front; the others keep their order.
        let dirs = newest_yt_dlp_first(vec![user.clone(), empty.clone(), system.clone(), broken.clone()]);
        assert_eq!(dirs, [system.clone(), user.clone(), empty.clone(), broken.clone()]);
        // A copy whose version cannot be read never wins.
        assert_eq!(newest_yt_dlp_first(vec![broken.clone(), user.clone()]), [user.clone(), broken.clone()]);
        // Equal versions, or a single copy: the search order stays.
        let twin = dir("twin", "2026.09.16");
        assert_eq!(newest_yt_dlp_first(vec![system.clone(), twin.clone()]), [system.clone(), twin.clone()]);
        assert_eq!(newest_yt_dlp_first(vec![empty.clone(), user.clone()]), [empty, user]);
        let _ = std::fs::remove_dir_all(&root);
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
        play(&p, &url, "", Duration::from_secs(5), |_| {}).await.unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), url.as_str());

        // An early failure comes back with the player's error line.
        let path = fake(&dir, "mpv", "echo '[ytdl_hook] ERROR: [youtube] abc: Video unavailable' >&2\nexit 2");
        let p = find(path.to_str().unwrap()).unwrap();
        assert_eq!(p.kind, Kind::Mpv);
        let err = play(&p, &url, "", Duration::from_secs(5), |_| {}).await.unwrap_err().to_string();
        assert!(err.starts_with("mpv could not play this video: [youtube] abc: Video unavailable"), "{err}");

        // mpv quit by the user (exit 4) is not an error.
        let path = fake(&dir, "mpv", "exit 4");
        play(&player_at(path), &url, "", Duration::from_secs(5), |_| {}).await.unwrap();

        // A player still running after the wait counts as playing.
        let path = fake(&dir, "slow", "sleep 3");
        play(&player_at(path), &url, "", Duration::from_millis(300), |_| {}).await.unwrap();

        // A failure after the wait still reaches the caller, through `late`.
        let path = fake(
            &dir,
            "mpv",
            "sleep 1\necho 'Failed to open https://rr1.googlevideo.com/videoplayback?sig=x' >&2\n\
             echo 'No video or audio streams selected.' >&2\nexit 2",
        );
        let (tx, rx) = std::sync::mpsc::channel();
        play(&player_at(path), &url, "", Duration::from_millis(200), move |e| tx.send(e.to_string()).unwrap())
            .await
            .unwrap();
        let err = rx.recv_timeout(Duration::from_secs(20)).expect("late failure");
        assert!(err.starts_with("mpv could not play this video: it could not open the stream from rr1.googlevideo.com."), "{err}");

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
