use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (compatible; Tidings/0.1; feed reader)";

/// User settings. Stored as one JSON value in the `settings` table.
/// `#[serde(default)]` lets old saved settings load after new fields are added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Minutes between automatic refreshes. 0 turns automatic refresh off.
    pub refresh_interval: u32,
    pub refresh_on_startup: bool,
    /// "all", "count" or "days".
    pub retention_mode: String,
    pub retention_count: u32,
    pub retention_days: u32,
    /// Load remote images (always through the built-in image proxy).
    pub load_images: bool,
    /// Remove utm_* and similar tracking parameters from links.
    pub strip_tracking: bool,
    pub https_only: bool,
    /// Empty = no proxy. Examples: socks5h://127.0.0.1:9050, http://proxy:3128
    pub proxy: String,
    pub user_agent: String,
    /// Seconds before a selected article is marked as read.
    pub mark_read_delay: u32,
    /// What a plain click on a link inside an article does: "tab" or "browser".
    pub link_target: String,
    pub theme: String,
    pub accent: String,
    pub reader_font: String,
    pub reader_size: u32,
    pub view_mode: String,
    pub dock_badge: bool,
    pub concurrency: u32,
    /// External video player: empty finds mpv or IINA, else a program name or path.
    pub video_player: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            refresh_interval: 60,
            refresh_on_startup: true,
            retention_mode: "count".into(),
            retention_count: 1000,
            retention_days: 90,
            load_images: true,
            strip_tracking: true,
            https_only: false,
            proxy: String::new(),
            user_agent: DEFAULT_USER_AGENT.into(),
            mark_read_delay: 0,
            link_target: "tab".into(),
            theme: "system".into(),
            accent: "ember".into(),
            reader_font: "serif".into(),
            reader_size: 18,
            view_mode: "widescreen".into(),
            dock_badge: true,
            concurrency: 6,
            video_player: String::new(),
        }
    }
}

impl Settings {
    /// Clamp numbers and reject values that would break networking.
    pub fn validated(mut self) -> Result<Self> {
        let pick = |value: &str, allowed: &[&str], fallback: &str| -> String {
            if allowed.contains(&value) { value.to_string() } else { fallback.to_string() }
        };
        self.refresh_interval = self.refresh_interval.min(7 * 24 * 60);
        self.retention_mode = pick(&self.retention_mode, &["all", "count", "days"], "count");
        self.retention_count = self.retention_count.clamp(10, 1_000_000);
        self.retention_days = self.retention_days.clamp(1, 36_500);
        self.mark_read_delay = self.mark_read_delay.min(60);
        self.link_target = pick(&self.link_target, &["tab", "browser"], "tab");
        self.theme = pick(&self.theme, &["system", "light", "dark", "sepia"], "system");
        self.accent = pick(
            &self.accent,
            &["ember", "ocean", "forest", "violet", "rose", "graphite"],
            "ember",
        );
        self.reader_font = pick(&self.reader_font, &["serif", "sans", "mono"], "serif");
        self.reader_size = self.reader_size.clamp(13, 28);
        self.view_mode = pick(&self.view_mode, &["widescreen", "normal", "combined"], "widescreen");
        self.concurrency = self.concurrency.clamp(1, 16);

        self.proxy = self.proxy.trim().to_string();
        if !self.proxy.is_empty() {
            let url = url::Url::parse(&self.proxy)
                .map_err(|_| Error::msg("The proxy address is not a valid URL"))?;
            if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
                return Err(Error::msg(
                    "Proxy must start with http://, https://, socks5:// or socks5h://",
                ));
            }
        }

        self.video_player = self.video_player.trim().to_string();
        if self.video_player.chars().any(|c| c.is_control()) || self.video_player.len() > 1024 {
            return Err(Error::msg("The video player path contains invalid characters"));
        }

        self.user_agent = self.user_agent.trim().to_string();
        if self.user_agent.is_empty() {
            self.user_agent = DEFAULT_USER_AGENT.into();
        }
        if self.user_agent.chars().any(|c| c.is_control()) || self.user_agent.len() > 300 {
            return Err(Error::msg("The user agent contains invalid characters"));
        }
        Ok(self)
    }
}

pub fn load(conn: &Connection) -> Settings {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    raw.and_then(|json| serde_json::from_str::<Settings>(&json).ok())
        .and_then(|s| s.validated().ok())
        .unwrap_or_default()
}

pub fn save(conn: &Connection, settings: &Settings) -> Result<()> {
    let json = serde_json::to_string(settings).map_err(|e| Error::msg(e.to_string()))?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('app', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![json],
    )?;
    Ok(())
}
