//! Commands the UI can call through Tauri IPC.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::backup;
use crate::brief;
use crate::db::{self, ArticleItem, ArticleRow, Counts, FeedPatch, Item, ListQuery, Scope, Tree};
use crate::discover::{self, Candidate};
use crate::error::{Error, Result};
use crate::export;
use crate::feeds;
use crate::files;
use crate::net;
use crate::opml::{self, ImportPreview, ImportStats, Layout};
use crate::peertube;
use crate::reader;
use crate::rules::Rule;
use crate::sanitize;
use crate::translate;
use crate::settings::Settings;
use crate::state::AppState;
use crate::updates;
use crate::video;

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
    /// The article is mostly Chinese.
    pub chinese: bool,
    /// "zh" (Chinese), "ar" (Arabic) or "" (another language): the UI offers the
    /// translations that fit it and `Settings::translate_to`.
    pub language: &'static str,
    /// For an article the UI offers to translate, in the article view: where its
    /// translated paragraphs and its images go (`translate::layout`). Empty otherwise.
    pub translation_layout: Vec<translate::Block>,
}

/// `translate_to`: the usual translation language, with `Some` when the article
/// view asks, so `translation_layout` is filled for an article it can translate:
/// with Arabic, every article; with English, Chinese and Arabic ones.
fn detail(row: ArticleRow, load_images: bool, full_text: bool, translate_to: Option<translate::Lang>) -> ArticleDetail {
    let use_full = full_text && row.full_content.is_some();
    let source = if use_full { row.full_content.as_deref() } else { row.content.as_deref() };
    let rendered = sanitize::render(source.unwrap_or(""), load_images);
    let text = sanitize::plain_text(&rendered.html);
    let language = translate::language(&format!("{} {}", row.title, text.chars().take(2_000).collect::<String>()));
    let translatable = match translate_to {
        Some(translate::Lang::Arabic) => true,
        Some(translate::Lang::English) => !language.is_empty(),
        None => false,
    };
    let translation_layout = if translatable { translate::layout(source.unwrap_or(""), load_images) } else { Vec::new() };
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
        chinese: language == "zh",
        language,
        translation_layout,
    }
}

/// The language articles are usually translated into.
fn translate_to(settings: &crate::settings::Settings) -> translate::Lang {
    translate::Lang::from_code(&settings.translate_to).unwrap_or_default()
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

// Commands that show article HTML run on the async runtime, not on the main
// thread: a long article must not stop the window while it is cleaned.
#[tauri::command(async)]
pub fn list_articles_full(
    state: AppStateRef<'_>,
    query: ListQuery,
    load_images: Option<bool>,
) -> Result<Vec<ArticleDetail>> {
    let load = load_images.unwrap_or(state.settings().load_images);
    let conn = state.db();
    let ids = db::list_article_ids(&conn, &query)?;
    ids.into_iter()
        .map(|id| db::article(&conn, id).map(|row| detail(row, load, false, None)))
        .collect()
}

#[tauri::command(async)]
pub fn get_article(
    state: AppStateRef<'_>,
    id: i64,
    load_images: Option<bool>,
    full_text: Option<bool>,
) -> Result<ArticleDetail> {
    let settings = state.settings();
    let row = db::article(&state.db(), id)?;
    Ok(detail(row, load_images.unwrap_or(settings.load_images), full_text.unwrap_or(false), Some(translate_to(&settings))))
}

/// Downloads the linked web page and extracts the full article (reader mode).
#[tauri::command]
pub async fn fetch_full_text(
    state: AppStateRef<'_>,
    id: i64,
    load_images: Option<bool>,
) -> Result<ArticleDetail> {
    let settings = state.settings();
    let generation = state.generation();
    let row = db::article(&state.db(), id)?;
    if row.full_content.is_none() {
        let url = row.url.clone().ok_or_else(|| Error::msg("This article has no link"))?;
        // The link may reach the local network only if it is on the host and port of
        // its own feed, and the user allowed that feed there.
        let info = db::fetch_info(&state.db(), row.feed_id)?;
        let feed_url = url::Url::parse(&info.url).ok().filter(|_| info.local);
        let extracted =
            reader::extract_url(&state.net(), &url, feed_url.as_ref(), settings.strip_tracking).await?;
        db::save_full_content(&*state.db_since(generation)?, id, &extracted.html)?;
    }
    let row = db::article(&state.db(), id)?;
    Ok(detail(row, load_images.unwrap_or(settings.load_images), true, Some(translate_to(&settings))))
}

/// Reader pages kept in memory hold at most this much HTML together.
const PAGE_CACHE_BYTES: usize = 16 * 1024 * 1024;

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
            // Reader tabs open links from articles and web pages, which anyone can
            // write. They never reach the local network, also when the user has a
            // feed there: any feed could otherwise link to that host. Local pages
            // open in the browser.
            let page = Arc::new(reader::extract_url(&state.net(), &key, None, settings.strip_tracking).await?);
            let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
            pages.put(key, page.clone());
            // The cache holds up to 40 pages, and at most `PAGE_CACHE_BYTES` of HTML.
            while pages.len() > 1 && pages.iter().map(|(_, p)| p.html.len()).sum::<usize>() > PAGE_CACHE_BYTES {
                pages.pop_lru();
            }
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
    feed_type: Option<String>,
) -> Result<usize> {
    db::mark_scope_read(&state.db(), &scope, &db::DateRange { since, until }, feed_type.as_deref())
}

