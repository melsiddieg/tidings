//! Integration tests that run every database query against a real SQLite file.

use crate::db::{self, Item, ListQuery, Scope};
use crate::feeds;
use crate::opml;
use crate::sanitize;
use url::Url;

const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
<channel>
  <title>Example &amp; Co</title>
  <link>https://example.com/</link>
  <description>News from Example</description>
  <item>
    <title>First post</title>
    <link>https://example.com/first?utm_source=rss&amp;id=1</link>
    <guid>first</guid>
    <pubDate>Mon, 03 Nov 2025 10:00:00 GMT</pubDate>
    <description><![CDATA[<p onclick="x()">Hello <b>world</b><script>alert(1)</script></p><img src="/img/a.png">]]></description>
  </item>
  <item>
    <title>Second post about Rust</title>
    <link>https://example.com/second</link>
    <guid>second</guid>
    <pubDate>Tue, 04 Nov 2025 10:00:00 GMT</pubDate>
    <description>Plain text summary</description>
    <enclosure url="https://cdn.example.com/ep.mp3" type="audio/mpeg" length="123"/>
  </item>
  <item>
    <title>Third</title>
    <link>https://example.com/third</link>
    <guid>third</guid>
    <pubDate>Wed, 05 Nov 2025 10:00:00 GMT</pubDate>
    <description>Third summary</description>
  </item>
</channel>
</rss>"#;

const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Blog</title>
  <link href="https://atom.example/" rel="alternate"/>
  <link href="https://atom.example/feed.xml" rel="self"/>
  <id>urn:atom</id>
  <updated>2025-11-05T10:00:00Z</updated>
  <entry>
    <title type="html">A &lt;em&gt;styled&lt;/em&gt; title</title>
    <link href="https://atom.example/post"/>
    <id>urn:post1</id>
    <updated>2025-11-05T10:00:00Z</updated>
    <author><name>Ada</name></author>
    <content type="html">&lt;p&gt;Body with &lt;a href="/rel"&gt;link&lt;/a&gt;&lt;/p&gt;</content>
  </entry>
</feed>"#;

