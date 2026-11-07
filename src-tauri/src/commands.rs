//! Commands the UI can call through Tauri IPC.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::db::{self, ArticleItem, ArticleRow, Counts, FeedPatch, Item, ListQuery, Scope, Tree};
use crate::discover::{self, Candidate};
use crate::error::{Error, Result};
use crate::feeds;
use crate::net;
use crate::opml::{self, ImportStats};
use crate::reader;
use crate::sanitize;
use crate::settings::Settings;
use crate::state::AppState;

type AppStateRef<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleDetail {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub published: i64,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_new: bool,
    pub html: String,
    pub blocked_images: usize,
    pub showing_full_text: bool,
    pub has_full_text: bool,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
}

fn detail(row: ArticleRow, load_images: bool, full_text: bool) -> ArticleDetail {
    let use_full = full_text && row.full_content.is_some();
    let source = if use_full { row.full_content.as_deref() } else { row.content.as_deref() };
    let rendered = sanitize::render(source.unwrap_or(""), load_images);
    ArticleDetail {
        id: row.id,
        feed_id: row.feed_id,
        title: row.title,
        url: row.url,
        author: row.author,
        published: row.published,
        is_read: row.is_read,
        is_starred: row.is_starred,
        is_new: row.is_new,
        html: rendered.html,
        blocked_images: rendered.blocked_images,
        showing_full_text: use_full,
        has_full_text: row.full_content.is_some(),
        enclosure_url: row.enclosure_url,
        enclosure_type: row.enclosure_type,
    }
}

fn spawn_refresh(app: &AppHandle, state: &Arc<AppState>, ids: Vec<i64>) {
    let (app, state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        feeds::refresh_many(&app, &state, ids).await;
    });
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_tree(state: AppStateRef<'_>) -> Result<Tree> {
    db::tree(&state.db())
}

#[tauri::command]
pub fn get_counts(state: AppStateRef<'_>) -> Result<Counts> {
    db::counts(&state.db())
}

#[tauri::command]
pub fn list_articles(state: AppStateRef<'_>, query: ListQuery) -> Result<Vec<ArticleItem>> {
    let load_images = state.settings().load_images;
    db::list_articles(&state.db(), &query, |url| {
        if load_images { sanitize::proxy_image_url(url) } else { String::new() }
    })
}

#[tauri::command]
pub fn list_articles_full(
    state: AppStateRef<'_>,
    query: ListQuery,
    load_images: Option<bool>,
) -> Result<Vec<ArticleDetail>> {
    let load = load_images.unwrap_or(state.settings().load_images);
    let conn = state.db();
    let ids = db::list_article_ids(&conn, &query)?;
    ids.into_iter()
        .map(|id| db::article(&conn, id).map(|row| detail(row, load, false)))
        .collect()
}

#[tauri::command]
pub fn get_article(
    state: AppStateRef<'_>,
    id: i64,
    load_images: Option<bool>,
    full_text: Option<bool>,
) -> Result<ArticleDetail> {
    let load = load_images.unwrap_or(state.settings().load_images);
    let row = db::article(&state.db(), id)?;
    Ok(detail(row, load, full_text.unwrap_or(false)))
}

