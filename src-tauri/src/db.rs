//! SQLite storage: schema, migrations and all queries.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub position: i64,
    pub expanded: bool,
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
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

pub fn folders(conn: &Connection) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, title, position, expanded FROM folders
         ORDER BY position, lower(title)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Folder {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            title: r.get(2)?,
            position: r.get(3)?,
            expanded: r.get(4)?,
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
    (SELECT COUNT(*) FROM articles a WHERE a.feed_id = f.id AND a.is_deleted = 0)";

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
    }
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
fn where_clause(scope: &Scope, filter: &str, query: &str, args: &mut Vec<Value>) -> String {
    let scope = scope_sql(scope, args);
    let search = search_sql(query, args);
    format!("a.is_deleted = 0 AND {scope} AND {} AND {search}", filter_sql(filter))
}

pub fn list_articles(
    conn: &Connection,
    q: &ListQuery,
    thumb: impl Fn(&str) -> String,
) -> Result<Vec<ArticleItem>> {
    let mut args = Vec::new();
    let clause = where_clause(&q.scope, &q.filter, &q.query, &mut args);
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
    let clause = where_clause(&q.scope, &q.filter, &q.query, &mut args);
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

pub fn mark_scope_read(conn: &Connection, scope: &Scope) -> Result<usize> {
    let mut args = Vec::new();
    let clause = where_clause(scope, "unread", "", &mut args);
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
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM feeds WHERE folder_id IS ?1",
        [folder_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO feeds (url, folder_id, title, site_url, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![url, folder_id, title.unwrap_or(""), site_url, position, now()],
    )?;
    Ok(conn.last_insert_rowid())
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
}

pub fn update_feed(conn: &Connection, id: i64, patch: &FeedPatch) -> Result<()> {
    let old_url: String = conn.query_row("SELECT url FROM feeds WHERE id = ?1", [id], |r| r.get(0))?;
    let title = patch.custom_title.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let retention_mode = patch
        .retention_mode
        .as_deref()
        .filter(|m| matches!(*m, "all" | "count" | "days"));
    let interval = patch.update_interval.map(|m| m.clamp(0, 7 * 24 * 60));
    let old_folder: Option<i64> =
        conn.query_row("SELECT folder_id FROM feeds WHERE id = ?1", [id], |r| r.get(0))?;
    conn.execute(
        "UPDATE feeds SET custom_title = ?2, url = ?3, update_interval = ?4, retention_mode = ?5,
            retention_value = ?6, full_text = ?7, paused = ?8
         WHERE id = ?1",
        params![
            id,
            title,
            patch.url,
            interval,
            retention_mode,
            patch.retention_value.map(|v| v.max(1)),
            patch.full_text,
            patch.paused
        ],
    )?;
    if old_url != patch.url {
        conn.execute(
            "UPDATE feeds SET etag = NULL, last_modified = NULL, last_error = NULL WHERE id = ?1",
            [id],
        )?;
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

/// Puts `id` before `before` (or at the end) in a sibling list and renumbers positions.
fn reorder(
    conn: &Connection,
    table: &str,
    parent_col: &str,
    parent: Option<i64>,
    id: i64,
    before: Option<i64>,
) -> Result<()> {
    let mut ids: Vec<i64> = {
        let sql = format!(
            "SELECT id FROM {table} WHERE {parent_col} IS ?1 AND id != ?2 ORDER BY position, id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![parent, id], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let index = before.and_then(|b| ids.iter().position(|x| *x == b)).unwrap_or(ids.len());
    ids.insert(index, id);
    let sql = format!("UPDATE {table} SET position = ?1, {parent_col} = ?2 WHERE id = ?3");
    let mut stmt = conn.prepare(&sql)?;
    for (pos, item) in ids.iter().enumerate() {
        stmt.execute(params![pos as i64, parent, item])?;
    }
    Ok(())
}

pub fn move_feed(conn: &Connection, id: i64, folder_id: Option<i64>, before: Option<i64>) -> Result<()> {
    reorder(conn, "feeds", "folder_id", folder_id, id, before)
}

pub fn create_folder(conn: &Connection, title: &str, parent_id: Option<i64>) -> Result<i64> {
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM folders WHERE parent_id IS ?1",
        [parent_id],
        |r| r.get(0),
    )?;
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

pub fn move_folder(conn: &Connection, id: i64, parent_id: Option<i64>, before: Option<i64>) -> Result<()> {
    if let Some(parent) = parent_id {
        if folder_subtree(conn, id)?.contains(&parent) {
            return Err(Error::msg("A folder cannot be moved into itself"));
        }
    }
    reorder(conn, "folders", "parent_id", parent_id, id, before)
}