fn temp_db() -> (rusqlite::Connection, std::path::PathBuf) {
    // One folder per call: tests run in parallel and each removes its own folder.
    let dir = std::env::temp_dir().join(format!("tidings-test-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    (db::open(&path).unwrap(), dir)
}

/// Unique within this test run. The clock alone is not enough: on macOS it moves in
/// microseconds, so two tests that start together got the same folder.
fn rand_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{nanos}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn query(scope: Scope, filter: &str, q: &str) -> ListQuery {
    ListQuery {
        scope,
        filter: filter.into(),
        query: q.into(),
        oldest_first: false,
        limit: Some(100),
        offset: Some(0),
        range: db::DateRange::default(),
        feed_type: None,
    }
}

#[test]
fn parses_rss_and_atom() {
    let url = Url::parse("https://example.com/feed.xml").unwrap();
    let parsed = feeds::parse_feed(RSS.as_bytes(), &url, true).unwrap();
    assert_eq!(parsed.title.as_deref(), Some("Example & Co"));
    assert_eq!(parsed.site_url.as_deref(), Some("https://example.com/"));
    assert_eq!(parsed.articles.len(), 3);
    let first = parsed.articles.iter().find(|a| a.guid == "first").unwrap();
    assert_eq!(first.url.as_deref(), Some("https://example.com/first?id=1"));
    assert!(!first.content.contains("script"));
    assert!(!first.content.contains("onclick"));
    assert!(first.content.contains("https://example.com/img/a.png"));
    assert_eq!(first.image_url.as_deref(), Some("https://example.com/img/a.png"));
    let second = parsed.articles.iter().find(|a| a.guid == "second").unwrap();
    assert_eq!(second.enclosure_url.as_deref(), Some("https://cdn.example.com/ep.mp3"));
    assert!(second.content.contains("Plain text summary"));
    assert_eq!(second.excerpt, "Plain text summary");

    let atom_url = Url::parse("https://atom.example/feed.xml").unwrap();
    let atom = feeds::parse_feed(ATOM.as_bytes(), &atom_url, true).unwrap();
    assert_eq!(atom.site_url.as_deref(), Some("https://atom.example/"));
    let post = &atom.articles[0];
    assert_eq!(post.title, "A styled title");
    assert_eq!(post.author.as_deref(), Some("Ada"));
    assert!(post.content.contains("https://atom.example/rel"));
}

#[test]
fn rejects_html_pages_as_feeds() {
    let url = Url::parse("https://example.com/").unwrap();
    let err = feeds::parse_feed(b"<!doctype html><html><body>hi</body></html>", &url, true)
        .err()
        .unwrap();
    assert!(err.to_string().contains("web page"));
}

#[test]
fn full_database_workflow() {
    let (mut conn, dir) = temp_db();
    let url = Url::parse("https://example.com/feed.xml").unwrap();
    let parsed = feeds::parse_feed(RSS.as_bytes(), &url, true).unwrap();

    // Folders: Tech > Rust, and a feed in each place.
    let tech = db::create_folder(&conn, "Tech", None).unwrap();
    let rust = db::create_folder(&conn, "Rust", Some(tech)).unwrap();
    let feed_a = db::insert_feed(&conn, url.as_str(), Some(rust), None, None).unwrap();
    let feed_b = db::insert_feed(&conn, "https://other.example/rss", None, Some("Other"), None).unwrap();
    let mut urls = db::feed_urls(&conn).unwrap();
    urls.sort();
    assert_eq!(urls, ["https://example.com/feed.xml", "https://other.example/rss"]);

    // Store articles; storing twice must not duplicate.
    for _ in 0..2 {
        let tx = conn.transaction().unwrap();
        db::store_articles(&tx, feed_a, &parsed.articles, None).unwrap();
        db::update_feed_meta(
            &tx,
            feed_a,
            &db::FeedMeta {
                title: parsed.title.clone(),
                site_url: parsed.site_url.clone(),
                description: parsed.description.clone(),
                etag: Some("\"abc\"".into()),
                last_modified: None,
            },
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let thumb = |u: &str| sanitize::proxy_image_url(u);
    let all = db::list_articles(&conn, &query(Scope::All, "all", ""), thumb).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].title, "Third", "newest first");
    assert!(all.iter().any(|a| a.thumb.as_deref().is_some_and(|t| t.starts_with("tidings://"))));

    // Folder scope includes subfolders (recursive query).
    let in_tech = db::list_articles(&conn, &query(Scope::Folder { id: tech }, "all", ""), thumb).unwrap();
    assert_eq!(in_tech.len(), 3);
    let in_feed_b = db::list_articles(&conn, &query(Scope::Feed { id: feed_b }, "all", ""), thumb).unwrap();
    assert!(in_feed_b.is_empty());

    // Search, including LIKE wildcards in the query.
    let found = db::list_articles(&conn, &query(Scope::All, "all", "rust"), thumb).unwrap();
    assert_eq!(found.len(), 1);
    let none = db::list_articles(&conn, &query(Scope::All, "all", "100%_"), thumb).unwrap();
    assert!(none.is_empty());

    // Read / starred / counts.
    let ids: Vec<i64> = all.iter().map(|a| a.id).collect();
    db::set_read(&conn, &ids[..1], true).unwrap();
    db::set_starred(&conn, ids[1], true).unwrap();
    let counts = db::counts(&conn).unwrap();
    assert_eq!(counts.feeds, vec![(feed_a, 2)]);
    assert_eq!(counts.starred, 1);
    let unread = db::list_articles(&conn, &query(Scope::All, "unread", ""), thumb).unwrap();
    assert_eq!(unread.len(), 2);
    let starred = db::list_articles(&conn, &query(Scope::Starred, "all", ""), thumb).unwrap();
    assert_eq!(starred.len(), 1);
    let marked = db::mark_scope_read(&conn, &Scope::Folder { id: tech }, &db::DateRange::default(), None).unwrap();
    assert_eq!(marked, 2);

    // Tree has display titles and counts.
    let tree = db::tree(&conn).unwrap();
    let a = tree.feeds.iter().find(|f| f.id == feed_a).unwrap();
    assert_eq!(a.title, "Example & Co");
    assert_eq!(a.total, 3);
    assert_eq!(a.unread, 0);

    // Detail and rendering.
    let row = db::article(&conn, ids[2]).unwrap();
    let shown = sanitize::render(row.content.as_deref().unwrap_or(""), true);
    assert!(shown.html.contains("tidings://localhost/img/"));

    // Retention keeps starred articles.
    db::apply_retention(&conn, feed_a, "count", 1).unwrap();
    let left = db::list_articles(&conn, &query(Scope::Feed { id: feed_a }, "all", ""), thumb).unwrap();
    assert_eq!(left.len(), 2, "newest + starred survive");
    assert!(left.iter().any(|a| a.id == ids[1]));

    // Deleted articles do not come back on the next refresh.
    db::delete_articles(&conn, &[ids[0]]).unwrap();
    let tx = conn.transaction().unwrap();
    let added = db::store_articles(&tx, feed_a, &parsed.articles, None).unwrap();
    tx.commit().unwrap();
    assert_eq!(added, 0);

    // Moving: feed reordering and folder cycle protection.
    db::move_feed(&conn, feed_b, Some(rust), Some(Item::Feed { id: feed_a })).unwrap();
    let tree = db::tree(&conn).unwrap();
    let b = tree.feeds.iter().find(|f| f.id == feed_b).unwrap();
    let a = tree.feeds.iter().find(|f| f.id == feed_a).unwrap();
    assert_eq!(b.folder_id, Some(rust));
    assert!(b.position < a.position);
    assert!(db::move_folder(&conn, tech, Some(rust), None).is_err());
    db::move_folder(&conn, rust, None, None).unwrap();
    assert_eq!(db::folders(&conn).unwrap().iter().find(|f| f.id == rust).unwrap().parent_id, None);

    // Feed properties.
    db::update_feed(
        &conn,
        feed_a,
        &db::FeedPatch {
            custom_title: Some("  My Example ".into()),
            url: url.to_string(),
            folder_id: Some(tech),
            update_interval: Some(30),
            retention_mode: Some("days".into()),
            retention_value: Some(7),
            full_text: true,
            paused: false,
            type_override: None,
        },
    )
    .unwrap();
    let a = db::feed(&conn, feed_a).unwrap();
    assert_eq!(a.title, "My Example");
    assert_eq!(a.folder_id, Some(tech));
    assert!(a.full_text);

    // OPML round trip.
    let xml = opml::export(&conn).unwrap();
    assert!(xml.contains("xmlUrl=\"https://example.com/feed.xml\""));
    assert!(xml.contains("My Example"));
    let outlines = opml::parse(&xml).unwrap();
    let (conn2, dir2) = temp_db();
    let stats = opml::import(&conn2, &outlines).unwrap();
    assert_eq!(stats.feeds, 2);
    assert_eq!(stats.folders, 2);
    assert!(db::folders(&conn2).unwrap().iter().all(|f| !f.expanded), "imported folders start closed");
    let again = opml::import(&conn2, &outlines).unwrap();
    assert_eq!(again.feeds, 0);
    assert_eq!(again.skipped, 2);

    // Deleting a folder deletes its feeds and their articles.
    db::delete_folder(&conn, tech).unwrap();
    assert!(db::feed(&conn, feed_a).is_err());
    let all = db::list_articles(&conn, &query(Scope::All, "all", ""), thumb).unwrap();
    assert!(all.is_empty());

    db::purge_tombstones(&conn).unwrap();
    drop(conn);
    drop(conn2);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}

#[test]
fn folders_can_be_reordered() {
    let (conn, dir) = temp_db();
    let a = db::create_folder(&conn, "A", None).unwrap();
    let b = db::create_folder(&conn, "B", None).unwrap();
    let c = db::create_folder(&conn, "C", None).unwrap();
    let order = |conn: &rusqlite::Connection| -> Vec<String> {
        let mut f = db::folders(conn).unwrap();
        f.retain(|x| x.parent_id.is_none());
        f.sort_by_key(|x| x.position);
        f.into_iter().map(|x| x.title).collect()
    };
    db::move_folder(&conn, c, None, Some(Item::Folder { id: a })).unwrap(); // C before A
    assert_eq!(order(&conn), ["C", "A", "B"]);
    db::move_folder(&conn, c, None, Some(Item::Folder { id: b })).unwrap(); // C between A and B
    assert_eq!(order(&conn), ["A", "C", "B"]);
    db::move_folder(&conn, a, None, None).unwrap(); // A to the end
    assert_eq!(order(&conn), ["C", "B", "A"]);
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

/// Top-level children in display order, as "kind:title".
fn root_order(conn: &rusqlite::Connection) -> Vec<String> {
    let mut out: Vec<(i64, u8, String)> = Vec::new();
    for f in db::folders(conn).unwrap().into_iter().filter(|f| f.parent_id.is_none()) {
        out.push((f.position, 0, format!("folder:{}", f.title)));
    }
    for f in db::feeds(conn).unwrap().into_iter().filter(|f| f.folder_id.is_none()) {
        out.push((f.position, 1, format!("feed:{}", f.title)));
    }
    out.sort();
    out.into_iter().map(|(_, _, s)| s).collect()
}

#[test]
fn feeds_and_folders_share_one_order() {
    let (conn, dir) = temp_db();
    let tech = db::create_folder(&conn, "Tech", None).unwrap();
    let news = db::create_folder(&conn, "News", None).unwrap();
    let jb = db::insert_feed(&conn, "https://jb.example/rss", None, Some("Jupiter"), None).unwrap();
    assert_eq!(root_order(&conn), ["folder:Tech", "folder:News", "feed:Jupiter"]);

    // A feed can go to the very top, above every folder.
    db::move_feed(&conn, jb, None, Some(Item::Folder { id: tech })).unwrap();
    assert_eq!(root_order(&conn), ["feed:Jupiter", "folder:Tech", "folder:News"]);
    // ...or between two folders.
    db::move_feed(&conn, jb, None, Some(Item::Folder { id: news })).unwrap();
    assert_eq!(root_order(&conn), ["folder:Tech", "feed:Jupiter", "folder:News"]);
    // A folder can go below a feed.
    db::move_folder(&conn, tech, None, None).unwrap();
    assert_eq!(root_order(&conn), ["feed:Jupiter", "folder:News", "folder:Tech"]);
    // New items go to the end of the shared order.
    db::insert_feed(&conn, "https://late.example/rss", None, Some("Late"), None).unwrap();
    assert_eq!(root_order(&conn).last().unwrap(), "feed:Late");

    // The OPML export keeps the mixed order.
    let xml = opml::export(&conn).unwrap();
    let pos = |needle: &str| xml.find(needle).unwrap();
    assert!(pos("Jupiter") < pos("News") && pos("News") < pos("Tech") && pos("Tech") < pos("Late"));

    // Favorites.
    db::set_pinned(&conn, Item::Feed { id: jb }, true).unwrap();
    db::set_pinned(&conn, Item::Folder { id: news }, true).unwrap();
    let tree = db::tree(&conn).unwrap();
    assert!(tree.feeds.iter().find(|f| f.id == jb).unwrap().pinned > 0);
    assert!(tree.folders.iter().find(|f| f.id == news).unwrap().pinned > 0);
    db::set_pinned(&conn, Item::Feed { id: jb }, false).unwrap();
    assert_eq!(db::feed(&conn, jb).unwrap().pinned, 0);
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn migration_keeps_the_old_look() {
    // Before v2, folders and feeds were numbered separately, both from 0.
    let (conn, dir) = temp_db();
    let a = db::create_folder(&conn, "A", None).unwrap();
    let b = db::create_folder(&conn, "B", None).unwrap();
    let f = db::insert_feed(&conn, "https://f.example/rss", None, Some("F"), None).unwrap();
    conn.execute("UPDATE folders SET position = 0 WHERE id = ?1", [a]).unwrap();
    conn.execute("UPDATE folders SET position = 1 WHERE id = ?1", [b]).unwrap();
    conn.execute("UPDATE feeds SET position = 0 WHERE id = ?1", [f]).unwrap();
    db::normalize_positions(&conn).unwrap();
    // Folders stay first, as the old sidebar showed them.
    assert_eq!(root_order(&conn), ["folder:A", "folder:B", "feed:F"]);
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 4);
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

fn item(guid: &str, published: i64, enclosure: Option<(&str, &str)>) -> db::NewArticle {
    db::NewArticle {
        guid: guid.into(),
        url: Some(format!("https://example.com/{guid}")),
        title: guid.into(),
        author: None,
        excerpt: String::new(),
        content: String::new(),
        image_url: None,
        enclosure_url: enclosure.map(|(_, url)| url.into()),
        enclosure_type: enclosure.map(|(mime, _)| mime.into()),
        published: Some(published),
        hash: published,
    }
}

fn store(conn: &mut rusqlite::Connection, feed: i64, items: &[db::NewArticle]) {
    let tx = conn.transaction().unwrap();
    db::store_articles(&tx, feed, items, None).unwrap();
    db::detect_feed_type(&tx, feed).unwrap();
    tx.commit().unwrap();
}

fn titles(conn: &rusqlite::Connection, q: &ListQuery) -> Vec<String> {
    let mut t: Vec<String> = db::list_articles(conn, q, |u| u.to_string()).unwrap().into_iter().map(|a| a.title).collect();
    t.sort();
    t
}

#[test]
fn feed_types_and_date_ranges() {
    let (mut conn, dir) = temp_db();
    let day = 86_400;
    let t0 = db::now() - 30 * day;

    // Types from the address are known as soon as the feed is added.
    let code = db::insert_feed(&conn, "https://github.com/tauri-apps/tauri/releases.atom", None, None, None).unwrap();
    let video = db::insert_feed(&conn, "https://www.youtube.com/feeds/videos.xml?channel_id=UC1", None, None, None).unwrap();
    let show = db::insert_feed(&conn, "https://shows.example/all", None, Some("Show"), None).unwrap();
    let blog = db::insert_feed(&conn, "https://blog.example/feed", None, Some("Blog"), None).unwrap();
    let type_of = |conn: &rusqlite::Connection, id: i64| db::feed(conn, id).unwrap().feed_type;
    assert_eq!(type_of(&conn, code), "code");
    assert_eq!(type_of(&conn, video), "video");
    assert_eq!(type_of(&conn, show), "article");

    // A feed whose items carry audio becomes a podcast after its refresh.
    fn mp3(url: &str) -> (&str, &str) {
        ("audio/mpeg", url)
    }
    store(&mut conn, show, &[item("ep1", t0, Some(mp3("https://cdn.example/1.mp3"))), item("ep2", t0 + 10 * day, Some(mp3("https://cdn.example/2.mp3")))]);
    store(&mut conn, blog, &[item("post1", t0 + day, None), item("post2", t0 + 20 * day, Some(mp3("https://cdn.example/x.mp3"))), item("post3", t0 + 25 * day, None)]);
    store(&mut conn, code, &[item("v1.0", t0 + 2 * day, None)]);
    assert_eq!(type_of(&conn, show), "podcast");
    assert_eq!(type_of(&conn, blog), "article", "one episode among posts is still a blog");

    let in_type = |t: &str| query(Scope::Type { feed_type: t.into() }, "all", "");
    assert_eq!(titles(&conn, &in_type("podcast")), ["ep1", "ep2"]);
    assert_eq!(titles(&conn, &in_type("code")), ["v1.0"]);
    assert!(titles(&conn, &in_type("video")).is_empty());
    assert!(titles(&conn, &in_type("nonsense")).is_empty(), "unknown types match nothing");

    // The user's choice wins over detection, and survives the next detection.
    let mut patch = db::FeedPatch {
        custom_title: None,
        url: "https://blog.example/feed".into(),
        folder_id: None,
        update_interval: None,
        retention_mode: None,
        retention_value: None,
        full_text: false,
        paused: false,
        type_override: Some("newsletter".into()),
    };
    db::update_feed(&conn, blog, &patch).unwrap();
    db::detect_feed_type(&conn, blog).unwrap();
    let b = db::feed(&conn, blog).unwrap();
    assert_eq!((b.feed_type.as_str(), b.type_override.as_deref(), b.detected_type.as_str()), ("newsletter", Some("newsletter"), "article"));
    assert_eq!(titles(&conn, &in_type("newsletter")), ["post1", "post2", "post3"]);
    // An invalid choice is ignored; None goes back to automatic.
    patch.type_override = Some("bogus".into());
    db::update_feed(&conn, blog, &patch).unwrap();
    assert_eq!(type_of(&conn, blog), "article");

    // Date ranges: since is inclusive, until is exclusive, both combine with scope and filter.
    let ranged = |scope: Scope, since: Option<i64>, until: Option<i64>| {
        let mut q = query(scope, "all", "");
        q.range = db::DateRange { since, until };
        q
    };
    assert_eq!(titles(&conn, &ranged(Scope::All, Some(t0 + 10 * day), None)), ["ep2", "post2", "post3"]);
    assert_eq!(titles(&conn, &ranged(Scope::All, None, Some(t0 + day))), ["ep1"]);
    assert_eq!(titles(&conn, &ranged(Scope::All, Some(t0 + day), Some(t0 + 10 * day))), ["post1", "v1.0"]);
    assert_eq!(titles(&conn, &ranged(Scope::Type { feed_type: "podcast".into() }, Some(t0 + day), None)), ["ep2"]);
    let ids = db::list_article_ids(&conn, &ranged(Scope::All, None, Some(t0 + day))).unwrap();
    assert_eq!(ids.len(), 1);

    // "Mark all as read" honors the range: clear everything older than 20 days ago.
    let older = db::DateRange { since: None, until: Some(t0 + 20 * day) };
    assert_eq!(db::mark_scope_read(&conn, &Scope::All, &older, None).unwrap(), 4);
    assert_eq!(titles(&conn, &query(Scope::All, "unread", "")), ["post2", "post3"]);
    // Both podcast episodes were in that range, so the podcast scope has nothing left.
    assert_eq!(db::mark_scope_read(&conn, &Scope::Type { feed_type: "podcast".into() }, &db::DateRange::default(), None).unwrap(), 0);

    // One type inside a folder (the type icons above the list). The list and
    // "mark all as read" cover the same articles.
    let mixed = db::create_folder(&conn, "Mixed", None).unwrap();
    db::move_feed(&conn, show, Some(mixed), None).unwrap();
    db::move_feed(&conn, blog, Some(mixed), None).unwrap();
    let typed = |filter: &str, t: Option<&str>| {
        let mut q = query(Scope::Folder { id: mixed }, filter, "");
        q.feed_type = t.map(Into::into);
        q
    };
    assert_eq!(titles(&conn, &typed("all", None)).len(), 5);
    assert_eq!(titles(&conn, &typed("all", Some("podcast"))), ["ep1", "ep2"]);
    assert_eq!(titles(&conn, &typed("all", Some("article"))), ["post1", "post2", "post3"]);
    assert!(titles(&conn, &typed("all", Some("code"))).is_empty(), "the code feed is not in the folder");
    assert!(titles(&conn, &typed("all", Some("nonsense"))).is_empty());
    assert_eq!(titles(&conn, &typed("unread", Some("article"))), ["post2", "post3"]);
    let folder = Scope::Folder { id: mixed };
    assert_eq!(db::mark_scope_read(&conn, &folder, &db::DateRange::default(), Some("podcast")).unwrap(), 0);
    assert_eq!(db::mark_scope_read(&conn, &folder, &db::DateRange::default(), Some("article")).unwrap(), 2);

    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn migration_detects_types_of_existing_feeds() {
    // A v2 database: no type columns yet, feeds and their articles already stored.
    let (conn, dir) = temp_db();
    let path = dir.join("test.db");
    let pod = db::insert_feed(&conn, "https://shows.example/all", None, Some("Show"), None).unwrap();
    db::insert_feed(&conn, "https://codeberg.org/forgejo/forgejo/releases.rss", None, None, None).unwrap();
    db::insert_feed(&conn, "https://blog.example/feed", None, None, None).unwrap();
    conn.execute(
        "INSERT INTO articles (feed_id, guid, title, enclosure_url, enclosure_type, published, fetched_at, last_seen)
         VALUES (?1, 'e1', 'Episode', 'https://cdn.example/e1.mp3', 'audio/mpeg', 1, 1, 1)",
        [pod],
    )
    .unwrap();
    conn.execute_batch(
        "ALTER TABLE feeds DROP COLUMN type_override;
         ALTER TABLE feeds DROP COLUMN detected_type;
         PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(conn);

    let conn = db::open(&path).unwrap();
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 4);
    let mut types: Vec<(String, String)> = db::feeds(&conn).unwrap().into_iter().map(|f| (f.url, f.feed_type)).collect();
    types.sort();
    assert_eq!(
        types,
        [
            ("https://blog.example/feed".to_string(), "article".to_string()),
            ("https://codeberg.org/forgejo/forgejo/releases.rss".to_string(), "code".to_string()),
            ("https://shows.example/all".to_string(), "podcast".to_string()),
        ]
    );
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn bulk_delete_and_expand() {
    let (conn, dir) = temp_db();
    let tech = db::create_folder(&conn, "Tech", None).unwrap();
    let rust = db::create_folder(&conn, "Rust", Some(tech)).unwrap();
    let news = db::create_folder(&conn, "News", None).unwrap();
    let a = db::insert_feed(&conn, "https://a.example/feed", Some(rust), None, None).unwrap();
    let b = db::insert_feed(&conn, "https://b.example/feed", Some(news), None, None).unwrap();
    let c = db::insert_feed(&conn, "https://c.example/feed", None, None, None).unwrap();
    let open = |conn: &rusqlite::Connection| {
        let mut ids: Vec<i64> = db::folders(conn).unwrap().iter().filter(|f| f.expanded).map(|f| f.id).collect();
        ids.sort();
        ids
    };

    // New folders are open; close all, then open two.
    assert_eq!(open(&conn), vec![tech, rust, news]);
    db::set_folders_expanded(&conn, None, false).unwrap();
    assert!(open(&conn).is_empty());
    db::set_folders_expanded(&conn, Some(&[tech, rust]), true).unwrap();
    assert_eq!(open(&conn), vec![tech, rust]);

    // One call deletes a folder with its subfolder and feed, plus a loose feed. A feed
    // already gone with its folder, and one that never existed, are skipped.
    let deleted = db::delete_items(
        &conn,
        &[Item::Folder { id: tech }, Item::Feed { id: c }, Item::Feed { id: a }, Item::Feed { id: 9999 }],
    )
    .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(db::feeds(&conn).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(), vec![b]);
    assert_eq!(db::folders(&conn).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(), vec![news]);

    // Starting over removes everything.
    db::delete_all_subscriptions(&conn).unwrap();
    assert!(db::feeds(&conn).unwrap().is_empty());
    assert!(db::folders(&conn).unwrap().is_empty());
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_validation() {
    let mut s = crate::settings::Settings::default();
    s.proxy = "ftp://nope".into();
    assert!(s.clone().validated().is_err());
    s.proxy = "socks5h://127.0.0.1:9050".into();
    s.reader_size = 99;
    s.theme = "neon".into();
    s.video_player = "  /opt/homebrew/bin/mpv \n".into();
    let v = s.clone().validated().unwrap();
    assert_eq!(v.reader_size, 28);
    assert_eq!(v.theme, "system");
    assert_eq!(v.video_player, "/opt/homebrew/bin/mpv");
    s.video_player = "mpv\u{0}--script=x".into();
    assert!(s.validated().is_err());
}

#[test]
fn image_host_checks() {
    use crate::net::host_looks_public;
    let ok = |u: &str| host_looks_public(&Url::parse(u).unwrap());
    assert!(ok("https://images.example.com/a.png"));
    assert!(!ok("http://127.0.0.1/a.png"));
    assert!(!ok("http://192.168.1.1/a.png"));
    assert!(!ok("http://[::1]/a.png"));
    assert!(!ok("http://localhost:8080/a.png"));
    assert!(!ok("http://printer.local/a.png"));
    assert!(!ok("http://169.254.169.254/latest/meta-data"));
}

#[test]
fn local_names() {
    use crate::net::{host_looks_public, is_local_name, same_host};
    for name in ["localhost", "nas", "router.lan", "printer.local", "box.home.arpa", "db.internal", "LOCALHOST."] {
        assert!(is_local_name(name), "{name}");
    }
    for name in ["example.com", "blog.rust-lang.org", "localhost.example.com"] {
        assert!(!is_local_name(name), "{name}");
    }
    let u = |s: &str| Url::parse(s).unwrap();
    assert!(!host_looks_public(&u("http://nas:8080/feed")));
    assert!(!host_looks_public(&u("http://10.0.0.5/feed")));
    assert!(!host_looks_public(&u("http://[fd00::1]/feed")));
    assert!(!host_looks_public(&u("http://100.64.0.1/feed"))); // carrier-grade NAT, Tailscale
    assert!(host_looks_public(&u("https://example.com/feed")));
    assert!(same_host(&u("http://10.0.0.5/feed"), &u("https://10.0.0.5:8443/story")));
    assert!(!same_host(&u("http://10.0.0.5/feed"), &u("http://10.0.0.6/feed")));
}

/// A tiny HTTP server on 127.0.0.1 for the private-network tests.
/// `/ok` answers 200; `/redirect?<url>` answers 302 to `<url>`.
async fn test_server() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let reply = if let Some(to) = path.strip_prefix("/redirect?") {
                    format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string()
                };
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    port
}

/// Serves a `size`-byte file (byte `i` is `i % 251`) with byte ranges, sending at most
/// `cap` bytes per request, like some real hosts. Returns the port and a request count.
async fn range_server(size: usize, cap: usize) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let range = head.lines().find_map(|l| l.strip_prefix("range: bytes=")).unwrap_or("0-").trim().to_string();
                let (a, b) = range.split_once('-').unwrap_or(("0", ""));
                let start: usize = a.parse().unwrap_or(0);
                let end = b.parse().unwrap_or(usize::MAX).min(size - 1).min(start + cap - 1);
                let reply = if start >= size {
                    format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
                } else {
                    let mut r = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes {start}-{end}/{size}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        end - start + 1
                    )
                    .into_bytes();
                    r.extend((start..=end).map(|i| (i % 251) as u8));
                    r
                };
                let _ = stream.write_all(&reply).await;
            });
        }
    });
    (port, hits)
}

