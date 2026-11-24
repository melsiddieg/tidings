//! Backups: the whole database in one file, and restoring one.
//!
//! A backup is a copy of the database made with `VACUUM INTO`: folders, feeds,
//! articles with their read and starred state, settings, rules, saved searches,
//! briefs and translations. It holds every feed address and the proxy setting,
//! which can hold passwords, so it is written readable only by this user. Freed
//! pages are not copied, so the text of removed articles is not in it.
//!
//! A backup file can come from anywhere, so restoring never uses its schema: none
//! of its tables, triggers, views or indexes reaches Tidings' database. Tidings
//! opens the file read-only with SQLite's settings for files that cannot be
//! trusted, checks it, and copies the rows of the tables it knows, column by
//! column, into its own tables. Every value must have the type Tidings' schema
//! gives its column. Rules and saved searches are checked like new ones, and
//! settings like saved ones (`settings::restored`). Stored HTML is copied as it is:
//! `sanitize::render` cleans it again before it is shown, and every export cleans
//! it too. Feed icons are re-encoded each time they are shown (`protocol.rs`).
//!
//! All of it happens in one transaction: a restore that fails changes nothing.
//! Before it starts, the current data is copied to `tidings-before-restore.db` in
//! the data folder, which can itself be restored to go back.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::config::DbConfig;
use rusqlite::limits::Limit;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::db;
use crate::error::{Error, Result};
use crate::files;
use crate::settings::{self, Settings};

/// Marks a Tidings database in the SQLite file header ("Tdng").
pub const APPLICATION_ID: i32 = 0x5464_6e67;

/// The oldest schema a backup may have. Versions 6 and 7 changed what `local`
/// means (the user's consent to the local network), so older rows cannot be
/// copied as they are. Raise it when a migration changes what stored values mean.
const OLDEST_VERSION: i64 = 7;

/// The tables copied from a backup, each after the tables it refers to. Other rows
/// of `settings` describe this computer (the yt-dlp version seen) and stay.
const TABLES: &[&str] = &["folders", "feeds", "articles", "translations", "briefs", "searches", "rules"];

/// The longest value read from a backup. Stored HTML is cut to 2 MB, icons are
/// small: a larger value is not from Tidings, and would only use memory.
const MAX_VALUE_BYTES: i32 = 32 * 1024 * 1024;

/// The `settings` row of a backup that says when it was made and by which version.
const ABOUT_KEY: &str = "backup";

/// The name of the copy of the data from before the last restore.
pub const PREVIOUS_FILE: &str = "tidings-before-restore.db";

fn damaged() -> Error {
    Error::msg("This file is not a Tidings backup, or it is damaged")
}

/// Copies the database `conn` reads to a new file at `target`, readable only by
/// this user. `conn` can be read-only: the live database stays open meanwhile.
pub fn copy_to(conn: &Connection, target: &Path) -> Result<()> {
    let (temp, file) = files::private_temp(target)?;
    // VACUUM INTO writes into an empty file.
    drop(file);
    let result = (|| -> Result<()> {
        let name = temp.to_str().ok_or_else(|| Error::msg("This folder's name cannot be used for a backup"))?;
        conn.execute("VACUUM INTO ?1", [name])?;
        let copy = Connection::open(&temp)?;
        let about = serde_json::json!({ "createdAt": db::now(), "version": env!("CARGO_PKG_VERSION") });
        copy.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ABOUT_KEY, about.to_string()],
        )?;
        copy.pragma_update(None, "application_id", APPLICATION_ID)?;
        copy.close().map_err(|(_, e)| e)?;
        std::fs::File::open(&temp)?.sync_all()?;
        std::fs::rename(&temp, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Opens a backup file to read, with SQLite's settings for files that cannot be
/// trusted, and checks that it is a Tidings database this version can restore.
fn open_backup(path: &Path) -> Result<Connection> {
    let uri = url::Url::from_file_path(path).map_err(|_| damaged())?;
    // `immutable`: no locks, no journal or write-ahead log of its own.
    let conn = Connection::open_with_flags(
        format!("{uri}?mode=ro&immutable=1"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| damaged())?;
    let header = (|| -> Result<(i32, i64)> {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
        conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_VALUE_BYTES)?;
        conn.execute_batch("PRAGMA cell_size_check = ON;")?;
        let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(damaged());
        }
        let app = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        Ok((app, version))
    })();
    let (app, version) = header.map_err(|_| damaged())?;
    // 0: a copy of the database file of Tidings 0.10 or older.
    if app != APPLICATION_ID && app != 0 {
        return Err(damaged());
    }
    if version > db::SCHEMA_VERSION {
        return Err(Error::msg("This backup was made by a newer version of Tidings. Update Tidings, then restore it."));
    }
    if version < OLDEST_VERSION {
        return Err(Error::msg("This backup is from a version of Tidings before 0.9.4, which cannot be restored."));
    }
    if !is_table(&conn, "feeds")? || !is_table(&conn, "articles")? {
        return Err(damaged());
    }
    Ok(conn)
}

/// Whether `name` is an ordinary table in `conn`: not a view or a virtual table,
/// whose reads would run code from the file.
fn is_table(conn: &Connection, name: &str) -> Result<bool> {
    let sql: Option<Option<String>> = conn
        .query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get(0))
        .optional()?;
    Ok(sql.flatten().is_some_and(|sql| !sql.trim_start().to_ascii_uppercase().starts_with("CREATE VIRTUAL")))
}

