//! SQLite storage: schema, migrations and all queries.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::feed_types;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only the current user may read the database.
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA secure_delete = ON;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE folders (
                id        INTEGER PRIMARY KEY,
                parent_id INTEGER REFERENCES folders(id) ON DELETE CASCADE,
                title     TEXT NOT NULL,
                position  INTEGER NOT NULL DEFAULT 0,
                expanded  INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE feeds (
                id              INTEGER PRIMARY KEY,
                folder_id       INTEGER REFERENCES folders(id) ON DELETE SET NULL,
                url             TEXT NOT NULL UNIQUE,
                title           TEXT NOT NULL DEFAULT '',
                custom_title    TEXT,
                site_url        TEXT,
                description     TEXT,
                position        INTEGER NOT NULL DEFAULT 0,
                icon            BLOB,
                icon_mime       TEXT,
                icon_version    INTEGER,
                icon_checked    INTEGER,
                update_interval INTEGER,
                retention_mode  TEXT,
                retention_value INTEGER,
                full_text       INTEGER NOT NULL DEFAULT 0,
                paused          INTEGER NOT NULL DEFAULT 0,
                etag            TEXT,
                last_modified   TEXT,
                last_fetched    INTEGER,
                last_success    INTEGER,
                last_error      TEXT,
                created_at      INTEGER NOT NULL
            );
            CREATE TABLE articles (
                id             INTEGER PRIMARY KEY,
                feed_id        INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
                guid           TEXT NOT NULL,
                url            TEXT,
                title          TEXT NOT NULL DEFAULT '',
                author         TEXT,
                excerpt        TEXT,
                content        TEXT,
                full_content   TEXT,
                image_url      TEXT,
                enclosure_url  TEXT,
                enclosure_type TEXT,
                published      INTEGER NOT NULL,
                fetched_at     INTEGER NOT NULL,
                last_seen      INTEGER NOT NULL,
                content_hash   INTEGER,
                is_read        INTEGER NOT NULL DEFAULT 0,
                is_starred     INTEGER NOT NULL DEFAULT 0,
                is_new         INTEGER NOT NULL DEFAULT 1,
                is_deleted     INTEGER NOT NULL DEFAULT 0,
                UNIQUE(feed_id, guid)
            );
            CREATE INDEX idx_articles_feed ON articles(feed_id, is_deleted, published DESC);
            CREATE INDEX idx_articles_published ON articles(is_deleted, published DESC);
            CREATE INDEX idx_articles_unread ON articles(feed_id) WHERE is_read = 0 AND is_deleted = 0;
            CREATE INDEX idx_articles_starred ON articles(published DESC) WHERE is_starred = 1;
            CREATE TABLE settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 1;
            COMMIT;",
        )?;
    }
    if version < 2 {
        // v2: favorites, and one shared order for feeds and folders in the same parent.
        conn.execute_batch(
            "BEGIN;
            ALTER TABLE feeds ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE folders ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
            COMMIT;",
        )?;
        normalize_positions(conn)?;
        conn.execute_batch("PRAGMA user_version = 2;")?;
    }
    if version < 3 {
        // v3: feed types (podcast, video, ...). `type_override` is the user's choice,
        // NULL for automatic; `detected_type` is filled in from what is already stored.
        // One transaction, so an interrupted upgrade leaves the database at v2.
        conn.execute_batch(
            "BEGIN;
            ALTER TABLE feeds ADD COLUMN type_override TEXT;
            ALTER TABLE feeds ADD COLUMN detected_type TEXT NOT NULL DEFAULT 'article';",
        )?;
        let backfill = (|| -> Result<()> {
            let ids: Vec<i64> = conn
                .prepare("SELECT id FROM feeds")?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            for id in ids {
                detect_feed_type(conn, id)?;
            }
            Ok(())
        })();
        match backfill {
            Ok(()) => conn.execute_batch("PRAGMA user_version = 3; COMMIT;")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Works out a feed's type from its address and the enclosures of its recent
/// items, and stores it. Run after each refresh and when the address changes.
pub fn detect_feed_type(conn: &Connection, feed_id: i64) -> Result<()> {
    let (url, site_url): (String, Option<String>) = conn.query_row(
        "SELECT url, site_url FROM feeds WHERE id = ?1",
        [feed_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let enclosures: Vec<(Option<String>, Option<String>)> = conn
        .prepare(
            "SELECT enclosure_type, enclosure_url FROM articles
             WHERE feed_id = ?1 AND is_deleted = 0 ORDER BY published DESC LIMIT ?2",
        )?
        .query_map(params![feed_id, feed_types::RECENT_ITEMS], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let detected = feed_types::detect(&url, site_url.as_deref(), &enclosures);
    conn.execute("UPDATE feeds SET detected_type = ?2 WHERE id = ?1", params![feed_id, detected])?;
    Ok(())
}

/// Gives every parent a single 0..n order, folders first, then feeds,
/// so the tree looks the same as before the shared order existed.
pub fn normalize_positions(conn: &Connection) -> Result<()> {
    let parents: Vec<Option<i64>> = {
        let mut stmt = conn.prepare(
            "SELECT parent_id FROM folders UNION SELECT folder_id FROM feeds",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for parent in parents {
        let items = children(conn, parent, true)?;
        renumber(conn, &items)?;
    }
    Ok(())
}

/// Articles marked "new" belong to the previous session. Clear them at startup.
pub fn clear_new_flags(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE articles SET is_new = 0 WHERE is_new = 1", [])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Models sent to the UI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub position: i64,
    pub expanded: bool,
    /// When it was added to Favorites (Unix time), 0 if it is not a favorite.
    pub pinned: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub id: i64,
    pub folder_id: Option<i64>,
    /// Display title: custom title, else feed title, else URL.
    pub title: String,
    pub feed_title: String,
    pub custom_title: Option<String>,
    pub url: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub position: i64,
    pub icon_version: Option<i64>,
    pub update_interval: Option<i64>,
    pub retention_mode: Option<String>,
    pub retention_value: Option<i64>,
    pub full_text: bool,
    pub paused: bool,
    pub last_fetched: Option<i64>,
    pub last_success: Option<i64>,
    pub last_error: Option<String>,
    pub unread: i64,
    pub total: i64,
    /// When it was added to Favorites (Unix time), 0 if it is not a favorite.
    pub pinned: i64,
    /// The type in use: the user's choice, else the detected one.
    pub feed_type: String,
    /// The user's choice, `None` for automatic.
    pub type_override: Option<String>,
    pub detected_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub folders: Vec<Folder>,
    pub feeds: Vec<Feed>,
    pub starred: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// (feed id, unread count)
    pub feeds: Vec<(i64, i64)>,
    pub starred: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleItem {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub excerpt: Option<String>,
    pub published: i64,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_new: bool,
    pub has_enclosure: bool,
    /// Proxied thumbnail URL, only set when remote images are enabled.
    pub thumb: Option<String>,
}

#[derive(Debug)]
pub struct ArticleRow {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub content: Option<String>,
    pub full_content: Option<String>,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub published: i64,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_new: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    All,
    Starred,
    Feed { id: i64 },
    Folder { id: i64 },
    /// Every feed of one type (podcast, video, ...).
    Type {
        #[serde(rename = "type")]
        feed_type: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub scope: Scope,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub oldest_first: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    #[serde(flatten)]
    pub range: DateRange,
    /// Only feeds of this type inside the scope (the type icons above the list).
    #[serde(default)]
    pub feed_type: Option<String>,
}

/// Published between `since` (inclusive) and `until` (exclusive), Unix seconds.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub until: Option<i64>,
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

pub fn folders(conn: &Connection) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, title, position, expanded, pinned FROM folders
         ORDER BY position, lower(title)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Folder {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            title: r.get(2)?,
            position: r.get(3)?,
            expanded: r.get(4)?,
            pinned: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

const FEED_COLUMNS: &str = "
    f.id, f.folder_id,
    COALESCE(NULLIF(f.custom_title, ''), NULLIF(f.title, ''), f.url),
    f.title, f.custom_title, f.url, f.site_url, f.description, f.position,
    f.icon_version, f.update_interval, f.retention_mode, f.retention_value,
    f.full_text, f.paused, f.last_fetched, f.last_success, f.last_error,
    (SELECT COUNT(*) FROM articles a WHERE a.feed_id = f.id AND a.is_read = 0 AND a.is_deleted = 0),
    (SELECT COUNT(*) FROM articles a WHERE a.feed_id = f.id AND a.is_deleted = 0),
    f.pinned, COALESCE(f.type_override, f.detected_type), f.type_override, f.detected_type";

fn feed_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        title: r.get(2)?,
        feed_title: r.get(3)?,
        custom_title: r.get(4)?,
        url: r.get(5)?,
        site_url: r.get(6)?,
        description: r.get(7)?,
        position: r.get(8)?,
        icon_version: r.get(9)?,
        update_interval: r.get(10)?,
        retention_mode: r.get(11)?,
        retention_value: r.get(12)?,
        full_text: r.get(13)?,
        paused: r.get(14)?,
        last_fetched: r.get(15)?,
        last_success: r.get(16)?,
        last_error: r.get(17)?,
        unread: r.get(18)?,
        total: r.get(19)?,
        pinned: r.get(20)?,
        feed_type: r.get(21)?,
        type_override: r.get(22)?,
        detected_type: r.get(23)?,
    })
}

pub fn feeds(conn: &Connection) -> Result<Vec<Feed>> {
    let sql = format!("SELECT {FEED_COLUMNS} FROM feeds f ORDER BY f.position, lower(f.title)");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], feed_from_row)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn feed(conn: &Connection, id: i64) -> Result<Feed> {
    let sql = format!("SELECT {FEED_COLUMNS} FROM feeds f WHERE f.id = ?1");
    conn.query_row(&sql, [id], feed_from_row)
        .optional()?
        .ok_or_else(|| Error::msg("Feed not found"))
}

pub fn tree(conn: &Connection) -> Result<Tree> {
    Ok(Tree { folders: folders(conn)?, feeds: feeds(conn)?, starred: starred_count(conn)? })
}

fn starred_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE is_starred = 1 AND is_deleted = 0",
        [],
        |r| r.get(0),
    )?)
}

pub fn counts(conn: &Connection) -> Result<Counts> {
    let mut stmt = conn.prepare(
        "SELECT feed_id, COUNT(*) FROM articles
         WHERE is_read = 0 AND is_deleted = 0 GROUP BY feed_id",
    )?;
    let feeds = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(Counts { feeds, starred: starred_count(conn)? })
}

// ---------------------------------------------------------------------------
// Article queries
// ---------------------------------------------------------------------------

const FOLDER_FEEDS_SQL: &str = "SELECT fe.id FROM feeds fe WHERE fe.folder_id IN (
    WITH RECURSIVE sub(id) AS (
        SELECT ? UNION ALL SELECT fo.id FROM folders fo JOIN sub ON fo.parent_id = sub.id
    ) SELECT id FROM sub)";

fn scope_sql(scope: &Scope, args: &mut Vec<Value>) -> String {
    match scope {
        Scope::All => "1 = 1".into(),
        Scope::Starred => "a.is_starred = 1".into(),
        Scope::Feed { id } => {
            args.push(Value::Integer(*id));
            "a.feed_id = ?".into()
        }
        Scope::Folder { id } => {
            args.push(Value::Integer(*id));
            format!("a.feed_id IN ({FOLDER_FEEDS_SQL})")
        }
        Scope::Type { feed_type } => type_sql(Some(feed_type), args),
    }
}

/// Articles of feeds of one type: the user's choice for the feed, else the detected one.
fn type_sql(feed_type: Option<&str>, args: &mut Vec<Value>) -> String {
    match feed_type {
        None => "1 = 1".into(),
        Some(t) if feed_types::is_type(t) => {
            args.push(Value::Text(t.to_string()));
            "a.feed_id IN (SELECT fe.id FROM feeds fe WHERE COALESCE(fe.type_override, fe.detected_type) = ?)".into()
        }
        Some(_) => "1 = 0".into(),
    }
}

fn range_sql(range: &DateRange, args: &mut Vec<Value>) -> String {
    let mut parts = vec!["1 = 1"];
    if let Some(since) = range.since {
        args.push(Value::Integer(since));
        parts.push("a.published >= ?");
    }
    if let Some(until) = range.until {
        args.push(Value::Integer(until));
        parts.push("a.published < ?");
    }
    parts.join(" AND ")
}

fn filter_sql(filter: &str) -> &'static str {
    match filter {
        "unread" => "a.is_read = 0",
        "new" => "a.is_new = 1",
        "starred" => "a.is_starred = 1",
        _ => "1 = 1",
    }
}

fn search_sql(query: &str, args: &mut Vec<Value>) -> String {
    let mut parts = Vec::new();
    for word in query.split_whitespace().take(8) {
        let escaped = word.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        for _ in 0..3 {
            args.push(Value::Text(pattern.clone()));
        }
        parts.push(
            "(a.title LIKE ? ESCAPE '\\' OR a.excerpt LIKE ? ESCAPE '\\' OR a.author LIKE ? ESCAPE '\\')",
        );
    }
    if parts.is_empty() { "1 = 1".into() } else { parts.join(" AND ") }
}

/// Builds the WHERE clause shared by list and "mark all read" queries.
/// The one place that turns the list's filters into SQL, so the list, its ID list and
/// "mark all as read" always cover the same articles. `args` are pushed in the order
/// the parts appear in the result.
fn where_clause(
    scope: &Scope,
    filter: &str,
    query: &str,
    range: &DateRange,
    feed_type: Option<&str>,
    args: &mut Vec<Value>,
) -> String {
    let scope = scope_sql(scope, args);
    let search = search_sql(query, args);
    let range = range_sql(range, args);
    let types = type_sql(feed_type, args);
    format!("a.is_deleted = 0 AND {scope} AND {} AND {search} AND {range} AND {types}", filter_sql(filter))
}

pub fn list_articles(
    conn: &Connection,
    q: &ListQuery,
    thumb: impl Fn(&str) -> String,
) -> Result<Vec<ArticleItem>> {
    let mut args = Vec::new();
    let clause = where_clause(&q.scope, &q.filter, &q.query, &q.range, q.feed_type.as_deref(), &mut args);
    let order = if q.oldest_first { "ASC" } else { "DESC" };
    args.push(Value::Integer(q.limit.unwrap_or(200).min(1000) as i64));
    args.push(Value::Integer(q.offset.unwrap_or(0) as i64));
    let sql = format!(
        "SELECT a.id, a.feed_id, a.title, a.url, a.author, a.excerpt, a.published,
                a.is_read, a.is_starred, a.is_new, a.enclosure_url IS NOT NULL, a.image_url
         FROM articles a WHERE {clause}
         ORDER BY a.published {order}, a.id {order} LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), |r| {
        let image: Option<String> = r.get(11)?;
        Ok(ArticleItem {
            id: r.get(0)?,
            feed_id: r.get(1)?,
            title: r.get(2)?,
            url: r.get(3)?,
            author: r.get(4)?,
            excerpt: r.get(5)?,
            published: r.get(6)?,
            is_read: r.get(7)?,
            is_starred: r.get(8)?,
            is_new: r.get(9)?,
            has_enclosure: r.get(10)?,
            thumb: image.map(|u| thumb(&u)),
        })
    })?;
    let mut items: Vec<ArticleItem> = rows.collect::<std::result::Result<_, _>>()?;
    for item in &mut items {
        if item.thumb.as_deref() == Some("") {
            item.thumb = None;
        }
    }
    Ok(items)
}