#[tokio::test]
async fn media_blocks() {
    use crate::media::{fetch, Media, CHUNK};
    use std::sync::atomic::Ordering;
    let mut net = crate::net::Net::new(&crate::settings::Settings::default()).unwrap();
    net.image = net.local.clone(); // the test server is on 127.0.0.1
    let size = (CHUNK * 5 + 1234) as usize;
    let (port, hits) = range_server(size, CHUNK as usize / 3).await;
    let url = format!("http://127.0.0.1:{port}/v.mp4");
    let media = std::sync::Arc::new(Media::default());
    let expect = |start: usize, len: usize| (start..start + len).map(|i| (i % 251) as u8).collect::<Vec<u8>>();
    let settle = || async {
        // Wait until background read-ahead stops sending requests.
        let mut last = usize::MAX;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let now = hits.load(Ordering::SeqCst);
            if now == last {
                break;
            }
            last = now;
        }
    };

    // A tiny probe, then a request late in block 0 that continues into block 1. The
    // host sends a third of a block per request; blocks are still filled completely.
    let c = fetch(&net, &media, &url, Some("bytes=0-1")).await.unwrap();
    assert_eq!((c.status, c.bytes.clone()), (206, expect(0, 2)));
    assert_eq!(c.content_range, Some(format!("bytes 0-1/{size}")));
    let c = fetch(&net, &media, &url, Some("bytes=1000-")).await.unwrap();
    assert_eq!(c.bytes, expect(1000, CHUNK as usize));
    assert_eq!(c.mime, "video/mp4");

    // Read-ahead fetched the next blocks, so reading on needs no new requests.
    settle().await;
    let before = hits.load(Ordering::SeqCst);
    let start = 2 * CHUNK as usize;
    let c = fetch(&net, &media, &url, Some(&format!("bytes={start}-"))).await.unwrap();
    assert_eq!(c.bytes, expect(start, CHUNK as usize));
    assert_eq!(hits.load(Ordering::SeqCst), before, "block 2 should come from memory");

    // The end of the file: a short answer, then 416 past it.
    let c = fetch(&net, &media, &url, Some(&format!("bytes={}-", size - 10))).await.unwrap();
    assert_eq!(c.bytes, expect(size - 10, 10));
    assert_eq!(c.content_range, Some(format!("bytes {}-{}/{size}", size - 10, size - 1)));
    let c = fetch(&net, &media, &url, Some(&format!("bytes={size}-"))).await.unwrap();
    assert_eq!(c.status, 416);
}