/// The columns of `table` and the types they were declared with.
fn columns(conn: &Connection, table: &str) -> Result<Vec<(String, String)>> {
    let columns = conn
        .prepare("SELECT name, type FROM pragma_table_info(?1)")?
        .query_map([table], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(columns)
}

/// Whether `value` has the type Tidings declares for its column.
fn fits(value: &Value, declared: &str) -> bool {
    match value {
        Value::Null => true,
        Value::Integer(_) => declared.eq_ignore_ascii_case("INTEGER"),
        Value::Text(_) => declared.eq_ignore_ascii_case("TEXT"),
        Value::Blob(_) => declared.eq_ignore_ascii_case("BLOB"),
        Value::Real(_) => false,
    }
}

/// Copies the rows of `table` from the backup into Tidings' table of that name:
/// the columns both have. Column names come from Tidings' own schema.
fn copy_table(from: &Connection, to: &Connection, table: &str) -> Result<usize> {
    if !is_table(from, table)? {
        return Ok(0);
    }
    let theirs: HashSet<String> = columns(from, table)?.into_iter().map(|(name, _)| name.to_ascii_lowercase()).collect();
    let shared: Vec<(String, String)> =
        columns(to, table)?.into_iter().filter(|(name, _)| theirs.contains(&name.to_ascii_lowercase())).collect();
    if shared.is_empty() {
        return Ok(0);
    }
    let names = shared.iter().map(|(name, _)| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
    let marks = vec!["?"; shared.len()].join(", ");
    let mut read = from.prepare(&format!("SELECT {names} FROM \"{table}\""))?;
    let mut write = to.prepare(&format!("INSERT INTO \"{table}\" ({names}) VALUES ({marks})"))?;
    let mut rows = read.query([])?;
    let mut count = 0;
    // A value that cannot be read: a damaged page, text that is not UTF-8, or a
    // value over MAX_VALUE_BYTES.
    while let Some(row) = rows.next().map_err(|_| damaged())? {
        let mut values = Vec::with_capacity(shared.len());
        for (i, (name, declared)) in shared.iter().enumerate() {
            let value: Value = row.get(i).map_err(|_| damaged())?;
            if !fits(&value, declared) {
                return Err(Error::msg(format!("This backup has a value Tidings cannot use ({table}.{name})")));
            }
            values.push(value);
        }
        write.execute(params_from_iter(values))?;
        count += 1;
    }
    Ok(count)
}

/// The backup's saved settings (JSON), if it has them.
fn settings_json(conn: &Connection) -> Result<Option<String>> {
    if !is_table(conn, "settings")? {
        return Ok(None);
    }
    Ok(conn.query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0)).optional()?)
}

/// What a backup holds, shown before it is restored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub file_name: String,
    /// When the backup was made (Unix seconds), and by which version of Tidings.
    /// None for a copy of the database file.
    pub created_at: Option<i64>,
    pub version: Option<String>,
    pub folders: i64,
    pub feeds: i64,
    pub articles: i64,
    pub unread: i64,
    pub starred: i64,
    pub rules: i64,
    pub searches: i64,
    /// The addresses (host and port) of feeds the user allowed on the local
    /// network. They keep that permission.
    pub local_feeds: Vec<String>,
    /// The proxy now and after the restore, without any password in them.
    pub proxy_now: String,
    pub proxy_after: String,
    /// Set when the restored settings cannot be used: Tidings goes offline.
    pub offline: Option<String>,
}

