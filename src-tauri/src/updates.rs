//! The security check in Settings: are Tidings and the programs it relies on up to date?
//!
//! - The offline check reads only this computer: Tidings' build date, the yt-dlp
//!   version, and on Linux and the BSDs the WebKitGTK version in use. It runs at
//!   startup and can only say that something is old.
//! - The online check runs only when the user clicks "Check now", never by itself.
//!   It asks GitHub for the newest Tidings and yt-dlp releases, and webkitgtk.org for
//!   its latest security advisory, through net.rs (so through the proxy). It can say
//!   that a version has known security holes.
//!
//! Each finding comes with what to do, matched to how the program was installed.

use std::path::Path;

use chrono::{Datelike, NaiveDate};
use reqwest::header::ACCEPT;
use serde::Serialize;
use url::Url;

use crate::error::{Error, Result};
use crate::net::{self, Net};
use crate::video;

const RELEASES_PAGE: &str = "https://github.com/melsiddieg/tidings/releases/latest";
const TIDINGS_API: &str = "https://api.github.com/repos/melsiddieg/tidings/releases/latest";
const YT_DLP_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const WEBKIT_ADVISORIES: &str = "https://webkitgtk.org/security.html";

/// A Tidings build older than this gets a reminder to look for a new release.
const OLD_TIDINGS_DAYS: i64 = 180;
/// The day this copy of Tidings was built, from build.rs.
const BUILD_DATE: &str = env!("TIDINGS_BUILD_DATE");

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Up to date, as far as Tidings can tell.
    Ok,
    /// Months old (found offline). It may still be fine.
    Old,
    /// A newer release or a security fix exists (found online).
    Update,
    /// Not installed. Only some features need it.
    Missing,
    /// No answer: the version could not be read, or the online check failed.
    Unknown,
    /// Nothing Tidings can check; the message says what to do.
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    /// "tidings", "yt-dlp" or "webkit".
    pub id: &'static str,
    pub name: String,
    pub version: Option<String>,
    /// Where it is installed, or when it was built.
    pub detail: Option<String>,
    pub status: Status,
    pub message: String,
    /// A command that fixes it, when one fits.
    pub command: Option<String>,
    /// A page to open, when that is the fix.
    pub link: Option<String>,
}

/// The offline check. Blocking: it may run `yt-dlp --version`.
pub fn check_offline(today: NaiveDate) -> Vec<Component> {
    vec![tidings(today), yt_dlp(today), webkit(today)]
}

/// The online check: the offline one, then the newest versions and the latest
/// WebKitGTK advisory. Only on the user's click.
pub async fn check_online(net: &Net, today: NaiveDate) -> Vec<Component> {
    let mut list = tokio::task::spawn_blocking(move || check_offline(today)).await.unwrap_or_default();
    // On macOS, WebKit comes with the system: there is no advisory to ask for.
    let advisory = async {
        if cfg!(webkitgtk) {
            latest_advisory(net).await
        } else {
            None
        }
    };
    let (tidings, yt_dlp, webkit) = tokio::join!(latest_release(net, TIDINGS_API), latest_release(net, YT_DLP_API), advisory);
    for c in &mut list {
        match c.id {
            "tidings" => online_tidings(c, tidings.as_deref()),
            "yt-dlp" => online_yt_dlp(c, yt_dlp.as_deref()),
            "webkit" if cfg!(webkitgtk) => online_webkit(c, webkit.as_ref()),
            _ => {}
        }
    }
    list
}

// Tidings ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Install {
    MacApp,
    Flatpak,
    Snap,
    AppImage,
    /// A .deb, .rpm or the Linux tarball.
    LinuxPackage,
    /// The tarball from scripts/build-bsd.sh.
    Bsd,
}

fn install() -> Install {
    if cfg!(target_os = "macos") {
        Install::MacApp
    } else if std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists() {
        Install::Flatpak
    } else if std::env::var_os("SNAP").is_some() {
        Install::Snap
    } else if std::env::var_os("APPIMAGE").is_some() {
        Install::AppImage
    } else if cfg!(target_os = "linux") {
        Install::LinuxPackage
    } else {
        Install::Bsd
    }
}