#[tokio::test]
async fn private_addresses_need_a_trusted_host() {
    let net = crate::net::Net::new(&crate::settings::Settings::default()).unwrap();
    let port = test_server().await;
    let lan_feed = Url::parse(&format!("http://127.0.0.1:{port}/ok")).unwrap();
    let public_feed = Url::parse("https://example.com/feed.xml").unwrap();
    let fetch = |url: Url, trusted: Option<Url>| {
        let net = net.clone();
        async move {
            let response = net.get(&url, trusted.as_ref()).await?.send().await?;
            Ok::<String, crate::error::Error>(response.text().await?)
        }
    };
    let blocked = |r: crate::error::Result<String>| {
        let err = r.expect_err("request should be blocked").to_string();
        assert!(err.starts_with("Blocked"), "{err}");
    };

    // A feed the user added on a private host works.
    assert_eq!(fetch(lan_feed.clone(), Some(lan_feed.clone())).await.unwrap(), "ok");

    // A link to that host from a public feed, or from nowhere, does not.
    blocked(fetch(lan_feed.clone(), Some(public_feed)).await);
    blocked(fetch(lan_feed.clone(), None).await);

    // The public client also refuses it when a redirect leads there.
    blocked(net.web.get(format!("http://127.0.0.1:{port}/redirect?http://10.0.0.1/")).send().await.map_err(Into::into).map(|_| String::new()));

    // A private feed may redirect within its own host, but not to another private host.
    let same = Url::parse(&format!("http://127.0.0.1:{port}/redirect?http://127.0.0.1:{port}/ok")).unwrap();
    assert_eq!(fetch(same, Some(lan_feed.clone())).await.unwrap(), "ok");
    let other = Url::parse(&format!("http://127.0.0.1:{port}/redirect?http://localhost:{port}/ok")).unwrap();
    blocked(fetch(other, Some(lan_feed)).await);
}

