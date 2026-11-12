use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use lru::LruCache;
use rusqlite::Connection;
use tauri::{AppHandle, Manager};

use crate::error::Result;
use crate::net::Net;
use crate::settings::{self, Settings};
use crate::db;

pub struct CachedImage {
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// Recently shown images, re-encoded. Limited by total size, so a feed with many
/// large images cannot fill the memory.
pub struct ImageCache {
    entries: LruCache<String, Arc<CachedImage>>,
    bytes: usize,
}

impl ImageCache {
    /// All cached images together, at most.
    pub const MAX_BYTES: usize = 64 * 1024 * 1024;
    /// Larger images are not cached.
    pub const MAX_IMAGE: usize = 3 * 1024 * 1024;

    fn new() -> Self {
        ImageCache { entries: LruCache::unbounded(), bytes: 0 }
    }

    pub fn get(&mut self, url: &str) -> Option<Arc<CachedImage>> {
        self.entries.get(url).cloned()
    }

    pub fn put(&mut self, url: String, image: Arc<CachedImage>) {
        if image.bytes.len() > Self::MAX_IMAGE {
            return;
        }
        self.bytes += image.bytes.len();
        if let Some(old) = self.entries.put(url, image) {
            self.bytes -= old.bytes.len();
        }
        while self.bytes > Self::MAX_BYTES {
            match self.entries.pop_lru() {
                Some((_, old)) => self.bytes -= old.bytes.len(),
                None => break,
            }
        }
    }

    #[cfg(test)]
    pub fn size(&self) -> (usize, usize) {
        (self.entries.len(), self.bytes)
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

pub struct AppState {
    db: Mutex<Connection>,
    settings: RwLock<Settings>,
    net: RwLock<Net>,
    /// Feeds being refreshed right now, so the same feed is never fetched twice at once.
    pub inflight: Mutex<HashSet<i64>>,
    pub progress: Mutex<Progress>,
    pub images: Mutex<ImageCache>,
    /// Recently extracted web pages (reader tabs), keyed by URL.
    pub pages: Mutex<LruCache<String, Arc<crate::reader::Extracted>>>,
    /// Media proxy state: final URLs after redirects and recent blocks of audio and video.
    pub media: Arc<crate::media::Media>,
    /// Base URL of the loopback media server (Linux only).
    pub media_base: std::sync::OnceLock<String>,
    pub data_dir: PathBuf,
    /// Set when the saved proxy could not be used at startup and Tidings went offline.
    pub startup_warning: Option<String>,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<Self> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700));
        }
        let conn = db::open(&data_dir.join("tidings.db"))?;
        db::clear_new_flags(&conn)?;
        // If the saved settings or the proxy in them cannot be used, do not connect
        // directly instead: that would send everything around the proxy (Tor). Stay
        // offline until the user saves working settings.
        let loaded = settings::load(&conn);
        let settings = loaded.settings;
        let (net, startup_warning) = match (loaded.offline, Net::new(&settings)) {
            (None, Ok(net)) => (net, None),
            (reason, _) => (
                Net::offline()?,
                Some(reason.unwrap_or_else(|| {
                    "The saved proxy setting cannot be used, so Tidings is offline. \
                     Fix or remove the proxy in Settings → Privacy & network."
                        .to_string()
                })),
            ),
        };
        Ok(AppState {
            db: Mutex::new(conn),
            settings: RwLock::new(settings),
            net: RwLock::new(net),
            inflight: Mutex::new(HashSet::new()),
            progress: Mutex::new(Progress::default()),
            images: Mutex::new(ImageCache::default()),
            pages: Mutex::new(LruCache::new(NonZeroUsize::new(40).unwrap())),
            media: Default::default(),
            media_base: Default::default(),
            data_dir,
            startup_warning,
        })
    }

    /// Locks the database. Never hold this guard across an `.await`.
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn net(&self) -> Net {
        self.net.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace_settings(&self, new: Settings) -> Result<()> {
        let net = Net::new(&new)?;
        settings::save(&self.db(), &new)?;
        *self.net.write().unwrap_or_else(|e| e.into_inner()) = net;
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = new;
        Ok(())
    }
}