/// How to get a new Tidings: (advice, command, link).
fn tidings_update() -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match install() {
        Install::MacApp => ("Download the newest .dmg from the releases page.", None, Some(RELEASES_PAGE)),
        Install::Flatpak => ("Update it in your software center, or run:", Some("flatpak update"), None),
        Install::Snap => ("Update it in your software center, or run:", Some("sudo snap refresh"), None),
        Install::AppImage => (
            "Download the newest AppImage from the releases page. The AppImage carries its own \
             WebKitGTK, which only gets security fixes with a new Tidings release; the .deb and \
             .rpm use your system's.",
            None,
            Some(RELEASES_PAGE),
        ),
        Install::LinuxPackage => ("Download the newest .deb or .rpm from the releases page.", None, Some(RELEASES_PAGE)),
        Install::Bsd => (
            "Build the newest release with scripts/build-bsd.sh (see the releases page).",
            None,
            Some(RELEASES_PAGE),
        ),
    }
}

fn tidings(today: NaiveDate) -> Component {
    let built = NaiveDate::parse_from_str(BUILD_DATE, "%Y-%m-%d").ok();
    let days = built.map(|b| (today - b).num_days()).unwrap_or(0);
    let (advice, command, link) = tidings_update();
    let old = days > OLD_TIDINGS_DAYS;
    Component {
        id: "tidings",
        name: "Tidings".into(),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        detail: built.map(|b| format!("built {}", b.format("%Y-%m-%d"))),
        status: if old { Status::Old } else { Status::Ok },
        message: if old {
            format!("This copy was built {} months ago. There may be a newer release. {advice}", days / 30)
        } else {
            "Built recently.".into()
        },
        command: old.then(|| command.map(str::to_string)).flatten(),
        link: old.then(|| link.map(str::to_string)).flatten(),
    }
}

fn online_tidings(c: &mut Component, latest: Option<&str>) {
    let current = semver(env!("CARGO_PKG_VERSION"));
    match latest.and_then(semver) {
        None => unknown(c, "Could not get the newest version from GitHub."),
        Some(newest) if current.is_some_and(|v| v >= newest) => {
            c.status = Status::Ok;
            c.message = "This is the newest release.".into();
            c.command = None;
            c.link = None;
        }
        Some(newest) => {
            let (advice, command, link) = tidings_update();
            c.status = Status::Update;
            c.message = format!("Tidings {}.{}.{} is out. {advice}", newest.0, newest.1, newest.2);
            c.command = command.map(str::to_string);
            c.link = link.map(str::to_string);
        }
    }
}

/// "v0.8.0" or "0.8.0".
fn semver(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    Some((next()?, next()?, next()?))
}

// yt-dlp -------------------------------------------------------------------------

fn yt_dlp(today: NaiveDate) -> Component {
    let Some((path, dirs)) = video::find_yt_dlp() else {
        return Component {
            id: "yt-dlp",
            name: "yt-dlp".into(),
            version: None,
            detail: None,
            status: Status::Missing,
            message: "Not installed. Listen needs it, and so does mpv for YouTube and similar sites.".into(),
            command: yt_dlp_install().map(str::to_string),
            link: None,
        };
    };
    let version = video::yt_dlp_version(&path, &dirs);
    let mut c = Component {
        id: "yt-dlp",
        name: "yt-dlp".into(),
        version: version.map(|v| v.format("%Y.%m.%d").to_string()),
        detail: Some(path.display().to_string()),
        status: Status::Ok,
        message: "Recent enough for YouTube.".into(),
        command: None,
        link: None,
    };
    match version {
        None => {
            c.status = Status::Unknown;
            c.message = "Could not read its version.".into();
        }
        Some(v) if (today - v).num_days() > video::OLD_YT_DLP_DAYS => {
            let (advice, command) = yt_dlp_update(&path);
            c.status = Status::Old;
            c.message = format!(
                "This version is {} months old, and YouTube often breaks older versions. {advice}",
                (today - v).num_days() / 30
            );
            c.command = command;
        }
        Some(_) => {}
    }
    c
}