#[tokio::test]
async fn proxy_makes_no_dns_lookups() {
    let mut settings = crate::settings::Settings::default();
    settings.proxy = "socks5h://127.0.0.1:9".into();
    let net = crate::net::Net::new(&settings).unwrap();
    let u = |s: &str| Url::parse(s).unwrap();
    // Literal addresses and local names are still recognized without DNS.
    assert!(net.is_private_host(&u("http://192.168.1.1/")).await);
    assert!(net.is_private_host(&u("http://nas/")).await);
    // Other names are not looked up, so they count as public and go through the proxy.
    assert!(!net.is_private_host(&u("http://only-resolvable-on-the-lan.example/")).await);
}

#[test]
fn sniffs_images_only() {
    use crate::net::sniff_image;
    assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n....", "").as_deref(), Some("image/png"));
    assert_eq!(sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>", "image/svg+xml").as_deref(), Some("image/svg+xml"));
    assert_eq!(sniff_image(b"<!doctype html><html>", "image/png"), None);
}

#[test]
fn reader_mode_extracts_article() {
    let html = r#"<!doctype html><html><head><title>Story | Site</title></head><body>
      <nav><a href="/">Home</a><a href="/about">About</a></nav>
      <article><h1>The Story</h1>
      <p>This is the first paragraph of a long article that has enough text to be considered readable content by the extraction algorithm used in reader mode.</p>
      <p>Here is a second paragraph, also fairly long, with a <a href="/more?utm_medium=x">link</a> and more words to make the scoring happy and confident.</p>
      <p>And a third paragraph to be safe, because readability likes several paragraphs of real prose in the same container element.</p>
      <script>track()</script></article>
      <footer>Copyright</footer></body></html>"#;
    let url = Url::parse("https://site.example/story").unwrap();
    let page = crate::reader::extract_html(html, &url, true).unwrap();
    assert!(page.html.contains("first paragraph"));
    assert!(!page.html.contains("track()"));
    assert!(page.html.contains("https://site.example/more"));
    assert!(!page.html.contains("utm_medium"));
}

/// Talks to real servers. Run with: cargo test -- --ignored live_network
#[tokio::test]
#[ignore]
async fn live_network() {
    let net = crate::net::Net::new(&crate::settings::Settings::default()).unwrap();

    // Autodiscovery from a home page, then fetch and parse the feed.
    let found = crate::discover::discover(&net, "blog.rust-lang.org").await.unwrap();
    assert!(!found.is_empty(), "no feed discovered");
    let feed_url = Url::parse(&found[0].url).unwrap();
    let resp = net.web.get(feed_url.as_str()).send().await.unwrap();
    assert!(resp.status().is_success());
    let bytes = crate::net::read_limited(resp, crate::net::MAX_FEED_BYTES).await.unwrap();
    let parsed = feeds::parse_feed(&bytes, &feed_url, true).unwrap();
    assert!(parsed.articles.len() > 3);
    println!("feed: {:?}, {} articles", parsed.title, parsed.articles.len());

    // Favicon lookup.
    let icon = feeds::fetch_icon(&net, parsed.site_url.as_deref(), parsed.icon_url.as_deref(), &feed_url).await;
    println!("icon: {:?}", icon.as_ref().map(|(b, m)| (b.len(), m.clone())));

    // Reader mode on the newest article.
    let link = parsed.articles.iter().find_map(|a| a.url.clone()).unwrap();
    let page = crate::reader::extract_url(&net, &link, None, true).await.unwrap();
    assert!(page.html.len() > 500);
    println!("reader: {} ({} bytes)", page.title, page.html.len());

    // Podcast audio: tracker stripping and a ranged slice through the media proxy.
    let cache = std::sync::Arc::new(crate::media::Media::default());
    let lup = crate::media::strip_trackers(
        "https://dts.podtrac.com/redirect.mp3/mgln.ai/e/211/rss.art19.com/episodes/b6a6899c-93e0-412a-9222-6ad13baca403.mp3",
    );
    assert!(lup.starts_with("https://rss.art19.com/"));
    let chunk = crate::media::fetch(&net, &cache, &lup, Some("bytes=1000-")).await.unwrap();
    assert_eq!(chunk.status, 206);
    assert_eq!(chunk.bytes.len() as u64, crate::media::CHUNK);
    println!("audio slice: {} bytes, {:?}, {}", chunk.bytes.len(), chunk.content_range, chunk.mime);

    // A large podcast feed (about 19 MB) parses and keeps its audio enclosures.
    let bsd = Url::parse("https://feeds.fireside.fm/bsdnow/rss").unwrap();
    let resp = net.web.get(bsd.as_str()).send().await.unwrap();
    let bytes = crate::net::read_limited(resp, crate::net::MAX_FEED_BYTES).await.unwrap();
    let started = std::time::Instant::now();
    let podcast = feeds::parse_feed(&bytes, &bsd, true).unwrap();
    let with_audio = podcast.articles.iter().filter(|a| a.enclosure_type.as_deref().is_some_and(|t| t.starts_with("audio/"))).count();
    println!("bsdnow: {} episodes, {} with audio, parsed in {:?}", podcast.articles.len(), with_audio, started.elapsed());
    assert!(with_audio > 100);

    // The image client's resolver must refuse names that point to private addresses.
    // (Checked on the resolver itself, because a proxy from the environment would
    // resolve names on its own.)
    assert!(crate::net::resolve_public("localhost.localtest.me").await.is_err());
    assert!(crate::net::resolve_public("blog.rust-lang.org").await.is_ok());
}

/// Migrates a copy of a real database: TIDINGS_DB_COPY=/path cargo test -- --ignored real_db
#[test]
#[ignore]
fn real_db_migration() {
    let path = std::env::var("TIDINGS_DB_COPY").expect("set TIDINGS_DB_COPY");
    // (kind, id, parent) in display order, per parent, before the upgrade.
    let order_sql = |version: i64| {
        if version >= 2 {
            // One shared order for feeds and folders since v2.
            "SELECT 'folder', id, parent_id, position FROM folders
             UNION ALL SELECT 'feed', id, folder_id, position FROM feeds
             ORDER BY 3, 4, 1"
        } else {
            "SELECT 'folder', id, parent_id, position FROM folders
             UNION ALL SELECT 'feed', id, folder_id, position + 1000000 FROM feeds
             ORDER BY 3, 4, 1"
        }
    };
    let read_order = |c: &rusqlite::Connection, version: i64| -> Vec<(String, i64, Option<i64>)> {
        let mut s = c.prepare(order_sql(version)).unwrap();
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|x| x.unwrap()).collect()
    };
    let (old_version, before) = {
        let c = rusqlite::Connection::open(&path).unwrap();
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        (v, read_order(&c, v))
    };
    let conn = db::open(std::path::Path::new(&path)).unwrap();
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 4);
    let tree = db::tree(&conn).unwrap();

    // Nothing moved. (From v1, folders come first in each parent: the old look.)
    let after = read_order(&conn, v);
    assert_eq!(before.len(), after.len());
    let in_parent = |list: &[(String, i64, Option<i64>)], p: Option<i64>| {
        list.iter().filter(|x| x.2 == p).map(|x| (x.0.clone(), x.1)).collect::<Vec<_>>()
    };
    for item in &before {
        assert_eq!(in_parent(&before, item.2), in_parent(&after, item.2), "order changed under {:?}", item.2);
    }
    if old_version < 2 {
        for parent in tree.folders.iter().map(|f| Some(f.id)).chain([None]) {
            let max_folder = tree.folders.iter().filter(|f| f.parent_id == parent).map(|f| f.position).max();
            let min_feed = tree.feeds.iter().filter(|f| f.folder_id == parent).map(|f| f.position).min();
            if let (Some(a), Some(b)) = (max_folder, min_feed) {
                assert!(a < b, "feeds above folders under {parent:?}");
            }
        }
    }

    // Every feed has a known type, and nobody has an override yet.
    let mut types = std::collections::BTreeMap::<String, usize>::new();
    for feed in &tree.feeds {
        assert!(crate::feed_types::is_type(&feed.feed_type), "{}: {}", feed.url, feed.feed_type);
        if old_version < 3 {
            assert_eq!(feed.type_override, None);
        }
        *types.entry(feed.feed_type.clone()).or_default() += 1;
    }
    println!("migrated from v{old_version}: {} folders, {} feeds, types {types:?}", tree.folders.len(), tree.feeds.len());
}