fn count(conn: &Connection, table: &str, condition: &str) -> Result<i64> {
    if !is_table(conn, table)? {
        return Ok(0);
    }
    Ok(conn.query_row(&format!("SELECT COUNT(*) FROM \"{table}\" WHERE {condition}"), [], |r| r.get(0))?)
}

/// Reads and checks a backup and says what restoring it would do.
pub fn inspect(path: &Path, current: &Settings) -> Result<Summary> {
    let conn = open_backup(path)?;
    let about: Option<serde_json::Value> = if is_table(&conn, "settings")? {
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [ABOUT_KEY], |r| r.get::<_, String>(0))
            .optional()?
            .and_then(|json| serde_json::from_str(&json).ok())
    } else {
        None
    };
    let mut local_feeds: Vec<String> = Vec::new();
    let urls: Vec<String> = conn
        .prepare("SELECT url FROM feeds WHERE local = 1 ORDER BY url")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for url in urls {
        let Ok(url) = url::Url::parse(&url) else { continue };
        let host = match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_string(),
            _ => continue,
        };
        if !local_feeds.contains(&host) {
            local_feeds.push(host);
        }
    }
    let restored = settings::restored(settings_json(&conn)?.as_deref(), current);
    Ok(Summary {
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        created_at: about.as_ref().and_then(|a| a["createdAt"].as_i64()),
        version: about.as_ref().and_then(|a| a["version"].as_str()).map(|v| v.chars().take(40).collect()),
        folders: count(&conn, "folders", "1")?,
        feeds: count(&conn, "feeds", "1")?,
        articles: count(&conn, "articles", "is_deleted = 0")?,
        unread: count(&conn, "articles", "is_deleted = 0 AND is_read = 0")?,
        starred: count(&conn, "articles", "is_deleted = 0 AND is_starred = 1")?,
        rules: count(&conn, "rules", "1")?,
        searches: count(&conn, "searches", "1")?,
        local_feeds,
        proxy_now: crate::net::without_credentials(&current.proxy),
        proxy_after: crate::net::without_credentials(&restored.settings.proxy),
        offline: restored.offline,
    })
}

/// Removes restored rules and saved searches that a new one could not be.
fn drop_invalid(conn: &Connection) -> Result<()> {
    for rule in db::rules(conn)? {
        if rule.clone().validated().is_err() {
            db::delete_rule(conn, rule.id)?;
        }
    }
    for search in db::searches(conn)? {
        if db::search_title(&search.title).is_err() || !db::search_state_is_valid(&search.state) {
            db::delete_search(conn, search.id)?;
        }
    }
    Ok(())
}

/// Replaces everything in `live` with the backup at `path`, in one transaction.
/// The current data is first copied to `previous`. Returns the settings to use
/// now, and why Tidings must stay offline, if it must.
pub fn restore(live: &mut Connection, path: &Path, previous: &Path, current: &Settings) -> Result<settings::Loaded> {
    let source = open_backup(path)?;
    let restored = settings::restored(settings_json(&source)?.as_deref(), current);
    copy_to(live, previous)?;

    let tx = live.transaction()?;
    // Rows refer to rows of the same table (folders inside folders), in any order.
    // The references are checked once all are copied.
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    for table in TABLES.iter().rev() {
        tx.execute(&format!("DELETE FROM \"{table}\""), [])?;
    }
    for table in TABLES {
        copy_table(&source, &tx, table)?;
    }
    drop_invalid(&tx)?;
    // Languages are worked out again: the file's could be anything.
    db::fill_languages(&tx, true)?;
    db::clear_new_flags(&tx)?;
    settings::save(&tx, &restored.settings)?;
    // What the rest of Tidings reads must load.
    db::tree(&tx).map_err(|_| damaged())?;
    db::counts(&tx).map_err(|_| damaged())?;
    let broken: i64 = tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
    if broken > 0 {
        return Err(damaged());
    }
    tx.commit()?;
    // Deleting wrote the old text into the write-ahead log: empty it.
    db::checkpoint(live);
    Ok(restored)
}