#[tauri::command]
pub fn delete_articles(state: AppStateRef<'_>, ids: Vec<i64>) -> Result<()> {
    let conn = state.db();
    db::delete_articles(&conn, &ids)?;
    // The deleted text is overwritten in the database; empty the log that may still hold it.
    db::checkpoint(&conn);
    Ok(())
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

/// `allow_local`: the user agreed to connect to this address on the local network.
#[tauri::command]
pub async fn discover_feeds(state: AppStateRef<'_>, url: String, allow_local: Option<bool>) -> Result<Vec<Candidate>> {
    discover::discover(&state.net(), &url, allow_local.unwrap_or(false)).await
}

#[tauri::command]
pub fn add_feed(
    app: AppHandle,
    state: AppStateRef<'_>,
    url: String,
    folder_id: Option<i64>,
    title: Option<String>,
    allow_local: Option<bool>,
) -> Result<i64> {
    let url = net::parse_web_url(&url)?;
    let allow_local = allow_local.unwrap_or(false);
    // A feed on the local network needs the user's consent for that address. A web
    // page could otherwise name `http://192.168.1.1/...` as its feed and have it
    // subscribed in one click.
    if !allow_local && !net::host_looks_public(&url) {
        return Err(net::needs_consent(&url));
    }
    let id = {
        let conn = state.db();
        if db::feed_id_by_url(&conn, url.as_str())?.is_some() {
            return Err(Error::msg("You are already subscribed to this feed"));
        }
        let title = title.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let id = db::insert_feed(&conn, url.as_str(), folder_id, title, None)?;
        if allow_local {
            db::set_feed_local(&conn, id, true)?;
        }
        id
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
    let conn = state.db();
    db::delete_feed(&conn, id)?;
    // As in `delete_articles`: the feed's articles must not stay in the log.
    db::checkpoint(&conn);
    Ok(())
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

/// Sets the type of feeds by hand (`feed_type` None: back to the detected type).
#[tauri::command]
pub fn set_feed_type(state: AppStateRef<'_>, ids: Vec<i64>, feed_type: Option<String>) -> Result<usize> {
    db::set_feed_type(&state.db(), &ids, feed_type.as_deref())
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
    let conn = state.db();
    db::delete_folder(&conn, id)?;
    db::checkpoint(&conn);
    Ok(())
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

/// Deletes several feeds and folders at once. Returns how many feeds were deleted.
#[tauri::command]
pub fn delete_items(state: AppStateRef<'_>, items: Vec<Item>) -> Result<usize> {
    let conn = state.db();
    let deleted = db::delete_items(&conn, &items)?;
    db::checkpoint(&conn);
    Ok(deleted)
}

/// Removes every subscription and article, to start over.
#[tauri::command]
pub fn delete_all_subscriptions(state: AppStateRef<'_>) -> Result<()> {
    let conn = state.db();
    db::delete_all_subscriptions(&conn)?;
    db::checkpoint(&conn);
    Ok(())
}

/// Opens or closes the given folders, or all folders when `ids` is None.
#[tauri::command]
pub fn set_folders_expanded(state: AppStateRef<'_>, ids: Option<Vec<i64>>, expanded: bool) -> Result<()> {
    db::set_folders_expanded(&state.db(), ids.as_deref(), expanded)
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

/// Reads an OPML file the user picks and compares it with the subscriptions. The
/// feeds are kept for `import_opml`, which adds them.
#[tauri::command]
pub async fn preview_opml(app: AppHandle, state: AppStateRef<'_>) -> Result<Option<ImportPreview>> {
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
    // Parsing is CPU work on a file from anywhere: keep it off the async threads.
    let pending = tokio::task::spawn_blocking(move || -> Result<opml::Pending> {
        Ok(opml::feeds_in(&opml::parse(&String::from_utf8_lossy(&bytes))?))
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let preview = opml::preview(&state.db(), &pending, &name)?;
    *state.opml_import.lock().unwrap_or_else(|e| e.into_inner()) = Some(pending);
    Ok(Some(preview))
}

/// Adds the new feeds of the file `preview_opml` read. `layout` says whether
/// subscribed feeds move to the file's folders.
#[tauri::command]
pub fn import_opml(app: AppHandle, state: AppStateRef<'_>, layout: Layout) -> Result<ImportStats> {
    let pending = state
        .opml_import
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| Error::msg("Choose the OPML file again"))?;
    let stats = opml::apply(&state.db(), &pending, layout)?;
    spawn_refresh(&app, state.inner(), stats.new_feed_ids.clone());
    Ok(stats)
}

/// Forgets the file `preview_opml` read, when the user cancels.
#[tauri::command]
pub fn cancel_opml_import(state: AppStateRef<'_>) {
    *state.opml_import.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
    files::write_private(&path, xml.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

// ---------------------------------------------------------------------------
// Backup and restore (backup.rs), starred articles (export.rs)
// ---------------------------------------------------------------------------

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Copies the whole database to a file the user picks.
#[tauri::command]
pub async fn backup(app: AppHandle, state: AppStateRef<'_>) -> Result<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Back up Tidings")
        .set_file_name(format!("tidings-backup-{}.tidings", today()))
        .add_filter("Tidings backup", &["tidings"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else { return Ok(None) };
    let path = path.into_path().map_err(|e| Error::msg(e.to_string()))?;
    let source = state.db_path();
    let target = path.clone();
    // A second, read-only connection: the app keeps working while the copy is made.
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        backup::copy_to(&conn, &target)
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;
    Ok(Some(path.display().to_string()))
}

/// Lets the user pick a backup, checks it and says what restoring it would do.
/// `restore_backup` then restores that file.
#[tauri::command]
pub async fn choose_backup(app: AppHandle, state: AppStateRef<'_>) -> Result<Option<backup::Summary>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Restore a backup")
        .add_filter("Tidings backup", &["tidings", "db"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else { return Ok(None) };
    let path = path.into_path().map_err(|e| Error::msg(e.to_string()))?;
    let same = |a: &std::path::Path, b: &std::path::Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same(&path, &state.db_path()) {
        return Err(Error::msg("This is the database Tidings uses now. Choose a backup."));
    }
    let current = state.settings();
    let file = path.clone();
    let summary = tokio::task::spawn_blocking(move || backup::inspect(&file, &current))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;
    *state.restore_file.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    Ok(Some(summary))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// Where the data from before the restore is kept.
    pub previous: String,
    /// Set when Tidings is offline now because the restored proxy cannot be used.
    pub offline: Option<String>,
}

/// Replaces everything with the backup `choose_backup` checked.
#[tauri::command]
pub async fn restore_backup(state: AppStateRef<'_>) -> Result<RestoreResult> {
    // Kept until the restore succeeds, so the dialog can try again after a refusal.
    let path = state
        .restore_file
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| Error::msg("Choose a backup to restore first"))?;
    translate::stop();
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<RestoreResult> {
        let mut conn = state.db();
        // A refresh stores articles by feed id, which the restore gives to other feeds.
        if state.refreshing() {
            return Err(Error::msg("Feeds are being refreshed. Restore the backup when that is done."));
        }
        let previous = state.data_dir.join(backup::PREVIOUS_FILE);
        let loaded = backup::restore(&mut conn, &path, &previous, &state.settings())?;
        let offline = loaded.offline.clone();
        state.restored(&conn, loaded)?;
        *state.restore_file.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(RestoreResult { previous: previous.display().to_string(), offline })
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?
}

/// Saves every starred article in one file: "html" or "markdown".
#[tauri::command]
pub async fn export_starred(app: AppHandle, state: AppStateRef<'_>, format: String, images: bool) -> Result<Option<String>> {
    let markdown = match format.as_str() {
        "html" => false,
        "markdown" => true,
        _ => return Err(Error::msg("Choose HTML or Markdown")),
    };
    if db::tree(&state.db())?.starred == 0 {
        return Err(Error::msg("There are no starred articles to export"));
    }
    let (extension, label) = if markdown { ("md", "Markdown") } else { ("html", "HTML") };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Export starred articles")
        .set_file_name(format!("tidings-starred-{}.{extension}", today()))
        .add_filter(label, &[extension]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.ok().flatten() else { return Ok(None) };
    let path = path.into_path().map_err(|e| Error::msg(e.to_string()))?;
    let articles = db::starred_articles(&state.db())?;
    // Cleaning the HTML of many articles is CPU work: keep it off the async threads.
    let text = tokio::task::spawn_blocking(move || {
        if markdown { export::markdown(&articles, images) } else { export::html(&articles, images) }
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?;
    files::write_private(&path, text.as_bytes())?;
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

/// The external video player Tidings would use now, if one is found.
#[tauri::command]
pub fn video_player(state: AppStateRef<'_>) -> Option<video::Player> {
    video::find(&state.settings().video_player)
}

/// Plays a video link in the external player (mpv or IINA). See video.rs.
/// A failure after `video::STARTUP_WAIT` comes as a "video-failed" event.
#[tauri::command]
pub async fn play_video(app: AppHandle, state: AppStateRef<'_>, url: String) -> Result<()> {
    let settings = state.settings();
    let url = net::parse_web_url(&url)?;
    if settings.https_only && url.scheme() == "http" {
        return Err(Error::msg("HTTPS-only mode is on, and this video address uses plain http"));
    }
    // Feeds control these links. Like reader tabs, they never lead to the local
    // network. This covers the address given to the player; the player then makes
    // its own connections and follows redirects by itself.
    if state.net().is_private_host(&url).await {
        return Err(Error::blocked());
    }
    let player = video::find(&settings.video_player).ok_or_else(video::not_found)?;
    let late = move |e: Error| {
        let _ = app.emit("video-failed", e.to_string());
    };
    video::play(&player, &url, &settings.proxy, video::STARTUP_WAIT, late).await
}

/// Finds an audio-only stream of a video page with yt-dlp ("Listen"). The UI plays
/// it in the podcast player, through the media proxy. Same checks as `play_video`.
#[tauri::command]
pub async fn audio_stream(state: AppStateRef<'_>, url: String) -> Result<video::AudioStream> {
    let settings = state.settings();
    let url = net::parse_web_url(&url)?;
    if settings.https_only && url.scheme() == "http" {
        return Err(Error::msg("HTTPS-only mode is on, and this video address uses plain http"));
    }
    if state.net().is_private_host(&url).await {
        return Err(Error::blocked());
    }
    let stream = video::audio_stream(&url, &settings.proxy).await?;
    if settings.https_only && stream.url.starts_with("http://") {
        return Err(Error::msg("HTTPS-only mode is on, and this audio stream uses plain http"));
    }
    // Read the first bytes through the media proxy, as the player will: a refusal
    // gets a clear message here instead of "could not be played", and the first
    // block is then in the proxy's memory when the player asks for it.
    if let Err(e) = crate::media::fetch(&state.net(), &state.media, &stream.url, Some("bytes=0-1")).await {
        return Err(Error::msg(format!(
            "The video site refused the audio stream ({e}). Updating yt-dlp often helps: \
             see Settings → Security."
        )));
    }
    Ok(stream)
}

/// A stored brief with its body, for the brief tab.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefView {
    #[serde(flatten)]
    info: db::BriefInfo,
    /// The article filter it was written from, to write it again.
    query: serde_json::Value,
    #[serde(flatten)]
    body: brief::Body,
}

fn brief_view(state: &AppState, id: i64) -> Result<Option<BriefView>> {
    let Some((info, query, body)) = db::get_brief(&state.db(), id)? else { return Ok(None) };
    let query = serde_json::from_str(&query).unwrap_or(serde_json::Value::Null);
    let body = serde_json::from_str(&body).map_err(|_| Error::msg("This brief could not be read"))?;
    Ok(Some(BriefView { info, query, body }))
}

/// The models the configured local model server offers (Settings → AI briefs).
#[tauri::command]
pub async fn ai_models(state: AppStateRef<'_>) -> Result<Vec<String>> {
    brief::models(&state.net(), &state.settings()).await
}

/// Writes a brief of the articles `query` shows, with the local model, and stores it.
#[tauri::command]
pub async fn make_brief(state: AppStateRef<'_>, query: ListQuery, title: String) -> Result<BriefView> {
    let settings = state.settings();
    let generation = state.generation();
    let (sources, total) = db::brief_sources(&state.db(), &query, brief::MAX_ARTICLES)?;
    let title: String = title.trim().chars().take(200).collect();
    let title = if title.is_empty() { "Brief".to_string() } else { title };
    let (body, model) = brief::write(&state.net(), &settings, &title, &sources, total).await?;
    let query_json = serde_json::to_string(&query).map_err(|e| Error::msg(e.to_string()))?;
    let body_json = serde_json::to_string(&body).map_err(|e| Error::msg(e.to_string()))?;
    let id = db::insert_brief(&*state.db_since(generation)?, &title, &model, &query_json, &body_json)?;
    brief_view(&state, id)?.ok_or_else(|| Error::msg("The brief was not stored"))
}

/// One part of a translation, as it is done: the title (`index` None) or a paragraph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationPart {
    article_id: i64,
    full: bool,
    lang: translate::Lang,
    index: Option<usize>,
    total: usize,
    text: String,
}

/// Translates an article with the local model (translate.rs) into `lang`, or the
/// usual language from the settings: the full text when it is loaded and
/// `full_text` is set, else the feed's text. A stored translation is returned at
/// once unless `again`. Parts arrive as "translation-part" events while the model works.
#[tauri::command]
pub async fn translate_article(
    app: AppHandle,
    state: AppStateRef<'_>,
    id: i64,
    full_text: Option<bool>,
    again: Option<bool>,
    lang: Option<translate::Lang>,
) -> Result<translate::Translation> {
    let generation = state.generation();
    let settings = state.settings();
    let lang = lang.unwrap_or_else(|| translate_to(&settings));
    let row = db::article(&state.db(), id)?;
    let full = full_text.unwrap_or(false) && row.full_content.is_some();
    let stored = if again.unwrap_or(false) { None } else { db::translation(&state.db(), id, full, lang.code())? };
    if let Some(found) = stored.and_then(|json| serde_json::from_str(&json).ok()) {
        return Ok(found);
    }
    let source = if full { row.full_content.as_deref() } else { row.content.as_deref() };
    let paragraphs = sanitize::paragraphs(source.unwrap_or(""));
    let total = paragraphs.len();
    let send = |index: Option<usize>, text: &str| {
        let part = TranslationPart { article_id: id, full, lang, index, total, text: text.to_string() };
        let _ = app.emit("translation-part", part);
    };
    let translation = translate::translate(&state.net(), &settings, lang, &row.title, &paragraphs, send).await?;
    let json = serde_json::to_string(&translation).map_err(|e| Error::msg(e.to_string()))?;
    db::save_translation(&*state.db_since(generation)?, id, full, lang.code(), &json)?;
    Ok(translation)
}

/// Stops the translation that runs now.
#[tauri::command]
pub fn stop_translation() {
    translate::stop();
}

// ---------------------------------------------------------------------------
// Feed health and saved searches
// ---------------------------------------------------------------------------

/// The newest article and the article count of each feed (Feed health).
#[tauri::command]
pub fn feed_activity(state: AppStateRef<'_>) -> Result<Vec<db::FeedActivity>> {
    db::feed_activity(&state.db())
}

/// How many articles a list query matches: the unread counts of saved searches.
#[tauri::command]
pub fn count_articles(state: AppStateRef<'_>, query: ListQuery) -> Result<i64> {
    db::count_articles(&state.db(), &query)
}

#[tauri::command]
pub fn list_searches(state: AppStateRef<'_>) -> Result<Vec<db::SavedSearch>> {
    db::searches(&state.db())
}

/// Saves the list state `state` (JSON from the UI) under `title`.
#[tauri::command]
pub fn save_search(state: AppStateRef<'_>, title: String, search: String) -> Result<db::SavedSearch> {
    let title = db::search_title(&title)?;
    if !db::search_state_is_valid(&search) {
        return Err(Error::msg("This search cannot be saved"));
    }
    let id = db::insert_search(&state.db(), &title, &search)?;
    Ok(db::SavedSearch { id, title, state: search })
}

#[tauri::command]
pub fn rename_search(state: AppStateRef<'_>, id: i64, title: String) -> Result<()> {
    db::rename_search(&state.db(), id, &db::search_title(&title)?)
}

#[tauri::command]
pub fn delete_search(state: AppStateRef<'_>, id: i64) -> Result<()> {
    db::delete_search(&state.db(), id)
}

// ---------------------------------------------------------------------------
// Filter rules (rules.rs)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_rules(state: AppStateRef<'_>) -> Result<Vec<Rule>> {
    db::rules(&state.db())
}

/// Stores a new rule (id 0) or changes one, and returns it as stored.
#[tauri::command]
pub fn save_rule(state: AppStateRef<'_>, rule: Rule) -> Result<Rule> {
    let rule = rule.validated()?;
    let conn = state.db();
    let id = db::save_rule(&conn, &rule)?;
    db::rule(&conn, id)
}

#[tauri::command]
pub fn delete_rule(state: AppStateRef<'_>, id: i64) -> Result<()> {
    db::delete_rule(&state.db(), id)
}

/// Articles are looked at this many at a time, so the database is free between
/// batches for the rest of the app.
const RULE_BATCH: usize = 500;

/// The stored articles a rule (saved or not) would change now.
fn rule_matches(state: &AppState, rule: &Rule) -> Result<Vec<i64>> {
    let feeds = db::rule_feeds(&state.db(), rule)?;
    let (mut all, mut after) = (Vec::new(), 0);
    loop {
        let (matched, last) = db::rule_batch(&state.db(), rule, &feeds, after, RULE_BATCH)?;
        all.extend(matched);
        match last {
            Some(id) => after = id,
            None => return Ok(all),
        }
    }
}

/// How many stored articles a rule (saved or not) would change now.
#[tauri::command]
pub async fn count_rule_matches(state: AppStateRef<'_>, rule: Rule) -> Result<usize> {
    let rule = rule.validated()?;
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || rule_matches(&state, &rule).map(|ids| ids.len()))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
}

/// Does what a saved rule says to the stored articles it matches. Returns how many.
#[tauri::command]
pub async fn apply_rule(state: AppStateRef<'_>, id: i64) -> Result<usize> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let rule = db::rule(&state.db(), id)?;
        let ids = rule_matches(&state, &rule)?;
        let conn = state.db();
        for chunk in ids.chunks(RULE_BATCH) {
            db::apply_rule_to(&conn, &rule, chunk)?;
        }
        if rule.action == "delete" {
            db::checkpoint(&conn);
        }
        Ok(ids.len())
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?
}

/// Past briefs, newest first.
#[tauri::command]
pub fn briefs(state: AppStateRef<'_>) -> Result<Vec<db::BriefInfo>> {
    db::list_briefs(&state.db(), 50)
}

#[tauri::command]
pub fn brief(state: AppStateRef<'_>, id: i64) -> Result<Option<BriefView>> {
    brief_view(&state, id)
}

#[tauri::command]
pub fn delete_brief(state: AppStateRef<'_>, id: i64) -> Result<()> {
    let conn = state.db();
    db::delete_brief(&conn, id)?;
    db::checkpoint(&conn);
    Ok(())
}

/// What the last check that ran yt-dlp saw, kept in the database.
const YT_DLP_SEEN: &str = "yt_dlp_seen";

fn yt_dlp_seen(state: &AppState) -> Option<updates::Seen> {
    db::get_value(&state.db(), YT_DLP_SEEN).and_then(|json| serde_json::from_str(&json).ok())
}

fn remember_yt_dlp(state: &AppState, seen: Option<updates::Seen>) {
    if let Some(json) = seen.and_then(|s| serde_json::to_string(&s).ok()) {
        let _ = db::set_value(&state.db(), YT_DLP_SEEN, &json);
    }
}

/// The offline security check (updates.rs): no network. With `run` (the Security
/// section of Settings is open) it runs `yt-dlp --version`; at startup it does not
/// start any program and uses the version seen last time.
#[tauri::command]
pub async fn check_components(state: AppStateRef<'_>, run: Option<bool>) -> Result<Vec<updates::Component>> {
    let today = chrono::Local::now().date_naive();
    let (run, seen) = (run.unwrap_or(false), yt_dlp_seen(&state));
    let (list, seen) = tokio::task::spawn_blocking(move || updates::check_offline(today, run, seen))
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    remember_yt_dlp(&state, seen);
    Ok(list)
}

/// The online security check. Only when the user clicks "Check now".
#[tauri::command]
pub async fn check_components_online(state: AppStateRef<'_>) -> Result<Vec<updates::Component>> {
    let net = state.net();
    let (list, seen) = updates::check_online(&net, chrono::Local::now().date_naive()).await;
    remember_yt_dlp(&state, seen);
    Ok(list)
}

/// Finds a playable file for a PeerTube watch page. The UI plays it through the media proxy.
#[tauri::command]
pub async fn peertube_video(state: AppStateRef<'_>, url: String) -> Result<peertube::VideoFile> {
    let url = net::parse_web_url(&url)?;
    peertube::video(&state.net(), &url).await
}

/// Searches PeerTube (through Sepia Search) for a video title.
#[tauri::command]
pub async fn search_peertube(state: AppStateRef<'_>, query: String) -> Result<Vec<peertube::SearchHit>> {
    peertube::search(&state.net(), &query, state.settings().load_images).await
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
    /// Shown once at startup: the saved proxy could not be used and Tidings is offline.
    pub startup_warning: Option<String>,
}

#[tauri::command]
pub fn app_info(state: AppStateRef<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.display().to_string(),
        platform: std::env::consts::OS,
        media_base: state.media_base.get().cloned(),
        startup_warning: state.startup_warning.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}