/// PeerTube against real servers: search Sepia Search, ask the instance for a file,
/// and read the first slice through the media proxy.
/// Run with: cargo test -- --ignored live_peertube
#[tokio::test]
#[ignore]
async fn live_peertube() {
    let net = crate::net::Net::new(&crate::settings::Settings::default()).unwrap();
    let found = crate::peertube::search(&net, "peertube", true).await.unwrap();
    assert!(!found.is_empty(), "no search results");
    println!("search: {} results, first: {} ({})", found.len(), found[0].title, found[0].host);

    // Not every instance in the index is online; use the first that answers.
    let mut file = None;
    for hit in found.iter().take(5) {
        match crate::peertube::video(&net, &Url::parse(&hit.url).unwrap()).await {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) => println!("skipped {}: {e}", hit.host),
        }
    }
    let file = file.expect("no instance answered");
    println!("file: {} {} ({} s)", file.label, file.url, file.duration);

    let cache = std::sync::Arc::new(crate::media::Media::default());
    let chunk = crate::media::fetch(&net, &cache, &file.url, Some("bytes=0-")).await.unwrap();
    assert_eq!(chunk.status, 206);
    assert!(chunk.mime.starts_with("video/"), "{}", chunk.mime);
    println!("media: {} bytes, {}, {:?}", chunk.bytes.len(), chunk.mime, chunk.content_range);
}

