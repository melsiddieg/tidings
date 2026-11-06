//! Integration tests that run every database query against a real SQLite file.

use crate::db::{self, ListQuery, Scope};
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
    let dir = std::env::temp_dir().join(format!("tidings-test-{}-{}", std::process::id(), db::now()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.db", rand_suffix()));
    (db::open(&path).unwrap(), dir)
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn query(scope: Scope, filter: &str, q: &str) -> ListQuery {
    ListQuery {
        scope,
        filter: filter.into(),
        query: q.into(),
        oldest_first: false,
        limit: Some(100),
        offset: Some(0),
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
    let marked = db::mark_scope_read(&conn, &Scope::Folder { id: tech }).unwrap();
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
    db::move_feed(&conn, feed_b, Some(rust), Some(feed_a)).unwrap();
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
fn settings_validation() {
    let mut s = crate::settings::Settings::default();
    s.proxy = "ftp://nope".into();
    assert!(s.clone().validated().is_err());
    s.proxy = "socks5h://127.0.0.1:9050".into();
    s.reader_size = 99;
    s.theme = "neon".into();
    let v = s.validated().unwrap();
    assert_eq!(v.reader_size, 28);
    assert_eq!(v.theme, "system");
}

#[test]
fn image_host_checks() {
    use crate::net::host_is_allowed_for_images;
    let ok = |u: &str| host_is_allowed_for_images(&Url::parse(u).unwrap());
    assert!(ok("https://images.example.com/a.png"));
    assert!(!ok("http://127.0.0.1/a.png"));
    assert!(!ok("http://192.168.1.1/a.png"));
    assert!(!ok("http://[::1]/a.png"));
    assert!(!ok("http://localhost:8080/a.png"));
    assert!(!ok("http://printer.local/a.png"));
    assert!(!ok("http://169.254.169.254/latest/meta-data"));
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
    let resp = net.feed.get(feed_url.as_str()).send().await.unwrap();
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
    let page = crate::reader::extract_url(&net, &link, true).await.unwrap();
    assert!(page.html.len() > 500);
    println!("reader: {} ({} bytes)", page.title, page.html.len());

    // The image client's resolver must refuse names that point to private addresses.
    // (Checked on the resolver itself, because a proxy from the environment would
    // resolve names on its own.)
    assert!(crate::net::resolve_public("localhost.localtest.me").await.is_err());
    assert!(crate::net::resolve_public("blog.rust-lang.org").await.is_ok());
}