fn online_yt_dlp(c: &mut Component, latest: Option<&str>) {
    if c.status == Status::Missing {
        return;
    }
    let installed = c.version.as_deref().and_then(video::parse_version);
    match (latest.and_then(video::parse_version), installed) {
        (None, _) | (_, None) => unknown(c, "Could not compare it with the newest release."),
        (Some(newest), Some(have)) if have >= newest => {
            c.status = Status::Ok;
            c.message = "This is the newest release.".into();
            c.command = None;
        }
        (Some(newest), Some(_)) => {
            let path = c.detail.clone().unwrap_or_default();
            let (advice, command) = yt_dlp_update(Path::new(&path));
            c.status = Status::Update;
            c.message = format!("yt-dlp {} is out. {advice}", newest.format("%Y.%m.%d"));
            c.command = command;
        }
    }
}

fn yt_dlp_install() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("brew install yt-dlp")
    } else if cfg!(target_os = "freebsd") {
        Some("sudo pkg install yt-dlp")
    } else if cfg!(target_os = "openbsd") {
        Some("doas pkg_add yt-dlp")
    } else {
        // Distribution packages are often months behind YouTube.
        Some("pipx install yt-dlp")
    }
}

/// How to update the yt-dlp at `path`: (advice, command). The way it was installed
/// shows in where its real file lives.
fn yt_dlp_update(path: &Path) -> (String, Option<String>) {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let head = std::fs::File::open(&real)
        .and_then(|f| {
            use std::io::Read;
            let mut buf = Vec::new();
            f.take(64 * 1024).read_to_end(&mut buf).map(|_| buf)
        })
        .unwrap_or_default();
    yt_dlp_update_for(&real.to_string_lossy(), &head, &os_release_ids())
}

fn yt_dlp_update_for(real: &str, head: &[u8], distro: &[String]) -> (String, Option<String>) {
    let run = |cmd: &str| ("Update it with:".to_string(), Some(cmd.to_string()));
    if real.contains("/IINA.app/") {
        return ("This is the copy inside IINA: update IINA, or install yt-dlp with Homebrew.".into(), Some("brew install yt-dlp".into()));
    }
    if real.contains("/Cellar/") || real.starts_with("/opt/homebrew/") || real.contains("/linuxbrew/") {
        return run("brew upgrade yt-dlp");
    }
    if real.contains("/uv/tools/") {
        return run("uv tool upgrade yt-dlp");
    }
    if real.contains("/pipx/venvs/") {
        return run("pipx upgrade yt-dlp");
    }
    if real.starts_with("/opt/local/") {
        return run("sudo port upgrade yt-dlp");
    }
    if real.starts_with("/snap/") {
        return run("sudo snap refresh yt-dlp");
    }
    // The single-file release is a Python zip archive and updates itself.
    if head.windows(4).any(|w| w == b"PK\x03\x04") {
        return run(&format!("{real} -U"));
    }
    // A script that pip wrote.
    if !real.starts_with("/usr/") && head.windows(6).any(|w| w == b"yt_dlp") {
        return run("python3 -m pip install --user --upgrade yt-dlp");
    }
    if cfg!(target_os = "freebsd") {
        return run("sudo pkg upgrade yt-dlp");
    }
    if cfg!(target_os = "openbsd") {
        return run("doas pkg_add -u yt-dlp");
    }
    let system = system_update(distro).map(|cmd| cmd.replace(" upgrade", " upgrade yt-dlp"));
    (
        "Update it with your package manager. Distribution packages are often months behind \
         YouTube; a copy from pipx (pipx install yt-dlp) is newer, and Tidings prefers the newest."
            .into(),
        system,
    )
}