/// "Listen" end to end without the UI: yt-dlp finds the audio of a 19-second YouTube
/// video, and the media proxy reads it in byte ranges, as the podcast player does.
/// Needs yt-dlp: `cargo test live_listen_through_proxy -- --ignored`
#[tokio::test]
#[ignore]
async fn live_listen_through_proxy() {
    let page = Url::parse("https://www.youtube.com/watch?v=jNQXAC9IVRw").unwrap();
    let stream = crate::video::audio_stream(&page, "").await.unwrap();
    let net = crate::net::Net::new(&crate::settings::Settings::default()).unwrap();
    let cache = std::sync::Arc::new(crate::media::Media::default());
    let first = crate::media::fetch(&net, &cache, &stream.url, Some("bytes=0-")).await.unwrap();
    assert_eq!(first.status, 206);
    assert_eq!(first.mime, "audio/mp4");
    // A seek into the middle.
    let middle = crate::media::fetch(&net, &cache, &stream.url, Some("bytes=150000-")).await.unwrap();
    assert_eq!(middle.status, 206);
    println!("{} bytes, then {} bytes from the middle, {:?}", first.bytes.len(), middle.bytes.len(), middle.content_range);
}

fn article(guid: &str, published: i64, excerpt: &str) -> db::NewArticle {
    db::NewArticle { excerpt: excerpt.into(), ..item(guid, published, None) }
}

