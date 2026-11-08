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
    pub images: Mutex<LruCache<String, Arc<CachedImage>>>,
    /// Recently extracted web pages (reader tabs), keyed by URL.
    pub pages: Mutex<LruCache<String, Arc<crate::reader::Extracted>>>,
    /// Media proxy state: final URLs after redirects and recent blocks of audio and video.
    pub media: Arc<crate::media::Media>,
    /// Base URL of the loopback media server (Linux only).
    pub media_base: std::sync::OnceLock<String>,
    pub data_dir: PathBuf,
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
        let settings = settings::load(&conn);
        let net = Net::new(&settings).or_else(|_| Net::new(&Settings::default()))?;
        Ok(AppState {
            db: Mutex::new(conn),
            settings: RwLock::new(settings),
            net: RwLock::new(net),
            inflight: Mutex::new(HashSet::new()),
            progress: Mutex::new(Progress::default()),
            images: Mutex::new(LruCache::new(NonZeroUsize::new(400).unwrap())),
            pages: Mutex::new(LruCache::new(NonZeroUsize::new(40).unwrap())),
            media: Default::default(),
            media_base: Default::default(),
            data_dir,
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