// WebKit -------------------------------------------------------------------------

/// The WebKitGTK library in use (also inside an AppImage or Flatpak).
#[cfg(webkitgtk)]
fn webkit_version() -> Option<(u32, u32, u32)> {
    extern "C" {
        fn webkit_get_major_version() -> std::os::raw::c_uint;
        fn webkit_get_minor_version() -> std::os::raw::c_uint;
        fn webkit_get_micro_version() -> std::os::raw::c_uint;
    }
    // SAFETY: these WebKitGTK functions take no arguments and only return numbers.
    unsafe { Some((webkit_get_major_version(), webkit_get_minor_version(), webkit_get_micro_version())) }
}

#[cfg(not(webkitgtk))]
fn webkit_version() -> Option<(u32, u32, u32)> {
    None
}

/// WebKitGTK releases a stable series every six months: 2.44 in March 2024, 2.46 in
/// September 2024, and so on. Upstream fixes only the newest series.
fn newest_webkit_series(today: NaiveDate) -> u32 {
    let months = (today.year() - 2024) * 12 + today.month0() as i32 - 2;
    44 + 2 * (months.max(0) / 6) as u32
}

fn webkit(today: NaiveDate) -> Component {
    if !cfg!(webkitgtk) {
        return Component {
            id: "webkit",
            name: "WebKit".into(),
            version: macos_version(),
            detail: Some("part of macOS".into()),
            status: Status::Info,
            message: "WebKit, which shows articles, comes with macOS and gets its security fixes \
                      from macOS updates: System Settings → General → Software Update."
                .into(),
            command: None,
            link: None,
        };
    }
    let version = webkit_version();
    let mut c = Component {
        id: "webkit",
        name: "WebKitGTK".into(),
        version: version.map(|(a, b, c)| format!("{a}.{b}.{c}")),
        detail: Some(match install() {
            Install::AppImage => "inside the AppImage".into(),
            Install::Flatpak => "from the Flatpak runtime".into(),
            _ => "from your system".into(),
        }),
        status: Status::Ok,
        message: "The current series.".into(),
        command: None,
        link: None,
    };
    let Some((_, minor, _)) = version else {
        c.status = Status::Unknown;
        c.message = "Could not read its version.".into();
        return c;
    };
    let newest = newest_webkit_series(today);
    if minor + 4 <= newest {
        let (advice, command) = webkit_update();
        c.status = Status::Old;
        c.message = format!(
            "This series is about {} months older than the newest (2.{newest}), and only the newest \
             gets security fixes. {advice}",
            (newest - minor) * 3
        );
        c.command = command;
    } else if minor + 2 <= newest {
        c.message = "One series behind the newest. Your system may still be catching up.".into();
    }
    c
}

/// How to update WebKitGTK: (advice, command).
fn webkit_update() -> (String, Option<String>) {
    match install() {
        Install::AppImage => {
            let (advice, _, _) = tidings_update();
            (advice.into(), None)
        }
        Install::Flatpak => ("Update the GNOME runtime:".into(), Some("flatpak update".into())),
        Install::Snap => ("Update your snaps:".into(), Some("sudo snap refresh".into())),
        _ => {
            let command = if cfg!(target_os = "freebsd") {
                Some("sudo pkg upgrade".into())
            } else if cfg!(target_os = "openbsd") {
                Some("doas pkg_add -u".into())
            } else {
                system_update(&os_release_ids())
            };
            ("Install your system's updates:".into(), command)
        }
    }
}