#[test]
fn brief_sources_and_storage() {
    let (mut conn, dir) = temp_db();
    let t0 = db::now() - 3_600;
    let alpha = db::insert_feed(&conn, "https://alpha.example/rss", None, Some("Alpha"), None).unwrap();
    let beta = db::insert_feed(&conn, "https://beta.example/rss", None, Some("Beta"), None).unwrap();
    store(&mut conn, alpha, &[article("a1", t0, "First."), article("a2", t0 + 100, "Second.")]);
    store(&mut conn, beta, &[article("b1", t0 + 50, "")]);
    let a1 = db::list_articles(&conn, &query(Scope::All, "all", "a1"), |u| u.to_string()).unwrap()[0].id;
    db::set_read(&conn, &[a1], true).unwrap();

    // The same filters as the list, newest first, with feed names and excerpts.
    let (sources, total) = db::brief_sources(&conn, &query(Scope::All, "all", ""), 300).unwrap();
    assert_eq!(total, 3);
    let seen: Vec<(&str, &str, &str)> = sources.iter().map(|s| (s.title.as_str(), s.feed.as_str(), s.excerpt.as_str())).collect();
    assert_eq!(seen, [("a2", "Alpha", "Second."), ("b1", "Beta", ""), ("a1", "Alpha", "First.")]);
    let (unread, total) = db::brief_sources(&conn, &query(Scope::All, "unread", ""), 300).unwrap();
    assert_eq!((unread.len(), total), (2, 2));
    let (newest, total) = db::brief_sources(&conn, &query(Scope::Feed { id: alpha }, "all", ""), 1).unwrap();
    assert_eq!((newest[0].title.as_str(), total), ("a2", 2));

    // Stored briefs: newest first, read back whole, deleted, and only the newest 100 kept.
    let first = db::insert_brief(&conn, "Morning", "qwen3:4b", "{\"q\":1}", "{\"b\":1}").unwrap();
    let second = db::insert_brief(&conn, "Evening", "qwen3:4b", "{}", "{}").unwrap();
    let list = db::list_briefs(&conn, 10).unwrap();
    assert_eq!(list.iter().map(|b| b.id).collect::<Vec<_>>(), [second, first]);
    let (info, q, body) = db::get_brief(&conn, first).unwrap().unwrap();
    assert_eq!((info.title.as_str(), info.model.as_str(), q.as_str(), body.as_str()), ("Morning", "qwen3:4b", "{\"q\":1}", "{\"b\":1}"));
    db::delete_brief(&conn, second).unwrap();
    assert!(db::get_brief(&conn, second).unwrap().is_none());
    for i in 0..105 {
        db::insert_brief(&conn, &format!("B{i}"), "m", "{}", "{}").unwrap();
    }
    assert_eq!(db::list_briefs(&conn, 500).unwrap().len(), 100);
    assert!(db::get_brief(&conn, first).unwrap().is_none());
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

/// A model server on 127.0.0.1 that answers like Ollama (`openai` false) or like an
/// OpenAI-style server that refuses JSON schemas. It keeps the request bodies.
async fn model_server(openai: bool) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = bodies.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                // Read the head, then as much body as Content-Length says.
                loop {
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let length = text[..head_end]
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buf.len() >= head_end + 4 + length {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                seen.lock().unwrap().push(body.to_string());
                let brief = r#"{"sections": [{"title": "Elections", "points": [{"text": "Polls opened and turnout was high.", "sources": [1, 3, 7]}]}]}"#;
                let (status, reply) = match (openai, path.as_str()) {
                    (false, "/api/tags") => ("200 OK", r#"{"models": [{"name": "qwen3:4b"}, {"name": "llama3.2:latest"}]}"#.to_string()),
                    (false, "/api/chat") => ("200 OK", serde_json::json!({"message": {"role": "assistant", "content": brief}, "done": true}).to_string()),
                    (true, "/v1/models") => ("200 OK", r#"{"data": [{"id": "qwen3-4b-instruct"}]}"#.to_string()),
                    (true, "/v1/chat/completions") if body.contains("response_format") => {
                        ("400 Bad Request", r#"{"error": {"message": "response_format is not supported"}}"#.to_string())
                    }
                    (true, "/v1/chat/completions") => {
                        ("200 OK", serde_json::json!({"choices": [{"message": {"content": format!("Sure! {brief}")}}]}).to_string())
                    }
                    _ => ("404 Not Found", r#"{"error": "model 'nope' not found"}"#.to_string()),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (port, bodies)
}

#[tokio::test]
async fn briefs_with_a_local_model() {
    let sources = vec![
        db::BriefSource { id: 501, feed_id: 1, feed: "CBC".into(), title: "Polls open".into(), excerpt: "Voters lined up.".into(), published: 1_794_200_000 },
        db::BriefSource { id: 502, feed_id: 1, feed: "CBC".into(), title: "Snow".into(), excerpt: String::new(), published: 1_794_190_000 },
        db::BriefSource { id: 503, feed_id: 2, feed: "Globe".into(), title: "Turnout high".into(), excerpt: String::new(), published: 1_794_180_000 },
    ];
    for openai in [false, true] {
        let (port, bodies) = model_server(openai).await;
        let settings = crate::settings::Settings {
            ai_provider: if openai { "openai" } else { "ollama" }.into(),
            ai_endpoint: format!("http://127.0.0.1:{port}"),
            ai_model: "qwen3:4b".into(),
            ..Default::default()
        }
        .validated()
        .unwrap();
        let net = crate::net::Net::new(&settings).unwrap();
        let models = crate::brief::models(&net, &settings).await.unwrap();
        assert_eq!(models, if openai { vec!["qwen3-4b-instruct"] } else { vec!["llama3.2:latest", "qwen3:4b"] });

        let (body, model) = crate::brief::write(&net, &settings, "Canadian News", &sources, 40).await.unwrap();
        assert_eq!(model, "qwen3:4b");
        assert_eq!((body.articles_used, body.articles_total), (3, 40));
        assert_eq!(body.sections[0].title, "Elections");
        // Article 7 was never sent, so its citation is dropped.
        assert_eq!(body.sections[0].points[0].sources, [501, 503]);
        assert_eq!(body.sources.iter().map(|s| s.id).collect::<Vec<_>>(), [501, 503]);

        // The model got the titles, feeds and excerpts, and nothing else of the database.
        let sent = bodies.lock().unwrap().last().unwrap().clone();
        assert!(sent.contains("Polls open") && sent.contains("Voters lined up.") && sent.contains("[3] Globe"), "{sent}");
        if openai {
            // The schema was refused once, then the request went without it.
            assert!(bodies.lock().unwrap().iter().filter(|b| b.contains("chat")).count() <= 2);
            assert!(!sent.contains("response_format"));
        } else {
            assert!(sent.contains("\"format\"") && sent.contains("num_ctx"));
        }
    }

    // A model the server does not have: its own words come back.
    let (port, _) = model_server(false).await;
    let settings = crate::settings::Settings {
        ai_provider: "ollama".into(),
        ai_endpoint: format!("http://127.0.0.1:{port}/nope"),
        ai_model: "nope".into(),
        ..Default::default()
    };
    let err = crate::brief::write(&crate::net::Net::new(&settings).unwrap(), &settings, "x", &sources, 3).await.unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");

    // Nobody listening.
    let settings = crate::settings::Settings {
        ai_provider: "ollama".into(),
        ai_endpoint: "http://127.0.0.1:9".into(),
        ai_model: "m".into(),
        ..Default::default()
    };
    let err = crate::brief::models(&crate::net::Net::new(&settings).unwrap(), &settings).await.unwrap_err();
    assert!(err.to_string().starts_with("Could not reach Ollama at http://127.0.0.1:9/"), "{err}");
}

#[test]
fn model_server_must_be_local() {
    let check = |endpoint: &str, proxy: &str| {
        crate::settings::Settings { ai_provider: "ollama".into(), ai_endpoint: endpoint.into(), proxy: proxy.into(), ..Default::default() }
            .validated()
            .map(|s| s.ai_server().unwrap().to_string())
    };
    assert_eq!(check("", "").unwrap(), "http://127.0.0.1:11434/");
    assert!(check("http://192.168.1.20:11434/", "").is_ok());
    assert!(check("http://gpu-box.lan:11434", "").is_ok());
    assert!(check("http://localhost:1234", "socks5h://127.0.0.1:9050").is_ok());
    // A public host would send the articles over the internet.
    assert!(check("https://api.example.com", "").is_err());
    assert!(check("http://8.8.8.8:11434", "").is_err());
    // With a proxy, names would be looked up outside it.
    assert!(check("http://gpu-box.lan:11434", "socks5h://127.0.0.1:9050").is_err());
    assert!(check("ftp://127.0.0.1", "").is_err());
    // Briefs off: no server.
    let off = crate::settings::Settings::default().validated().unwrap();
    assert!(off.ai_server().is_none());
}

/// Writes a brief with a real local model from a copy of a real database, and prints
/// it. Never point it at the live database: it is migrated in place.
/// `TIDINGS_DB_COPY=copy.db TIDINGS_BRIEF_FOLDER="Canadian News" TIDINGS_AI_PROVIDER=openai
///  TIDINGS_AI_ENDPOINT=http://127.0.0.1:8080 TIDINGS_AI_MODEL=qwen3 cargo test live_brief -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_brief() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let conn = db::open(std::path::Path::new(&var("TIDINGS_DB_COPY"))).unwrap();
    let folder: i64 = conn.query_row("SELECT id FROM folders WHERE title = ?1", [var("TIDINGS_BRIEF_FOLDER")], |r| r.get(0)).unwrap();
    let hours: i64 = std::env::var("TIDINGS_BRIEF_HOURS").ok().and_then(|h| h.parse().ok()).unwrap_or(24);
    let mut q = query(Scope::Folder { id: folder }, "all", "");
    q.range = db::DateRange { since: Some(db::now() - hours * 3600), until: None };
    let (sources, total) = db::brief_sources(&conn, &q, crate::brief::MAX_ARTICLES).unwrap();
    let settings = crate::settings::Settings {
        ai_provider: var("TIDINGS_AI_PROVIDER"),
        ai_endpoint: var("TIDINGS_AI_ENDPOINT"),
        ai_model: var("TIDINGS_AI_MODEL"),
        ..Default::default()
    }
    .validated()
    .unwrap();
    let net = crate::net::Net::new(&settings).unwrap();
    let title = format!("{} · last {hours} hours", var("TIDINGS_BRIEF_FOLDER"));
    let (body, model) = crate::brief::write(&net, &settings, &title, &sources, total).await.unwrap();
    println!("\n{title} — {model}, {} of {} articles, {} s\n", body.articles_used, body.articles_total, body.seconds);
    let by_id: std::collections::HashMap<i64, &crate::brief::SourceRef> = body.sources.iter().map(|s| (s.id, s)).collect();
    for section in &body.sections {
        println!("## {}", section.title);
        for point in &section.points {
            let cites: Vec<String> = point.sources.iter().filter_map(|id| by_id.get(id)).map(|s| format!("{}: {}", s.feed, s.title)).collect();
            println!("- {}\n    [{}]", point.text, cites.join(" | "));
        }
    }
}