pub fn list_article_ids(conn: &Connection, q: &ListQuery) -> Result<Vec<i64>> {
    let mut args = Vec::new();
    let clause = where_clause(&q.scope, &q.filter, &q.query, &q.range, q.feed_type.as_deref(), &mut args);
    let order = if q.oldest_first { "ASC" } else { "DESC" };
    args.push(Value::Integer(q.limit.unwrap_or(20).min(100) as i64));
    args.push(Value::Integer(q.offset.unwrap_or(0) as i64));
    let sql = format!(
        "SELECT a.id FROM articles a WHERE {clause}
         ORDER BY a.published {order}, a.id {order} LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn article(conn: &Connection, id: i64) -> Result<ArticleRow> {
    conn.query_row(
        "SELECT id, feed_id, title, url, author, content, full_content,
                enclosure_url, enclosure_type, published, is_read, is_starred, is_new
         FROM articles WHERE id = ?1 AND is_deleted = 0",
        [id],
        |r| {
            Ok(ArticleRow {
                id: r.get(0)?,
                feed_id: r.get(1)?,
                title: r.get(2)?,
                url: r.get(3)?,
                author: r.get(4)?,
                content: r.get(5)?,
                full_content: r.get(6)?,
                enclosure_url: r.get(7)?,
                enclosure_type: r.get(8)?,
                published: r.get(9)?,
                is_read: r.get(10)?,
                is_starred: r.get(11)?,
                is_new: r.get(12)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::msg("Article not found"))
}

pub fn save_full_content(conn: &Connection, id: i64, html: &str) -> Result<()> {
    conn.execute("UPDATE articles SET full_content = ?1 WHERE id = ?2", params![html, id])?;
    Ok(())
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

pub fn set_read(conn: &Connection, ids: &[i64], read: bool) -> Result<()> {
    for chunk in ids.chunks(500) {
        let sql = format!(
            "UPDATE articles SET is_read = ?, is_new = CASE WHEN ? THEN 0 ELSE is_new END
             WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut args: Vec<Value> = vec![Value::Integer(read as i64), Value::Integer(read as i64)];
        args.extend(chunk.iter().map(|id| Value::Integer(*id)));
        conn.execute(&sql, params_from_iter(args))?;
    }
    Ok(())
}

pub fn set_starred(conn: &Connection, id: i64, starred: bool) -> Result<()> {
    conn.execute("UPDATE articles SET is_starred = ?1 WHERE id = ?2", params![starred, id])?;
    Ok(())
}

pub fn mark_scope_read(conn: &Connection, scope: &Scope, range: &DateRange, feed_type: Option<&str>) -> Result<usize> {
    let mut args = Vec::new();
    let clause = where_clause(scope, "unread", "", range, feed_type, &mut args);
    let sql = format!(
        "UPDATE articles SET is_read = 1, is_new = 0
         WHERE id IN (SELECT a.id FROM articles a WHERE {clause})"
    );
    Ok(conn.execute(&sql, params_from_iter(args))?)
}

/// Deleted articles keep their guid as a tombstone so a refresh does not bring them back.
pub fn delete_articles(conn: &Connection, ids: &[i64]) -> Result<()> {
    for chunk in ids.chunks(500) {
        let sql = format!(
            "UPDATE articles SET is_deleted = 1, is_starred = 0, is_read = 1, is_new = 0,
                content = NULL, full_content = NULL, excerpt = NULL, image_url = NULL
             WHERE id IN ({})",
            placeholders(chunk.len())
        );
        conn.execute(&sql, params_from_iter(chunk.iter()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Refresh: storing fetched articles
// ---------------------------------------------------------------------------

pub struct FetchInfo {
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub site_url: Option<String>,
    pub has_icon: bool,
    pub icon_checked: Option<i64>,
    pub retention_mode: Option<String>,
    pub retention_value: Option<i64>,
}

/// The address of every subscribed feed.
pub fn feed_urls(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT url FROM feeds")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn fetch_info(conn: &Connection, id: i64) -> Result<FetchInfo> {
    conn.query_row(
        "SELECT url, etag, last_modified, site_url, icon IS NOT NULL, icon_checked,
                retention_mode, retention_value
         FROM feeds WHERE id = ?1",
        [id],
        |r| {
            Ok(FetchInfo {
                url: r.get(0)?,
                etag: r.get(1)?,
                last_modified: r.get(2)?,
                site_url: r.get(3)?,
                has_icon: r.get(4)?,
                icon_checked: r.get(5)?,
                retention_mode: r.get(6)?,
                retention_value: r.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::msg("Feed not found"))
}

pub struct NewArticle {
    pub guid: String,
    pub url: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub excerpt: String,
    pub content: String,
    pub image_url: Option<String>,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub published: Option<i64>,
    pub hash: i64,
}

pub struct FeedMeta {
    pub title: Option<String>,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// Inserts new articles and updates changed ones. Returns the number of new articles.
/// Articles published before `cutoff` are not inserted (they would be removed right away).
pub fn store_articles(
    tx: &Transaction<'_>,
    feed_id: i64,
    items: &[NewArticle],
    cutoff: Option<i64>,
) -> Result<usize> {
    let now = now();
    let mut find = tx.prepare_cached(
        "SELECT id, is_deleted, content_hash FROM articles WHERE feed_id = ?1 AND guid = ?2",
    )?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO articles (feed_id, guid, url, title, author, excerpt, content, image_url,
            enclosure_url, enclosure_type, published, fetched_at, last_seen, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13)",
    )?;
    let mut update = tx.prepare_cached(
        "UPDATE articles SET url = ?2, title = ?3, author = ?4, excerpt = ?5, content = ?6,
            image_url = ?7, enclosure_url = ?8, enclosure_type = ?9, content_hash = ?10,
            full_content = NULL, last_seen = ?11
         WHERE id = ?1",
    )?;
    let mut touch = tx.prepare_cached("UPDATE articles SET last_seen = ?2 WHERE id = ?1")?;

    let mut added = 0;
    for item in items {
        let existing: Option<(i64, bool, Option<i64>)> = find
            .query_row(params![feed_id, item.guid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        match existing {
            None => {
                let published = item.published.unwrap_or(now).min(now + 3600);
                if cutoff.is_some_and(|c| published < c) {
                    continue;
                }
                insert.execute(params![
                    feed_id,
                    item.guid,
                    item.url,
                    item.title,
                    item.author,
                    item.excerpt,
                    item.content,
                    item.image_url,
                    item.enclosure_url,
                    item.enclosure_type,
                    published.min(now),
                    now,
                    item.hash
                ])?;
                added += 1;
            }
            Some((id, false, hash)) if hash != Some(item.hash) => {
                update.execute(params![
                    id,
                    item.url,
                    item.title,
                    item.author,
                    item.excerpt,
                    item.content,
                    item.image_url,
                    item.enclosure_url,
                    item.enclosure_type,
                    item.hash,
                    now
                ])?;
            }
            Some((id, _, _)) => {
                touch.execute(params![id, now])?;
            }
        }
    }
    Ok(added)
}

pub fn update_feed_meta(tx: &Transaction<'_>, id: i64, meta: &FeedMeta) -> Result<()> {
    let now = now();
    tx.execute(
        "UPDATE feeds SET title = COALESCE(?2, title), site_url = COALESCE(?3, site_url),
            description = COALESCE(?4, description), etag = ?5, last_modified = ?6,
            last_fetched = ?7, last_success = ?7, last_error = NULL
         WHERE id = ?1",
        params![id, meta.title, meta.site_url, meta.description, meta.etag, meta.last_modified, now],
    )?;
    Ok(())
}

pub fn set_not_modified(conn: &Connection, id: i64) -> Result<()> {
    let now = now();
    conn.execute(
        "UPDATE feeds SET last_fetched = ?2, last_success = ?2, last_error = NULL WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

pub fn set_fetch_error(conn: &Connection, id: i64, message: &str) -> Result<()> {
    conn.execute(
        "UPDATE feeds SET last_fetched = ?2, last_error = ?3 WHERE id = ?1",
        params![id, now(), message],
    )?;
    Ok(())
}

pub fn set_icon(conn: &Connection, id: i64, icon: Option<(&[u8], &str)>) -> Result<()> {
    let now = now();
    match icon {
        Some((bytes, mime)) => conn.execute(
            "UPDATE feeds SET icon = ?2, icon_mime = ?3, icon_version = ?4, icon_checked = ?4
             WHERE id = ?1",
            params![id, bytes, mime, now],
        )?,
        None => conn.execute("UPDATE feeds SET icon_checked = ?2 WHERE id = ?1", params![id, now])?,
    };
    Ok(())
}

pub fn icon(conn: &Connection, id: i64) -> Result<Option<(Vec<u8>, String)>> {
    Ok(conn
        .query_row(
            "SELECT icon, icon_mime FROM feeds WHERE id = ?1 AND icon IS NOT NULL",
            [id],
            |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "image/png".into()))),
        )
        .optional()?)
}

/// Applies the archive policy to one feed. Starred articles are always kept.
pub fn apply_retention(conn: &Connection, feed_id: i64, mode: &str, value: i64) -> Result<()> {
    const CLEAR: &str = "is_deleted = 1, is_new = 0, content = NULL, full_content = NULL,
                         excerpt = NULL, image_url = NULL";
    match mode {
        "count" => {
            conn.execute(
                &format!(
                    "UPDATE articles SET {CLEAR} WHERE id IN (
                        SELECT id FROM articles
                        WHERE feed_id = ?1 AND is_deleted = 0 AND is_starred = 0
                        ORDER BY published DESC, id DESC LIMIT -1 OFFSET ?2)"
                ),
                params![feed_id, value.max(1)],
            )?;
        }
        "days" => {
            let cutoff = now() - value.max(1) * 86_400;
            conn.execute(
                &format!(
                    "UPDATE articles SET {CLEAR}
                     WHERE feed_id = ?1 AND is_deleted = 0 AND is_starred = 0 AND published < ?2"
                ),
                params![feed_id, cutoff],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Removes tombstones of articles that have not appeared in their feed for 60 days.
pub fn purge_tombstones(conn: &Connection) -> Result<()> {
    let cutoff = now() - 60 * 86_400;
    conn.execute("DELETE FROM articles WHERE is_deleted = 1 AND last_seen < ?1", [cutoff])?;
    Ok(())
}

pub struct Schedule {
    pub id: i64,
    pub update_interval: Option<i64>,
    pub last_fetched: Option<i64>,
    pub paused: bool,
}

pub fn schedules(conn: &Connection) -> Result<Vec<Schedule>> {
    let mut stmt = conn.prepare("SELECT id, update_interval, last_fetched, paused FROM feeds")?;
    let rows = stmt.query_map([], |r| {
        Ok(Schedule {
            id: r.get(0)?,
            update_interval: r.get(1)?,
            last_fetched: r.get(2)?,
            paused: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn feed_retention(conn: &Connection) -> Result<Vec<(i64, Option<String>, Option<i64>)>> {
    let mut stmt = conn.prepare("SELECT id, retention_mode, retention_value FROM feeds")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------------------------------------------------------------------------
// Feed and folder management
// ---------------------------------------------------------------------------

pub fn feed_id_by_url(conn: &Connection, url: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT id FROM feeds WHERE url = ?1", [url], |r| r.get(0))
        .optional()?)
}

pub fn insert_feed(
    conn: &Connection,
    url: &str,
    folder_id: Option<i64>,
    title: Option<&str>,
    site_url: Option<&str>,
) -> Result<i64> {
    let position = next_position(conn, folder_id)?;
    conn.execute(
        "INSERT INTO feeds (url, folder_id, title, site_url, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![url, folder_id, title.unwrap_or(""), site_url, position, now()],
    )?;
    let id = conn.last_insert_rowid();
    // From the address alone for now; the first refresh looks at the items too.
    detect_feed_type(conn, id)?;
    Ok(id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPatch {
    pub custom_title: Option<String>,
    pub url: String,
    pub folder_id: Option<i64>,
    pub update_interval: Option<i64>,
    pub retention_mode: Option<String>,
    pub retention_value: Option<i64>,
    pub full_text: bool,
    pub paused: bool,
    /// `None` for automatic.
    #[serde(default)]
    pub type_override: Option<String>,
}

pub fn update_feed(conn: &Connection, id: i64, patch: &FeedPatch) -> Result<()> {
    let old_url: String = conn.query_row("SELECT url FROM feeds WHERE id = ?1", [id], |r| r.get(0))?;
    let title = patch.custom_title.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let retention_mode = patch
        .retention_mode
        .as_deref()
        .filter(|m| matches!(*m, "all" | "count" | "days"));
    let interval = patch.update_interval.map(|m| m.clamp(0, 7 * 24 * 60));
    let type_override = patch.type_override.as_deref().filter(|t| feed_types::is_type(t));
    let old_folder: Option<i64> =
        conn.query_row("SELECT folder_id FROM feeds WHERE id = ?1", [id], |r| r.get(0))?;
    conn.execute(
        "UPDATE feeds SET custom_title = ?2, url = ?3, update_interval = ?4, retention_mode = ?5,
            retention_value = ?6, full_text = ?7, paused = ?8, type_override = ?9
         WHERE id = ?1",
        params![
            id,
            title,
            patch.url,
            interval,
            retention_mode,
            patch.retention_value.map(|v| v.max(1)),
            patch.full_text,
            patch.paused,
            type_override
        ],
    )?;
    if old_url != patch.url {
        conn.execute(
            "UPDATE feeds SET etag = NULL, last_modified = NULL, last_error = NULL WHERE id = ?1",
            [id],
        )?;
        detect_feed_type(conn, id)?;
    }
    if old_folder != patch.folder_id {
        move_feed(conn, id, patch.folder_id, None)?;
    }
    Ok(())
}

pub fn delete_feed(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM feeds WHERE id = ?1", [id])?;
    Ok(())
}

/// A feed or a folder in the subscription tree. Both share one order per parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Folder { id: i64 },
    Feed { id: i64 },
}

fn next_position(conn: &Connection, parent: Option<i64>) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(p) + 1, 0) FROM (
            SELECT position AS p FROM folders WHERE parent_id IS ?1
            UNION ALL SELECT position FROM feeds WHERE folder_id IS ?1)",
        [parent],
        |r| r.get(0),
    )?)
}

/// The direct children of `parent` in display order.
fn children(conn: &Connection, parent: Option<i64>, folders_first: bool) -> Result<Vec<Item>> {
    let order = if folders_first { "kind, position, t" } else { "position, kind, t" };
    let sql = format!(
        "SELECT kind, id FROM (
            SELECT 0 AS kind, id, position, lower(title) AS t FROM folders WHERE parent_id IS ?1
            UNION ALL
            SELECT 1, id, position, lower(COALESCE(NULLIF(custom_title, ''), title)) FROM feeds
            WHERE folder_id IS ?1)
         ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([parent], |r| {
        let kind: i64 = r.get(0)?;
        let id: i64 = r.get(1)?;
        Ok(if kind == 0 { Item::Folder { id } } else { Item::Feed { id } })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn renumber(conn: &Connection, items: &[Item]) -> Result<()> {
    let mut folder = conn.prepare_cached("UPDATE folders SET position = ?1 WHERE id = ?2")?;
    let mut feed = conn.prepare_cached("UPDATE feeds SET position = ?1 WHERE id = ?2")?;
    for (pos, item) in items.iter().enumerate() {
        match item {
            Item::Folder { id } => folder.execute(params![pos as i64, id])?,
            Item::Feed { id } => feed.execute(params![pos as i64, id])?,
        };
    }
    Ok(())
}

/// Moves a feed or folder into `parent` (None = top level), placed before `before`
/// or at the end, and renumbers that parent's children.
pub fn move_item(conn: &Connection, item: Item, parent: Option<i64>, before: Option<Item>) -> Result<()> {
    match item {
        Item::Folder { id } => {
            if let Some(p) = parent {
                if folder_subtree(conn, id)?.contains(&p) {
                    return Err(Error::msg("A folder cannot be moved into itself"));
                }
            }
            conn.execute("UPDATE folders SET parent_id = ?2 WHERE id = ?1", params![id, parent])?;
        }
        Item::Feed { id } => {
            conn.execute("UPDATE feeds SET folder_id = ?2 WHERE id = ?1", params![id, parent])?;
        }
    }
    let mut list = children(conn, parent, false)?;
    list.retain(|x| *x != item);
    let index = before.and_then(|b| list.iter().position(|x| *x == b)).unwrap_or(list.len());
    list.insert(index, item);
    renumber(conn, &list)
}

pub fn move_feed(conn: &Connection, id: i64, folder_id: Option<i64>, before: Option<Item>) -> Result<()> {
    move_item(conn, Item::Feed { id }, folder_id, before)
}

/// Adds to or removes from Favorites.
pub fn set_pinned(conn: &Connection, item: Item, pinned: bool) -> Result<()> {
    let value = if pinned { now() } else { 0 };
    match item {
        Item::Folder { id } => conn.execute("UPDATE folders SET pinned = ?2 WHERE id = ?1", params![id, value])?,
        Item::Feed { id } => conn.execute("UPDATE feeds SET pinned = ?2 WHERE id = ?1", params![id, value])?,
    };
    Ok(())
}

pub fn create_folder(conn: &Connection, title: &str, parent_id: Option<i64>) -> Result<i64> {
    let position = next_position(conn, parent_id)?;
    conn.execute(
        "INSERT INTO folders (title, parent_id, position) VALUES (?1, ?2, ?3)",
        params![title, parent_id, position],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_or_create_folder(conn: &Connection, title: &str, parent_id: Option<i64>) -> Result<(i64, bool)> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM folders WHERE parent_id IS ?1 AND title = ?2",
            params![parent_id, title],
            |r| r.get(0),
        )
        .optional()?;
    match existing {
        Some(id) => Ok((id, false)),
        None => Ok((create_folder(conn, title, parent_id)?, true)),
    }
}

pub fn rename_folder(conn: &Connection, id: i64, title: &str) -> Result<()> {
    conn.execute("UPDATE folders SET title = ?2 WHERE id = ?1", params![id, title])?;
    Ok(())
}

pub fn set_folder_expanded(conn: &Connection, id: i64, expanded: bool) -> Result<()> {
    conn.execute("UPDATE folders SET expanded = ?2 WHERE id = ?1", params![id, expanded])?;
    Ok(())
}

/// Opens or closes many folders at once: the given ones, or all when `ids` is None.
pub fn set_folders_expanded(conn: &Connection, ids: Option<&[i64]>, expanded: bool) -> Result<()> {
    match ids {
        None => {
            conn.execute("UPDATE folders SET expanded = ?1", [expanded])?;
        }
        Some(ids) => {
            for chunk in ids.chunks(500) {
                // `?1` is the new state; the bare `?`s that follow are numbered 2, 3, ...
                let sql = format!("UPDATE folders SET expanded = ?1 WHERE id IN ({})", placeholders(chunk.len()));
                conn.execute(&sql, params_from_iter(std::iter::once(expanded as i64).chain(chunk.iter().copied())))?;
            }
        }
    }
    Ok(())
}

pub fn folder_subtree(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE sub(id) AS (
            SELECT ?1 UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
         ) SELECT id FROM sub",
    )?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn feeds_in_folder(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(FOLDER_FEEDS_SQL)?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Deletes a folder, its subfolders and every feed inside them.
pub fn delete_folder(conn: &Connection, id: i64) -> Result<()> {
    let feeds = feeds_in_folder(conn, id)?;
    for chunk in feeds.chunks(500) {
        let sql = format!("DELETE FROM feeds WHERE id IN ({})", placeholders(chunk.len()));
        conn.execute(&sql, params_from_iter(chunk.iter()))?;
    }
    conn.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    Ok(())
}

/// Deletes several feeds and folders (each folder with everything inside it) in one
/// transaction. Items that no longer exist are skipped. Returns how many feeds went.
pub fn delete_items(conn: &Connection, items: &[Item]) -> Result<usize> {
    let count = |c: &Connection| c.query_row("SELECT COUNT(*) FROM feeds", [], |r| r.get::<_, i64>(0));
    let tx = conn.unchecked_transaction()?;
    let before = count(&tx)?;
    for item in items {
        match *item {
            Item::Feed { id } => delete_feed(&tx, id)?,
            Item::Folder { id } => delete_folder(&tx, id)?,
        }
    }
    let after = count(&tx)?;
    tx.commit()?;
    Ok((before - after) as usize)
}

/// Removes every feed and folder, and with the feeds every article (to start over).
pub fn delete_all_subscriptions(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM feeds", [])?;
    tx.execute("DELETE FROM folders", [])?;
    tx.commit()?;
    Ok(())
}

pub fn move_folder(conn: &Connection, id: i64, parent_id: Option<i64>, before: Option<Item>) -> Result<()> {
    move_item(conn, Item::Folder { id }, parent_id, before)
}