fn online_webkit(c: &mut Component, advisory: Option<&Advisory>) {
    let installed = c.version.as_deref().and_then(semver).map(|(a, b, d)| (a as u32, b as u32, d as u32));
    let (Some(advisory), Some(installed)) = (advisory, installed) else {
        return unknown(c, "Could not read the latest WebKitGTK security advisory.");
    };
    match fixed_in(installed, &advisory.fixes) {
        Some(fix) => {
            let (advice, command) = webkit_update();
            c.status = Status::Update;
            c.message = format!(
                "This version has security holes fixed in {}.{}.{} ({}, {}). {advice}",
                fix.0, fix.1, fix.2, advisory.id, advisory.date
            );
            c.command = command;
            c.link = Some(advisory.url());
        }
        None => {
            if c.status != Status::Old {
                c.status = Status::Ok;
            }
            c.message = format!("No known security holes as of {} ({}).", advisory.id, advisory.date);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Advisory {
    /// "WSA-2026-0006".
    id: String,
    /// "29 September 2026".
    date: String,
    /// Every "before X.Y.Z" in it: the versions that fix its holes.
    fixes: Vec<(u32, u32, u32)>,
}

impl Advisory {
    fn url(&self) -> String {
        format!("https://webkitgtk.org/security/{}.html", self.id)
    }
}

async fn latest_advisory(net: &Net) -> Option<Advisory> {
    let list = fetch(net, WEBKIT_ADVISORIES, "text/html").await.ok()?;
    let (id, date) = first_advisory(&list)?;
    let mut advisory = Advisory { id, date, fixes: Vec::new() };
    let page = fetch(net, &advisory.url(), "text/html").await.ok()?;
    advisory.fixes = fix_versions(&page);
    (!advisory.fixes.is_empty()).then_some(advisory)
}

/// The id and date of the newest advisory in the list page. The page address is
/// built from the id, which must look like WSA-2026-0006.
fn first_advisory(html: &str) -> Option<(String, String)> {
    let start = html.find("/security/WSA-")? + "/security/".len();
    let id: String = html[start..].chars().take(13).collect();
    let valid = id.len() == 13
        && id.starts_with("WSA-")
        && id[4..8].chars().all(|c| c.is_ascii_digit())
        && &id[8..9] == "-"
        && id[9..].chars().all(|c| c.is_ascii_digit());
    if !valid {
        return None;
    }
    let after = &html[start..];
    let date = after
        .find("news_entry\">")
        .map(|i| &after[i + "news_entry\">".len()..])
        .and_then(|rest| rest.split('<').next())
        .map(|d| d.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').take(30).collect::<String>())
        // "29 SEPTEMBER 2026" becomes "29 September 2026".
        .map(|d| {
            d.split_whitespace()
                .map(|w| w.chars().take(1).chain(w.chars().skip(1).flat_map(char::to_lowercase)).collect::<String>())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| id[4..8].to_string());
    Some((id, date))
}

/// Every version after "before " in an advisory ("WebKitGTK and WPE WebKit before 2.54.0").
fn fix_versions(html: &str) -> Vec<(u32, u32, u32)> {
    let mut found = Vec::new();
    for (i, _) in html.match_indices("before ") {
        let rest = &html[i + "before ".len()..];
        let token: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Some((a, b, c)) = semver(token.trim_end_matches('.')) {
            if let (Ok(a), Ok(b), Ok(c)) = (u32::try_from(a), u32::try_from(b), u32::try_from(c)) {
                if a == 2 && !found.contains(&(a, b, c)) {
                    found.push((a, b, c));
                }
            }
        }
    }
    found
}

/// The version that fixes `installed`, if it is still affected. A fix in its own
/// series decides; for a series without one (no longer maintained), the newest fix.
fn fixed_in(installed: (u32, u32, u32), fixes: &[(u32, u32, u32)]) -> Option<(u32, u32, u32)> {
    let own = fixes.iter().filter(|f| (f.0, f.1) == (installed.0, installed.1)).max();
    let needed = *own.or(fixes.iter().max())?;
    (installed < needed).then_some(needed)
}

// Shared -------------------------------------------------------------------------

fn unknown(c: &mut Component, why: &str) {
    if c.status == Status::Ok {
        c.status = Status::Unknown;
        c.message = why.into();
    } else {
        c.message = format!("{} ({why})", c.message);
    }
}

async fn latest_release(net: &Net, api: &str) -> Option<String> {
    let json = fetch(net, api, "application/vnd.github+json").await.ok()?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let tag = value.get("tag_name")?.as_str()?;
    (tag.len() <= 40).then(|| tag.to_string())
}

async fn fetch(net: &Net, url: &str, accept: &str) -> Result<String> {
    let url = Url::parse(url).map_err(|_| Error::msg("bad address"))?;
    let response = net.get(&url, None).await?.header(ACCEPT, accept).send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("HTTP {}", response.status().as_u16())));
    }
    let bytes = net::read_limited(response, 2 * 1024 * 1024).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// ID and ID_LIKE from /etc/os-release: ["ubuntu", "debian"].
fn os_release_ids() -> Vec<String> {
    let text = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .unwrap_or_default();
    os_ids(&text)
}

fn os_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("ID=").or_else(|| line.strip_prefix("ID_LIKE=")) {
            ids.extend(value.trim_matches('"').split_whitespace().map(str::to_ascii_lowercase));
        }
    }
    ids
}

/// The command that installs system updates on this Linux distribution.
fn system_update(ids: &[String]) -> Option<String> {
    let has = |id: &str| ids.iter().any(|i| i == id);
    let command = if has("debian") || has("ubuntu") {
        "sudo apt update && sudo apt upgrade"
    } else if has("fedora") || has("rhel") {
        "sudo dnf upgrade --refresh"
    } else if has("arch") {
        "sudo pacman -Syu"
    } else if has("opensuse") || has("suse") {
        "sudo zypper update"
    } else if has("alpine") {
        "sudo apk upgrade"
    } else {
        return None;
    };
    Some(command.into())
}

/// "26.1" on macOS, from the system's own version file.
fn macos_version() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let after = &plist[plist.find("<key>ProductVersion</key>")?..];
    let value = after.split("<string>").nth(1)?.split("</string>").next()?;
    (value.len() <= 20 && value.chars().all(|c| c.is_ascii_digit() || c == '.')).then(|| format!("macOS {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn webkit_series_and_fixes() {
        assert_eq!(newest_webkit_series(date(2024, 3, 15)), 44);
        assert_eq!(newest_webkit_series(date(2024, 9, 1)), 46);
        assert_eq!(newest_webkit_series(date(2026, 11, 9)), 54);
        assert_eq!(newest_webkit_series(date(2023, 1, 1)), 44);

        // Fixed in the same series: decides. Old series: the newest fix.
        let fixes = [(2, 52, 7), (2, 54, 0)];
        assert_eq!(fixed_in((2, 54, 1), &fixes), None);
        assert_eq!(fixed_in((2, 54, 0), &fixes), None);
        assert_eq!(fixed_in((2, 52, 7), &fixes), None);
        assert_eq!(fixed_in((2, 52, 3), &fixes), Some((2, 52, 7)));
        assert_eq!(fixed_in((2, 50, 9), &fixes), Some((2, 54, 0)));
        assert_eq!(fixed_in((2, 56, 0), &fixes), None);
        assert_eq!(fixed_in((2, 50, 0), &[]), None);
    }

    #[test]
    fn reads_advisories() {
        let list = r#"<h2>WebKitGTK Security Advisories</h2>
            <h3 class="news_title"><a href="/security/WSA-2026-0006.html">WebKitGTK and WPE WebKit Security Advisory WSA-2026-0006</a></h3>
            <p class="news_entry">29 SEPTEMBER 2026</p>
            <h3 class="news_title"><a href="/security/WSA-2026-0005.html">…</a></h3>"#;
        assert_eq!(first_advisory(list), Some(("WSA-2026-0006".into(), "29 September 2026".into())));
        assert_eq!(first_advisory(r#"<a href="/security/WSA-../../x.html">"#), None);
        assert_eq!(first_advisory("<p>nothing</p>"), None);

        let page = "CVE-2024-1283 Versions affected: WebKitGTK and WPE WebKit before 2.54.0 or earlier. \
                    Chrome prior to 121.0.6167.160. CVE-2 Versions affected: WebKitGTK before 2.52.7 and \
                    2.54.0. before 2.54.0.";
        assert_eq!(fix_versions(page), [(2, 54, 0), (2, 52, 7)]);
    }

    #[test]
    fn versions_and_install_advice() {
        assert_eq!(semver("v0.8.0"), Some((0, 8, 0)));
        assert_eq!(semver("0.10.2"), Some((0, 10, 2)));
        assert_eq!(semver("nightly"), None);

        let none: Vec<String> = Vec::new();
        let cmd = |real: &str, head: &[u8]| yt_dlp_update_for(real, head, &none).1;
        assert_eq!(cmd("/opt/homebrew/Cellar/yt-dlp/2026.10.26/bin/yt-dlp", b"").as_deref(), Some("brew upgrade yt-dlp"));
        assert_eq!(cmd("/home/a/.local/share/uv/tools/yt-dlp/bin/yt-dlp", b"").as_deref(), Some("uv tool upgrade yt-dlp"));
        assert_eq!(cmd("/home/a/.local/share/pipx/venvs/yt-dlp/bin/yt-dlp", b"").as_deref(), Some("pipx upgrade yt-dlp"));
        assert_eq!(cmd("/home/a/bin/yt-dlp", b"#!/usr/bin/env python3\nPK\x03\x04...").as_deref(), Some("/home/a/bin/yt-dlp -U"));
        assert_eq!(
            cmd("/home/a/.local/bin/yt-dlp", b"#!/usr/bin/python3\nfrom yt_dlp import main").as_deref(),
            Some("python3 -m pip install --user --upgrade yt-dlp")
        );
        assert!(cmd("/Applications/IINA.app/Contents/MacOS/youtube-dl", b"").is_some());

        let ubuntu = os_ids("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n");
        assert_eq!(ubuntu, ["ubuntu", "debian"]);
        assert_eq!(system_update(&ubuntu).as_deref(), Some("sudo apt update && sudo apt upgrade"));
        if cfg!(target_os = "linux") {
            assert_eq!(
                yt_dlp_update_for("/usr/bin/yt-dlp", b"#!/usr/bin/python3\nimport yt_dlp", &ubuntu).1.as_deref(),
                Some("sudo apt update && sudo apt upgrade yt-dlp")
            );
        }
        assert_eq!(system_update(&os_ids("ID=fedora\n")).as_deref(), Some("sudo dnf upgrade --refresh"));
        assert_eq!(system_update(&os_ids("ID=gentoo\n")), None);
    }

    #[test]
    fn offline_check_runs() {
        let list = check_offline(date(2026, 11, 9));
        let ids: Vec<&str> = list.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["tidings", "yt-dlp", "webkit"]);
        assert_eq!(list[0].version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
        // Built today: not old.
        let today = NaiveDate::parse_from_str(BUILD_DATE, "%Y-%m-%d").unwrap();
        assert_eq!(tidings(today).status, Status::Ok);
        assert_eq!(tidings(today + chrono::Duration::days(200)).status, Status::Old);
    }

    /// Asks GitHub and webkitgtk.org, as "Check now" does:
    /// `cargo test live_security_check -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_security_check() {
        let net = Net::new(&crate::settings::Settings::default()).unwrap();
        let (t, y, w) = tokio::join!(latest_release(&net, TIDINGS_API), latest_release(&net, YT_DLP_API), latest_advisory(&net));
        println!("tidings {t:?}, yt-dlp {y:?}, webkit {w:?}");
        assert!(t.is_some() && y.is_some() && w.is_some());
        for c in check_online(&net, chrono::Local::now().date_naive()).await {
            println!("{:<10} {:<14} {:?}: {} {:?} {:?}", c.name, c.version.unwrap_or_default(), c.status, c.message, c.command, c.link);
        }
    }
}
