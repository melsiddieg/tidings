//! SQLite storage: schema, migrations and all queries.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::feed_types;
use crate::language;
use crate::rules::{self, Rule};

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
    // Marks the file as Tidings' database, so a copy of it can be restored (backup.rs).
    let id: i32 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    if id != crate::backup::APPLICATION_ID {
        conn.pragma_update(None, "application_id", crate::backup::APPLICATION_ID)?;
    }
    Ok(conn)
}

/// The version the last migration in `migrate` sets. A backup of a newer version is
/// refused (backup.rs). A migration that changes what stored values mean, rather
/// than adding tables or columns, must also raise `backup::OLDEST_VERSION`.
pub const SCHEMA_VERSION: i64 = 10;

/// 10 November 2026, when 0.9.2 came out: since then a feed on the local network
/// is added only after the user allowed its address.
const CONSENT_SINCE: i64 = 1_794_268_800;

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
    if version < 4 {
        // v4: AI briefs (brief.rs). `query` is the article filter a brief was written
        // from (JSON, to write it again); `body` is the brief itself (JSON).
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS briefs (
                id         INTEGER PRIMARY KEY,
                created_at INTEGER NOT NULL,
                title      TEXT NOT NULL,
                model      TEXT NOT NULL,
                query      TEXT NOT NULL,
                body       TEXT NOT NULL
            );
            PRAGMA user_version = 4;
            COMMIT;",
        )?;
    }
    if version < 5 {
        // v5: `local` says whether a feed may be fetched from a private network: 1 when
        // its host was private at the first fetch after it was added (or after this
        // upgrade), 0 when it was public, NULL until then. A public feed whose DNS later
        // points into the local network is then not followed there.
        conn.execute_batch(
            "BEGIN;
            ALTER TABLE feeds ADD COLUMN local INTEGER;
            PRAGMA user_version = 5;
            COMMIT;",
        )?;
    }
    if version < 6 {
        // v6: `local` now means "the user allowed this feed on the local network", and
        // DNS no longer decides it. Feeds whose address is itself private (an IP such
        // as 192.168.1.5, or a local name such as nas.local) keep working: the user
        // typed that address. A feed with a public-looking name loses the flag, also
        // when 0.9.1 set it because the name resolved to a private address; the user
        // can allow it again in the feed's properties.
        let feeds: Vec<(i64, String)> = conn
            .prepare("SELECT id, url FROM feeds")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        conn.execute_batch("BEGIN;")?;
        let update = (|| -> Result<()> {
            for (id, url) in feeds {
                let private = url::Url::parse(&url).is_ok_and(|u| !crate::net::host_looks_public(&u));
                conn.execute("UPDATE feeds SET local = ?2 WHERE id = ?1", params![id, private])?;
            }
            Ok(())
        })();
        match update {
            Ok(()) => conn.execute_batch("PRAGMA user_version = 6; COMMIT;")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
        }
    }
    if version < 7 {
        // v7: v6 kept every feed with a private address as allowed. Tidings cannot
        // tell which of those the user typed and which came from an imported OPML
        // file. A feed that works was fetched many times already, and stays. One
        // that never answered with a feed is only a request into the local network
        // at every refresh: it loses the flag, and its error then says how to allow
        // it. Feeds added since 0.9.2 got the flag from the user's own answer.
        conn.execute_batch(&format!(
            "BEGIN;
            UPDATE feeds SET local = 0 WHERE local = 1 AND last_success IS NULL AND created_at < {CONSENT_SINCE};
            PRAGMA user_version = 7;
            COMMIT;"
        ))?;
    }
    if version < 8 {
        // v8: saved searches, filter rules (rules.rs), and translations of articles
        // (translate.rs), one for the feed's text and one
        // for the full text (`full`). They go with the article: removed when it is
        // deleted (here, or by the trigger when it is archived) and when the text they
        // translate changes.
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS translations (
                article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE,
                full       INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                body       TEXT NOT NULL,
                PRIMARY KEY (article_id, full)
            );
            -- Saved searches: the UI's list state (scope, filter, words, dates,
            -- type) as JSON, shown in the sidebar with an unread count.
            CREATE TABLE IF NOT EXISTS searches (
                id         INTEGER PRIMARY KEY,
                position   INTEGER NOT NULL DEFAULT 0,
                title      TEXT NOT NULL,
                state      TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            -- Filter rules (rules.rs). A rule for a feed or a folder goes with it.
            CREATE TABLE IF NOT EXISTS rules (
                id         INTEGER PRIMARY KEY,
                position   INTEGER NOT NULL DEFAULT 0,
                enabled    INTEGER NOT NULL DEFAULT 1,
                feed_id    INTEGER REFERENCES feeds(id) ON DELETE CASCADE,
                folder_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
                field      TEXT NOT NULL,
                words      TEXT NOT NULL,
                action     TEXT NOT NULL,
                hits       INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE TRIGGER IF NOT EXISTS translations_follow_article
            AFTER UPDATE OF is_deleted, title, content, full_content ON articles
            BEGIN
                DELETE FROM translations WHERE article_id = NEW.id AND (
                    NEW.is_deleted = 1 OR NEW.title IS NOT OLD.title
                    OR (full = 0 AND NEW.content IS NOT OLD.content)
                    OR (full = 1 AND NEW.full_content IS NOT OLD.full_content));
            END;
            PRAGMA user_version = 8;
            COMMIT;",
        )?;
    }
    if version < 9 {
        // v9: a translation per language (`lang`: "en" or "ar"); the ones before
        // were all English. The primary key changes, so the table is made again.
        // The trigger names the table, so it goes first and comes back unchanged.
        conn.execute_batch(
            "BEGIN;
            DROP TRIGGER IF EXISTS translations_follow_article;
            CREATE TABLE translations_v9 (
                article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE,
                full       INTEGER NOT NULL,
                lang       TEXT NOT NULL DEFAULT 'en',
                created_at INTEGER NOT NULL,
                body       TEXT NOT NULL,
                PRIMARY KEY (article_id, full, lang)
            );
            INSERT INTO translations_v9 (article_id, full, lang, created_at, body)
                SELECT article_id, full, 'en', created_at, body FROM translations;
            DROP TABLE translations;
            ALTER TABLE translations_v9 RENAME TO translations;
            CREATE TRIGGER translations_follow_article
            AFTER UPDATE OF is_deleted, title, content, full_content ON articles
            BEGIN
                DELETE FROM translations WHERE article_id = NEW.id AND (
                    NEW.is_deleted = 1 OR NEW.title IS NOT OLD.title
                    OR (full = 0 AND NEW.content IS NOT OLD.content)
                    OR (full = 1 AND NEW.full_content IS NOT OLD.full_content));
            END;
            PRAGMA user_version = 9;
            COMMIT;",
        )?;
    }
    if version < 10 {
        // v10: the language of each article and of each feed, for the language
        // filter (language.rs). An article's `lang` is "" when its text does not
        // show it; a feed's `lang` is its usual language, `declared_lang` the one
        // the feed says it is in. Articles get a table of their own: a new column
        // in `articles` would make the upgrade write every article again, long
        // texts and all (seconds for a large database, at startup). One
        // transaction, like v3.
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS article_langs (
                article_id INTEGER PRIMARY KEY REFERENCES articles(id) ON DELETE CASCADE,
                lang       TEXT NOT NULL
            );",
        )?;
        let upgrade = (|| -> Result<()> {
            // A column that is there already stays: the upgrade tests set an older
            // version on a database made by this one.
            for (table, column) in [("feeds", "lang"), ("feeds", "declared_lang")] {
                let has: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
                    params![table, column],
                    |r| r.get(0),
                )?;
                if has == 0 {
                    conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT;"))?;
                }
            }
            fill_languages(conn, false)
        })();
        match upgrade {
            Ok(()) => conn.execute_batch("PRAGMA user_version = 10; COMMIT;")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Stores the language of an article (`language::detect`).
const SET_LANG_SQL: &str =
    "INSERT INTO article_langs (article_id, lang) VALUES (?1, ?2) ON CONFLICT(article_id) DO UPDATE SET lang = excluded.lang";

/// Works out the language of each article that has none yet (with `redo`, of every
/// article), then the usual language of every feed. Run by the v10 migration, and
/// after a restore, which does not copy the backup's languages.
pub fn fill_languages(conn: &Connection, redo: bool) -> Result<()> {
    if redo {
        conn.execute("DELETE FROM article_langs", [])?;
    }
    let rows: Vec<(i64, String, Option<String>)> = conn
        .prepare(
            "SELECT a.id, a.title, a.excerpt FROM articles a
             WHERE a.is_deleted = 0 AND NOT EXISTS (SELECT 1 FROM article_langs al WHERE al.article_id = a.id)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut set = conn.prepare(SET_LANG_SQL)?;
    for (id, title, excerpt) in rows {
        set.execute(params![id, language::detect(&title, excerpt.as_deref().unwrap_or(""))])?;
    }
    let feeds: Vec<i64> =
        conn.prepare("SELECT id FROM feeds")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    for id in feeds {
        detect_feed_language(conn, id)?;
    }
    Ok(())
}

/// Works out a feed's usual language from its newest articles and the language it
/// declares (`language::usual`), and stores it. Run after each refresh.
pub fn detect_feed_language(conn: &Connection, feed_id: i64) -> Result<()> {
    let langs: Vec<String> = conn
        .prepare(
            "SELECT COALESCE(al.lang, '') FROM articles a LEFT JOIN article_langs al ON al.article_id = a.id
             WHERE a.feed_id = ?1 AND a.is_deleted = 0 ORDER BY a.published DESC, a.id DESC LIMIT ?2",
        )?
        .query_map(params![feed_id, language::RECENT_ITEMS], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let declared: Option<String> =
        conn.query_row("SELECT declared_lang FROM feeds WHERE id = ?1", [feed_id], |r| r.get(0)).optional()?.flatten();
    let langs: Vec<&str> = langs.iter().map(String::as_str).collect();
    let usual = language::usual(&langs, declared.as_deref());
    conn.execute("UPDATE feeds SET lang = ?2 WHERE id = ?1", params![feed_id, usual])?;
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
    /// The user allowed Tidings to fetch this feed from the local network.
    pub local: bool,
    /// When it was added (Unix time).
    pub created_at: i64,
    /// Publication date of its newest article, `None` when it has none.
    pub newest: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Only articles in this language: a code from `language::CODES`.
    #[serde(default)]
    pub language: Option<String>,
}

/// Published between `since` (inclusive) and `until` (exclusive), Unix seconds.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
    f.pinned, COALESCE(f.type_override, f.detected_type), f.type_override, f.detected_type,
    COALESCE(f.local, 0), f.created_at,
    (SELECT MAX(a.published) FROM articles a WHERE a.feed_id = f.id AND a.is_deleted = 0)";

fn feed_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Feed> {
    // A feed without a title is shown by its address. A password in the address
    // (https://user:password@host/feed) must not show up in the sidebar.
    let (title, url): (String, String) = (r.get(2)?, r.get(5)?);
    let title = if title == url { crate::net::without_credentials(&url) } else { title };
    Ok(Feed {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        title,
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
        local: r.get(24)?,
        created_at: r.get(25)?,
        newest: r.get(26)?,
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

/// The language an article is listed under: its own, else its feed's usual one, else
/// "und" (Other). Both are always a code from `language::CODES` or "".
const LANG_SQL: &str = "COALESCE(
    NULLIF((SELECT al.lang FROM article_langs al WHERE al.article_id = a.id), ''),
    NULLIF((SELECT fl.lang FROM feeds fl WHERE fl.id = a.feed_id), ''),
    'und')";

/// Articles in one language (the language menu above the list).
fn lang_sql(lang: Option<&str>, args: &mut Vec<Value>) -> String {
    match lang {
        None => "1 = 1".into(),
        Some(code) if language::is_code(code) => {
            args.push(Value::Text(code.to_string()));
            format!("{LANG_SQL} = ?")
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
    lang: Option<&str>,
    args: &mut Vec<Value>,
) -> String {
    let scope = scope_sql(scope, args);
    let search = search_sql(query, args);
    let range = range_sql(range, args);
    let types = type_sql(feed_type, args);
    let lang = lang_sql(lang, args);
    format!("a.is_deleted = 0 AND {scope} AND {} AND {search} AND {range} AND {types} AND {lang}", filter_sql(filter))
}

/// `where_clause` with every filter of `q`.
fn query_clause(q: &ListQuery, args: &mut Vec<Value>) -> String {
    where_clause(&q.scope, &q.filter, &q.query, &q.range, q.feed_type.as_deref(), q.language.as_deref(), args)
}

pub fn list_articles(
    conn: &Connection,
    q: &ListQuery,
    thumb: impl Fn(&str) -> String,
) -> Result<Vec<ArticleItem>> {
    let mut args = Vec::new();
    let clause = query_clause(q, &mut args);
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

/// One article as the model of a brief sees it (brief.rs).
#[derive(Debug, Clone, PartialEq)]
pub struct BriefSource {
    pub id: i64,
    pub feed_id: i64,
    /// The feed's name, as the sidebar shows it.
    pub feed: String,
    pub title: String,
    pub excerpt: String,
    pub published: i64,
}

/// The newest `limit` articles that `q` shows (same filters as the list), and how
/// many it shows in all.
pub fn brief_sources(conn: &Connection, q: &ListQuery, limit: u32) -> Result<(Vec<BriefSource>, usize)> {
    let mut args = Vec::new();
    let clause = query_clause(q, &mut args);
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM articles a WHERE {clause}"), params_from_iter(args.clone()), |r| r.get(0))?;
    args.push(Value::Integer(i64::from(limit)));
    let sql = format!(
        "SELECT a.id, a.feed_id, COALESCE(NULLIF(fd.custom_title, ''), NULLIF(fd.title, ''), fd.url),
                a.title, COALESCE(a.excerpt, ''), a.published
         FROM articles a JOIN feeds fd ON fd.id = a.feed_id
         WHERE {clause} ORDER BY a.published DESC, a.id DESC LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), |r| {
        let feed: String = r.get(2)?;
        Ok(BriefSource {
            id: r.get(0)?,
            feed_id: r.get(1)?,
            // An untitled feed is named by its address: never with its password.
            feed: if feed.contains("://") { crate::net::without_credentials(&feed) } else { feed },
            title: r.get(3)?,
            excerpt: r.get(4)?,
            published: r.get(5)?,
        })
    })?;
    Ok((rows.collect::<std::result::Result<_, _>>()?, total.max(0) as usize))
}

/// A stored brief, without its body (the list of past briefs).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BriefInfo {
    pub id: i64,
    pub created_at: i64,
    pub title: String,
    pub model: String,
}

/// The briefs kept: older ones are removed when a new one is stored.
const KEEP_BRIEFS: i64 = 100;

/// Stores a brief and returns its id. `query` and `body` are JSON.
pub fn insert_brief(conn: &Connection, title: &str, model: &str, query: &str, body: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO briefs (created_at, title, model, query, body) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now(), title, model, query, body],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM briefs WHERE id NOT IN (SELECT id FROM briefs ORDER BY created_at DESC, id DESC LIMIT ?1)",
        [KEEP_BRIEFS],
    )?;
    Ok(id)
}

/// The newest briefs first.
pub fn list_briefs(conn: &Connection, limit: u32) -> Result<Vec<BriefInfo>> {
    let mut stmt =
        conn.prepare("SELECT id, created_at, title, model FROM briefs ORDER BY created_at DESC, id DESC LIMIT ?1")?;
    let rows = stmt.query_map([i64::from(limit)], |r| {
        Ok(BriefInfo { id: r.get(0)?, created_at: r.get(1)?, title: r.get(2)?, model: r.get(3)? })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// A brief's info, query and body.
pub fn get_brief(conn: &Connection, id: i64) -> Result<Option<(BriefInfo, String, String)>> {
    Ok(conn
        .query_row("SELECT id, created_at, title, model, query, body FROM briefs WHERE id = ?1", [id], |r| {
            Ok((BriefInfo { id: r.get(0)?, created_at: r.get(1)?, title: r.get(2)?, model: r.get(3)? }, r.get(4)?, r.get(5)?))
        })
        .optional()?)
}

/// A stored translation of an article (JSON) into `lang` ("en", "ar"), of its full
/// text when `full`.
pub fn translation(conn: &Connection, article_id: i64, full: bool, lang: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT body FROM translations WHERE article_id = ?1 AND full = ?2 AND lang = ?3",
            params![article_id, full, lang],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn save_translation(conn: &Connection, article_id: i64, full: bool, lang: &str, body: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO translations (article_id, full, lang, created_at, body) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(article_id, full, lang) DO UPDATE SET created_at = excluded.created_at, body = excluded.body",
        params![article_id, full, lang, now(), body],
    )?;
    Ok(())
}

pub fn delete_brief(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM briefs WHERE id = ?1", [id])?;
    Ok(())
}

pub fn list_article_ids(conn: &Connection, q: &ListQuery) -> Result<Vec<i64>> {
    let mut args = Vec::new();
    let clause = query_clause(q, &mut args);
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

/// Every starred article, newest first, with the name of its feed (export.rs).
pub fn starred_articles(conn: &Connection) -> Result<Vec<(ArticleRow, String)>> {
    let mut args = Vec::new();
    let clause = where_clause(&Scope::Starred, "all", "", &DateRange::default(), None, None, &mut args);
    let sql = format!("SELECT a.id FROM articles a WHERE {clause} ORDER BY a.published DESC, a.id DESC");
    let ids: Vec<i64> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(args), |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let names: std::collections::HashMap<i64, String> = feeds(conn)?.into_iter().map(|f| (f.id, f.title)).collect();
    ids.into_iter()
        .map(|id| {
            let row = article(conn, id)?;
            let feed = names.get(&row.feed_id).cloned().unwrap_or_default();
            Ok((row, feed))
        })
        .collect()
}

/// How many articles the list would show for `q` (its limit and offset aside).
pub fn count_articles(conn: &Connection, q: &ListQuery) -> Result<i64> {
    let mut args = Vec::new();
    let clause = query_clause(q, &mut args);
    let sql = format!("SELECT COUNT(*) FROM articles a WHERE {clause}");
    Ok(conn.query_row(&sql, params_from_iter(args), |r| r.get(0))?)
}

/// How many articles in one language the scope holds, and how many of them are unread.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCount {
    pub language: String,
    pub total: i64,
    pub unread: i64,
}

/// The languages of the articles in a scope, most articles first: the language menu
/// above the list. Like the type icons, it leaves out read state, dates and search.
pub fn language_counts(conn: &Connection, scope: &Scope, feed_type: Option<&str>) -> Result<Vec<LanguageCount>> {
    let mut args = Vec::new();
    let clause = where_clause(scope, "all", "", &DateRange::default(), feed_type, None, &mut args);
    let sql = format!(
        "SELECT {LANG_SQL}, COUNT(*), SUM(a.is_read = 0) FROM articles a
         WHERE {clause} GROUP BY 1 ORDER BY 2 DESC, 1"
    );
    let rows = conn
        .prepare(&sql)?
        .query_map(params_from_iter(args), |r| Ok(LanguageCount { language: r.get(0)?, total: r.get(1)?, unread: r.get(2)? }))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// How active a feed is, for the feed health page.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeedActivity {
    pub feed_id: i64,
    /// Its newest article, also a deleted one (deleted articles keep their date).
    pub newest: Option<i64>,
    /// Articles it has now.
    pub articles: i64,
}

pub fn feed_activity(conn: &Connection) -> Result<Vec<FeedActivity>> {
    let rows = conn
        .prepare(
            "SELECT f.id, MAX(a.published), COALESCE(SUM(a.is_deleted = 0), 0)
             FROM feeds f LEFT JOIN articles a ON a.feed_id = f.id
             GROUP BY f.id ORDER BY f.id",
        )?
        .query_map([], |r| Ok(FeedActivity { feed_id: r.get(0)?, newest: r.get(1)?, articles: r.get(2)? }))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// A saved search: a name and the UI's list state (JSON).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: i64,
    pub title: String,
    pub state: String,
}

/// A saved search's name: one line of at most 100 characters.
pub fn search_title(title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() || title.chars().count() > 100 {
        return Err(Error::msg("Give the search a name of at most 100 characters"));
    }
    Ok(title)
}

/// Whether `state` can be a saved search's list state: a JSON object of at most 16 KB.
pub fn search_state_is_valid(state: &str) -> bool {
    state.len() <= 16 * 1024 && serde_json::from_str::<serde_json::Value>(state).is_ok_and(|v| v.is_object())
}

pub fn searches(conn: &Connection) -> Result<Vec<SavedSearch>> {
    let rows = conn
        .prepare("SELECT id, title, state FROM searches ORDER BY position, id")?
        .query_map([], |r| Ok(SavedSearch { id: r.get(0)?, title: r.get(1)?, state: r.get(2)? }))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

pub fn insert_search(conn: &Connection, title: &str, state: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO searches (position, title, state, created_at)
         VALUES ((SELECT COALESCE(MAX(position), 0) + 1 FROM searches), ?1, ?2, ?3)",
        params![title, state, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_search(conn: &Connection, id: i64, title: &str) -> Result<()> {
    conn.execute("UPDATE searches SET title = ?2 WHERE id = ?1", params![id, title])?;
    Ok(())
}

pub fn delete_search(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM searches WHERE id = ?1", [id])?;
    Ok(())
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

pub fn mark_scope_read(
    conn: &Connection,
    scope: &Scope,
    range: &DateRange,
    feed_type: Option<&str>,
    lang: Option<&str>,
) -> Result<usize> {
    let mut args = Vec::new();
    let clause = where_clause(scope, "unread", "", range, feed_type, lang, &mut args);
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
            // Only the guid stays, so the article is not added again at the next refresh.
            "UPDATE articles SET is_deleted = 1, is_starred = 0, is_read = 1, is_new = 0,
                content = NULL, full_content = NULL, excerpt = NULL, image_url = NULL,
                title = '', url = NULL, author = NULL, enclosure_url = NULL, enclosure_type = NULL
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
    /// The user allowed Tidings to fetch this feed from the local network.
    pub local: bool,
}

/// Records that the user allowed (or no longer allows) a feed on the local network.
pub fn set_feed_local(conn: &Connection, id: i64, local: bool) -> Result<()> {
    conn.execute("UPDATE feeds SET local = ?2 WHERE id = ?1", params![id, local])?;
    Ok(())
}

pub fn fetch_info(conn: &Connection, id: i64) -> Result<FetchInfo> {
    conn.query_row(
        "SELECT url, etag, last_modified, site_url, icon IS NOT NULL, icon_checked,
                retention_mode, retention_value, COALESCE(local, 0)
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
                local: r.get(8)?,
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
    /// The language the feed says it is written in, as the feed gives it ("en-US").
    pub language: Option<String>,
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
    let mut set_lang = tx.prepare_cached(SET_LANG_SQL)?;
    let mut touch = tx.prepare_cached("UPDATE articles SET last_seen = ?2 WHERE id = ?1")?;

    let mut added = 0;
    let mut new = Vec::new();
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
                let id = tx.last_insert_rowid();
                set_lang.execute(params![id, language::detect(&item.title, &item.excerpt)])?;
                added += 1;
                new.push((id, item));
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
                set_lang.execute(params![id, language::detect(&item.title, &item.excerpt)])?;
            }
            Some((id, _, _)) => {
                touch.execute(params![id, now])?;
            }
        }
    }
    // Articles that a rule deleted or marked as read are not news.
    let handled = apply_rules_to_new(tx, feed_id, &new)?;
    Ok(added - handled)
}

// ---------------------------------------------------------------------------
// Filter rules (rules.rs)
// ---------------------------------------------------------------------------

fn rule_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Rule> {
    Ok(Rule {
        id: r.get(0)?,
        enabled: r.get(1)?,
        feed_id: r.get(2)?,
        folder_id: r.get(3)?,
        field: r.get(4)?,
        words: r.get(5)?,
        action: r.get(6)?,
        hits: r.get(7)?,
    })
}

const RULE_COLUMNS: &str = "id, enabled, feed_id, folder_id, field, words, action, hits";

pub fn rules(conn: &Connection) -> Result<Vec<Rule>> {
    let sql = format!("SELECT {RULE_COLUMNS} FROM rules ORDER BY position, id");
    let rules = conn.prepare(&sql)?.query_map([], rule_from_row)?.collect::<std::result::Result<_, _>>()?;
    Ok(rules)
}

/// Stores a new rule (`id` 0) or changes one; returns its id. `rule` must be validated.
pub fn save_rule(conn: &Connection, rule: &Rule) -> Result<i64> {
    if rule.id == 0 {
        conn.execute(
            "INSERT INTO rules (position, enabled, feed_id, folder_id, field, words, action, created_at)
             VALUES ((SELECT COALESCE(MAX(position), 0) + 1 FROM rules), ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rule.enabled, rule.feed_id, rule.folder_id, rule.field, rule.words, rule.action, now()],
        )?;
        return Ok(conn.last_insert_rowid());
    }
    let changed = conn.execute(
        "UPDATE rules SET enabled = ?2, feed_id = ?3, folder_id = ?4, field = ?5, words = ?6, action = ?7 WHERE id = ?1",
        params![rule.id, rule.enabled, rule.feed_id, rule.folder_id, rule.field, rule.words, rule.action],
    )?;
    if changed == 0 {
        return Err(Error::msg("This rule no longer exists"));
    }
    Ok(rule.id)
}

pub fn rule(conn: &Connection, id: i64) -> Result<Rule> {
    let sql = format!("SELECT {RULE_COLUMNS} FROM rules WHERE id = ?1");
    conn.query_row(&sql, [id], rule_from_row).optional()?.ok_or_else(|| Error::msg("This rule no longer exists"))
}

pub fn delete_rule(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM rules WHERE id = ?1", [id])?;
    Ok(())
}

/// The feeds a rule is for: one feed, the feeds in a folder and the folders inside
/// it, or all feeds.
pub fn rule_feeds(conn: &Connection, rule: &Rule) -> Result<Vec<i64>> {
    let ids = if let Some(feed) = rule.feed_id {
        conn.prepare("SELECT id FROM feeds WHERE id = ?1")?.query_map([feed], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
    } else if let Some(folder) = rule.folder_id {
        conn.prepare(
            "WITH RECURSIVE inside(id) AS (
                SELECT ?1 UNION SELECT f.id FROM folders f JOIN inside ON f.parent_id = inside.id)
             SELECT id FROM feeds WHERE folder_id IN inside",
        )?
        .query_map([folder], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?
    } else {
        conn.prepare("SELECT id FROM feeds")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
    };
    Ok(ids)
}

/// What a rule can look at in a stored article.
fn rule_article(title: String, content: Option<&str>, excerpt: &str, author: Option<String>, url: Option<String>) -> rules::Article {
    let text = match content {
        Some(html) if !html.is_empty() => crate::sanitize::quick_text(html),
        _ => excerpt.to_string(),
    };
    rules::Article { title, text, author: author.unwrap_or_default(), url: url.unwrap_or_default() }
}

/// Does what the rules for `feed_id` say to the articles a refresh just added.
/// Returns how many of them were deleted or marked as read.
fn apply_rules_to_new(tx: &Transaction<'_>, feed_id: i64, new: &[(i64, &NewArticle)]) -> Result<usize> {
    if new.is_empty() {
        return Ok(0);
    }
    let mut mine = Vec::new();
    for rule in rules(tx)?.into_iter().filter(|r| r.enabled) {
        if rule_feeds(tx, &rule)?.contains(&feed_id) {
            mine.push(rule);
        }
    }
    if mine.is_empty() {
        return Ok(0);
    }
    let mut hits: Vec<(i64, i64)> = mine.iter().map(|r| (r.id, 0)).collect();
    let (mut read, mut star, mut delete) = (Vec::new(), Vec::new(), Vec::new());
    for (id, item) in new {
        let article = rule_article(item.title.clone(), Some(&item.content), &item.excerpt, item.author.clone(), item.url.clone());
        for (rule, hit) in mine.iter().zip(hits.iter_mut()) {
            if rule.matches(&article) {
                hit.1 += 1;
                match rule.action.as_str() {
                    "read" => read.push(*id),
                    "star" => star.push(*id),
                    _ => delete.push(*id),
                }
            }
        }
    }
    // Deleting wins over the other rules.
    read.retain(|id| !delete.contains(id));
    star.retain(|id| !delete.contains(id));
    read.dedup();
    star.dedup();
    set_read(tx, &read, true)?;
    for id in &star {
        set_starred(tx, *id, true)?;
    }
    delete.dedup();
    delete_articles(tx, &delete)?;
    for (rule, count) in hits.into_iter().filter(|(_, n)| *n > 0) {
        tx.execute("UPDATE rules SET hits = hits + ?2 WHERE id = ?1", params![rule, count])?;
    }
    Ok(read.len() + delete.len())
}

/// Stored articles that `rule` matches, `limit` at a time from after the id
/// `after`: the matching ids, and the last id looked at (None when done).
/// Starred articles are never marked read or deleted by a rule, and articles the
/// action would not change are left out.
pub fn rule_batch(conn: &Connection, rule: &Rule, feeds: &[i64], after: i64, limit: usize) -> Result<(Vec<i64>, Option<i64>)> {
    if feeds.is_empty() {
        return Ok((Vec::new(), None));
    }
    let skip = match rule.action.as_str() {
        "read" => "AND is_read = 0 AND is_starred = 0",
        "star" => "AND is_starred = 0",
        _ => "AND is_starred = 0",
    };
    let sql = format!(
        "SELECT id, title, content, excerpt, author, url FROM articles
         WHERE is_deleted = 0 {skip} AND id > ?1 AND feed_id IN ({})
         ORDER BY id LIMIT {limit}",
        feeds.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([after])?;
    let (mut matched, mut last, mut seen) = (Vec::new(), None, 0);
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let content: Option<String> = r.get(2)?;
        let excerpt: Option<String> = r.get(3)?;
        let article = rule_article(r.get(1)?, content.as_deref(), excerpt.as_deref().unwrap_or(""), r.get(4)?, r.get(5)?);
        if rule.matches(&article) {
            matched.push(id);
        }
        last = Some(id);
        seen += 1;
    }
    Ok((matched, if seen < limit { None } else { last }))
}

/// Does what `rule` says to the articles `ids`, and counts them as its hits.
pub fn apply_rule_to(conn: &Connection, rule: &Rule, ids: &[i64]) -> Result<()> {
    match rule.action.as_str() {
        "read" => set_read(conn, ids, true)?,
        "star" => {
            for id in ids {
                set_starred(conn, *id, true)?;
            }
        }
        _ => delete_articles(conn, ids)?,
    }
    conn.execute("UPDATE rules SET hits = hits + ?2 WHERE id = ?1", params![rule.id, ids.len() as i64])?;
    Ok(())
}

pub fn update_feed_meta(tx: &Transaction<'_>, id: i64, meta: &FeedMeta) -> Result<()> {
    let now = now();
    tx.execute(
        "UPDATE feeds SET title = COALESCE(?2, title), site_url = COALESCE(?3, site_url),
            description = COALESCE(?4, description), etag = ?5, last_modified = ?6,
            last_fetched = ?7, last_success = ?7, last_error = NULL, declared_lang = ?8
         WHERE id = ?1",
        params![
            id,
            meta.title,
            meta.site_url,
            meta.description,
            meta.etag,
            meta.last_modified,
            now,
            meta.language.as_deref().and_then(language::declared)
        ],
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
                         excerpt = NULL, image_url = NULL, title = '', url = NULL, author = NULL,
                         enclosure_url = NULL, enclosure_type = NULL";
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

/// Moves everything from the write-ahead log into the database file and empties the
/// log. Deleted article text can otherwise stay in the log's old pages for a while.
pub fn checkpoint(conn: &Connection) {
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

/// Stored values next to the settings (the `settings` table is key and value).
pub fn get_value(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn set_value(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
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

/// Where a feed is, for comparing an OPML file with the subscriptions.
#[derive(Debug)]
pub struct FeedPlace {
    pub id: i64,
    pub url: String,
    pub folder_id: Option<i64>,
    /// Display title: custom title, else feed title, else URL.
    pub title: String,
}

pub fn feed_places(conn: &Connection) -> Result<Vec<FeedPlace>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, folder_id, COALESCE(NULLIF(custom_title, ''), NULLIF(title, ''), url) FROM feeds ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let (url, title): (String, String) = (r.get(1)?, r.get(3)?);
        // As in `feed_from_row`: a password in the address must not show.
        let title = if title == url { crate::net::without_credentials(&url) } else { title };
        Ok(FeedPlace { id: r.get(0)?, url, folder_id: r.get(2)?, title })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
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
    /// Allow this feed on the local network. `None` keeps the current answer, but a
    /// new address always starts without it.
    #[serde(default)]
    pub local: Option<bool>,
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
        // Consent for the local network belongs to one address: a new address starts
        // without it. The right-hand `url` is the old address.
        "UPDATE feeds SET custom_title = ?2, url = ?3, update_interval = ?4, retention_mode = ?5,
            retention_value = ?6, full_text = ?7, paused = ?8, type_override = ?9,
            local = CASE WHEN ?10 IS NOT NULL THEN ?10 WHEN url = ?3 THEN COALESCE(local, 0) ELSE 0 END
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
            type_override,
            patch.local,
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

/// Sets the type of several feeds: one of `feed_types::TYPES`, or `None` to go back
/// to the detected type. Returns how many feeds changed.
pub fn set_feed_type(conn: &Connection, ids: &[i64], feed_type: Option<&str>) -> Result<usize> {
    if feed_type.is_some_and(|t| !feed_types::is_type(t)) {
        return Err(Error::msg("Unknown feed type"));
    }
    let mut changed = 0;
    for chunk in ids.chunks(500) {
        let sql = format!("UPDATE feeds SET type_override = ? WHERE id IN ({})", placeholders(chunk.len()));
        let mut args: Vec<Value> = vec![feed_type.map_or(Value::Null, |t| Value::Text(t.into()))];
        args.extend(chunk.iter().map(|id| Value::Integer(*id)));
        changed += conn.execute(&sql, params_from_iter(args))?;
    }
    Ok(changed)
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