/// Downloads the linked web page and extracts the full article (reader mode).
#[tauri::command]
pub async fn fetch_full_text(
    state: AppStateRef<'_>,
    id: i64,
    load_images: Option<bool>,
) -> Result<ArticleDetail> {
    let settings = state.settings();
    let row = db::article(&state.db(), id)?;
    if row.full_content.is_none() {
        let url = row.url.clone().ok_or_else(|| Error::msg("This article has no link"))?;
        let extracted = reader::extract_url(&state.net(), &url, settings.strip_tracking).await?;
        db::save_full_content(&state.db(), id, &extracted.html)?;
    }
    let row = db::article(&state.db(), id)?;
    Ok(detail(row, load_images.unwrap_or(settings.load_images), true))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderPage {
    pub url: String,
    pub title: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub excerpt: Option<String>,
    pub published: Option<String>,
    pub dir: Option<String>,
    pub html: String,
    pub blocked_images: usize,
}

/// Opens any web page in a reader tab.
#[tauri::command]
pub async fn read_url(
    state: AppStateRef<'_>,
    url: String,
    load_images: Option<bool>,
    reload: Option<bool>,
) -> Result<ReaderPage> {
    let settings = state.settings();
    let key = net::parse_web_url(&url)?.to_string();
    let cached = if reload.unwrap_or(false) {
        None
    } else {
        state.pages.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned()
    };
    let page = match cached {
        Some(page) => page,
        None => {
            let page = Arc::new(reader::extract_url(&state.net(), &key, settings.strip_tracking).await?);
            state.pages.lock().unwrap_or_else(|e| e.into_inner()).put(key, page.clone());
            page
        }
    };
    let rendered = sanitize::render(&page.html, load_images.unwrap_or(settings.load_images));
    Ok(ReaderPage {
        url: page.url.clone(),
        title: page.title.clone(),
        byline: page.byline.clone(),
        site_name: page.site_name.clone(),
        excerpt: page.excerpt.clone(),
        published: page.published.clone(),
        dir: page.dir.clone(),
        html: rendered.html,
        blocked_images: rendered.blocked_images,
    })
}

#[tauri::command]
pub fn set_read(state: AppStateRef<'_>, ids: Vec<i64>, read: bool) -> Result<()> {
    db::set_read(&state.db(), &ids, read)
}

#[tauri::command]
pub fn set_starred(state: AppStateRef<'_>, id: i64, starred: bool) -> Result<()> {
    db::set_starred(&state.db(), id, starred)
}

#[tauri::command]
pub fn mark_all_read(
    state: AppStateRef<'_>,
    scope: Scope,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<usize> {
    db::mark_scope_read(&state.db(), &scope, &db::DateRange { since, until })
}

#[tauri::command]
pub fn delete_articles(state: AppStateRef<'_>, ids: Vec<i64>) -> Result<()> {
    db::delete_articles(&state.db(), &ids)
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn discover_feeds(state: AppStateRef<'_>, url: String) -> Result<Vec<Candidate>> {
    discover::discover(&state.net(), &url).await
}

#[tauri::command]
pub fn add_feed(
    app: AppHandle,
    state: AppStateRef<'_>,
    url: String,
    folder_id: Option<i64>,
    title: Option<String>,
) -> Result<i64> {
    let url = net::parse_web_url(&url)?;
    let id = {
        let conn = state.db();
        if db::feed_id_by_url(&conn, url.as_str())?.is_some() {
            return Err(Error::msg("You are already subscribed to this feed"));
        }
        let title = title.as_deref().map(str::trim).filter(|t| !t.is_empty());
        db::insert_feed(&conn, url.as_str(), folder_id, title, None)?
    };
    spawn_refresh(&app, state.inner(), vec![id]);
    Ok(id)
}

#[tauri::command]
pub fn update_feed(app: AppHandle, state: AppStateRef<'_>, id: i64, mut patch: FeedPatch) -> Result<()> {
    patch.url = net::parse_web_url(&patch.url)?.to_string();
    let url_changed = {
        let conn = state.db();
        let old = db::feed(&conn, id)?;
        if old.url != patch.url && db::feed_id_by_url(&conn, &patch.url)?.is_some() {
            return Err(Error::msg("Another subscription already uses this address"));
        }
        db::update_feed(&conn, id, &patch)?;
        old.url != patch.url
    };
    if url_changed {
        spawn_refresh(&app, state.inner(), vec![id]);
    }
    Ok(())
}

#[tauri::command]
pub fn delete_feed(state: AppStateRef<'_>, id: i64) -> Result<()> {
    db::delete_feed(&state.db(), id)
}

#[tauri::command]
pub fn move_feed(
    state: AppStateRef<'_>,
    id: i64,
    folder_id: Option<i64>,
    before: Option<Item>,
) -> Result<()> {
    db::move_feed(&state.db(), id, folder_id, before)
}

/// Adds a feed or folder to Favorites, or removes it.
#[tauri::command]
pub fn set_pinned(state: AppStateRef<'_>, item: Item, pinned: bool) -> Result<()> {
    db::set_pinned(&state.db(), item, pinned)
}

#[tauri::command]
pub fn create_folder(state: AppStateRef<'_>, title: String, parent_id: Option<i64>) -> Result<i64> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::msg("Folder name cannot be empty"));
    }
    db::create_folder(&state.db(), title, parent_id)
}

#[tauri::command]
pub fn rename_folder(state: AppStateRef<'_>, id: i64, title: String) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::msg("Folder name cannot be empty"));
    }
    db::rename_folder(&state.db(), id, title)
}

#[tauri::command]
pub fn delete_folder(state: AppStateRef<'_>, id: i64) -> Result<()> {
    db::delete_folder(&state.db(), id)
}

#[tauri::command]
pub fn move_folder(
    state: AppStateRef<'_>,
    id: i64,
    parent_id: Option<i64>,
    before: Option<Item>,
) -> Result<()> {
    db::move_folder(&state.db(), id, parent_id, before)
}

#[tauri::command]
pub fn set_folder_expanded(state: AppStateRef<'_>, id: i64, expanded: bool) -> Result<()> {
    db::set_folder_expanded(&state.db(), id, expanded)
}

/// Starts a refresh in the background. Progress arrives as events.
#[tauri::command]
pub fn refresh_feeds(
    app: AppHandle,
    state: AppStateRef<'_>,
    feed_ids: Option<Vec<i64>>,
    folder_id: Option<i64>,
) -> Result<usize> {
    let ids = {
        let conn = state.db();
        match (feed_ids, folder_id) {
            (Some(ids), _) => ids,
            (None, Some(folder)) => db::feeds_in_folder(&conn, folder)?,
            (None, None) => db::schedules(&conn)?.into_iter().filter(|s| !s.paused).map(|s| s.id).collect(),
        }
    };
    let count = ids.len();
    spawn_refresh(&app, state.inner(), ids);
    Ok(count)
}

// ---------------------------------------------------------------------------
// OPML
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn import_opml(app: AppHandle, state: AppStateRef<'_>) -> Result<Option<ImportStats>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Import subscriptions")
        .add_filter("OPML", &["opml", "xml"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else { return Ok(None) };
    let path = path.into_path().map_err(|e| Error::msg(e.to_string()))?;
    if std::fs::metadata(&path)?.len() > 20 * 1024 * 1024 {
        return Err(Error::msg("This file is too large to be an OPML subscription list"));
    }
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8_lossy(&bytes);
    let outlines = opml::parse(&text)?;
    let stats = opml::import(&state.db(), &outlines)?;
    spawn_refresh(&app, state.inner(), stats.new_feed_ids.clone());
    Ok(Some(stats))
}

#[tauri::command]
pub async fn export_opml(app: AppHandle, state: AppStateRef<'_>) -> Result<Option<String>> {
    let xml = opml::export(&state.db())?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Export subscriptions")
        .set_file_name("tidings-subscriptions.opml")
        .add_filter("OPML", &["opml"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else { return Ok(None) };
    let path = path.into_path().map_err(|e| Error::msg(e.to_string()))?;
    std::fs::write(&path, xml)?;
    Ok(Some(path.display().to_string()))
}

// ---------------------------------------------------------------------------
// Settings and system
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: AppStateRef<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(state: AppStateRef<'_>, settings: Settings) -> Result<Settings> {
    let settings = settings.validated()?;
    state.replace_settings(settings.clone())?;
    Ok(settings)
}

/// Opens a link in the system browser. Only web and mail links are allowed.
#[tauri::command]
pub fn open_external(url: String) -> Result<()> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| Error::msg("Invalid link"))?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(Error::msg("Only web and email links can be opened"));
    }
    open::that_detached(parsed.as_str())?;
    Ok(())
}

#[tauri::command]
pub fn set_badge(app: AppHandle, count: i64) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_badge_count(if count > 0 { Some(count) } else { None });
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub platform: &'static str,
    /// Linux: base URL of the loopback audio server. None elsewhere.
    pub media_base: Option<String>,
}

#[tauri::command]
pub fn app_info(state: AppStateRef<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.display().to_string(),
        platform: std::env::consts::OS,
        media_base: state.media_base.get().cloned(),
    }
}
