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
    /// AI briefs (brief.rs): "" (off), "ollama", or "openai" for a server with an
    /// OpenAI-style API (LM Studio, llama.cpp). Only on this computer or the local network.
    pub ai_provider: String,
    /// The model server's address. Empty uses the provider's usual local address.
    pub ai_endpoint: String,
    /// The model's name on that server.
    pub ai_model: String,
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
            ai_provider: String::new(),
            ai_endpoint: String::new(),
            ai_model: String::new(),
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
        // reqwest looks up names on this computer for socks5:// and leaves them to the
        // proxy only for socks5h://. With Tor, local lookups would show every feed's
        // name to the network's DNS. Every SOCKS5 proxy can do the lookups itself.
        if let Some(rest) = self.proxy.strip_prefix("socks5://") {
            self.proxy = format!("socks5h://{rest}");
        }
        if !self.proxy.is_empty() {
            let url = url::Url::parse(&self.proxy)
                .map_err(|_| Error::msg("The proxy address is not a valid URL"))?;
            if !matches!(url.scheme(), "http" | "https" | "socks5h") || url.host_str().is_none_or(str::is_empty) {
                return Err(Error::msg(
                    "Proxy must start with http://, https:// or socks5h:// and name a host",
                ));
            }
        }

        self.video_player = self.video_player.trim().to_string();
        if self.video_player.chars().any(|c| c.is_control()) || self.video_player.len() > 1024 {
            return Err(Error::msg("The video player path contains invalid characters"));
        }

        self.ai_provider = self.ai_provider.trim().to_string();
        if !matches!(self.ai_provider.as_str(), "" | "ollama" | "openai") {
            self.ai_provider = String::new();
        }
        self.ai_model = self.ai_model.trim().to_string();
        if self.ai_model.chars().any(|c| c.is_control()) || self.ai_model.len() > 300 {
            return Err(Error::msg("The model name contains invalid characters"));
        }
        self.ai_endpoint = self.ai_endpoint.trim().trim_end_matches('/').to_string();
        if !self.ai_endpoint.is_empty() {
            let url = url::Url::parse(&self.ai_endpoint)
                .map_err(|_| Error::msg("The model server address is not a valid URL"))?;
            crate::net::check_local_model(&url, !self.proxy.is_empty())?;
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

impl Settings {
    /// The model server for briefs, or None when briefs are off.
    pub fn ai_server(&self) -> Option<url::Url> {
        let default = match self.ai_provider.as_str() {
            "ollama" => "http://127.0.0.1:11434",
            // LM Studio's usual port; llama.cpp's llama-server uses 8080.
            "openai" => "http://127.0.0.1:1234",
            _ => return None,
        };
        let address = if self.ai_endpoint.is_empty() { default } else { &self.ai_endpoint };
        url::Url::parse(address).ok()
    }
}

/// Saved settings, and why Tidings must stay offline if they cannot be trusted.
pub struct Loaded {
    pub settings: Settings,
    /// Set when a proxy may have been configured but cannot be used or read. Going
    /// on without it would connect directly, around the proxy (Tor).
    pub offline: Option<String>,
}

pub fn load(conn: &Connection) -> Loaded {
    let row: rusqlite::Result<Option<String>> =
        conn.query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0)).optional();
    let unreadable = || Loaded {
        settings: Settings::default(),
        offline: Some(
            "Your saved settings could not be read, so Tidings is offline: a proxy may have been set. \
             Check Settings → Privacy & network and save to go online."
                .into(),
        ),
    };
    match row {
        // A new installation.
        Ok(None) => Loaded { settings: Settings::default(), offline: None },
        Ok(Some(json)) => match serde_json::from_str::<Settings>(&json) {
            Ok(settings) => settings.repaired(),
            Err(_) => unreadable(),
        },
        Err(_) => unreadable(),
    }
}

impl Settings {
    /// Saved settings that no longer pass `validated` (a rule changed, or the row was
    /// edited). Never falls back to all defaults: that would drop the proxy. Other
    /// invalid fields are reset; an invalid proxy stays as text and means offline.
    fn repaired(self) -> Loaded {
        if let Ok(settings) = self.clone().validated() {
            return Loaded { settings, offline: None };
        }
        let defaults = Settings::default();
        let rest = Settings {
            ai_provider: defaults.ai_provider,
            ai_endpoint: defaults.ai_endpoint,
            ai_model: defaults.ai_model,
            video_player: defaults.video_player,
            user_agent: defaults.user_agent,
            ..self
        };
        match rest.clone().validated() {
            Ok(settings) => Loaded { settings, offline: None },
            Err(_) => Loaded {
                settings: rest,
                offline: Some(
                    "The saved proxy setting cannot be used, so Tidings is offline. \
                     Fix or remove the proxy in Settings → Privacy & network."
                        .into(),
                ),
            },
        }
    }
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
