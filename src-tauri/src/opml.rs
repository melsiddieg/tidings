//! OPML import and export. Folders map to nested <outline> elements.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer, XmlVersion};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::error::{Error, Result};
use crate::net;

#[derive(Debug, Default)]
pub struct Outline {
    pub title: String,
    pub xml_url: Option<String>,
    pub html_url: Option<String>,
    pub children: Vec<Outline>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStats {
    /// New feeds added.
    pub feeds: usize,
    /// New folders made.
    pub folders: usize,
    /// Subscribed feeds moved to the folder the file puts them in.
    pub moved: usize,
    /// Feeds already subscribed, listed twice, or with an address that cannot be used.
    pub skipped: usize,
    #[serde(skip)]
    pub new_feed_ids: Vec<i64>,
}

/// A feed in an OPML file and the folders it is in, outermost first.
#[derive(Debug, Clone)]
pub struct FileFeed {
    pub url: String,
    pub title: String,
    pub site: Option<String>,
    pub path: Vec<String>,
}

/// The feeds of a file the user chose, kept between the preview and the import.
#[derive(Debug, Default)]
pub struct Pending {
    pub feeds: Vec<FileFeed>,
    /// Outlines whose address cannot be used.
    pub invalid: usize,
}

/// What to do with subscribed feeds that the file puts in another folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    /// Leave them where they are. New feeds still go into the file's folders.
    Keep,
    /// Move them to the file's folders.
    File,
}

/// A subscribed feed that the file puts in another folder.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub title: String,
    pub from: String,
    pub to: String,
}

/// What an import would do, shown before it runs.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub file_name: String,
    /// Feeds in the file with a usable address, each counted once.
    pub in_file: usize,
    pub new_feeds: usize,
    pub subscribed: usize,
    /// Feeds listed more than once in the file.
    pub repeated: usize,
    pub invalid: usize,
    /// The first few new feeds, by title.
    pub new_titles: Vec<String>,
    /// How many subscribed feeds the file puts in another folder, and the first few.
    pub move_count: usize,
    pub moves: Vec<Move>,
    /// Folders the import makes when it keeps your folders, and when it uses the file's.
    pub new_folders_keep: usize,
    pub new_folders_file: usize,
}

/// How many new feeds and moves the preview lists by name.
const PREVIEW_ITEMS: usize = 8;

fn outline_from(e: &BytesStart<'_>) -> Outline {
    let mut out = Outline::default();
    let mut text = String::new();
    // No check for duplicate attribute names: it compares every pair, so one tag
    // with a hundred thousand attributes took minutes. A real outline has a handful.
    if e.attributes_raw().len() > MAX_ATTRIBUTES_BYTES {
        return out;
    }
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref().to_ascii_lowercase();
        let Ok(value) = attr.normalized_value(XmlVersion::Implicit1_0) else { continue };
        let value = value.trim().to_string();
        match key.as_str() {
            "title" => out.title = value,
            "text" => text = value,
            "xmlurl" => out.xml_url = Some(value).filter(|v| !v.is_empty()),
            "htmlurl" => out.html_url = Some(value).filter(|v| !v.is_empty()),
            _ => {}
        }
    }
    if out.title.is_empty() {
        out.title = text;
    }
    out
}

/// The attributes of one outline, at most this many bytes. Longer ones are ignored.
const MAX_ATTRIBUTES_BYTES: usize = 64 * 1024;

/// Folders inside folders, at most this deep. Real lists are a few levels deep; a
/// crafted file with thousands of levels would overflow the stack on import.
const MAX_DEPTH: usize = 32;

pub fn parse(xml: &str) -> Result<Vec<Outline>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    // Stack of open outlines. Index 0 is a virtual root.
    let mut stack: Vec<Outline> = vec![Outline::default()];
    let mut saw_opml = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "opml" => saw_opml = true,
                "outline" => {
                    if stack.len() > MAX_DEPTH {
                        return Err(Error::msg("This OPML file has folders nested too deeply to import"));
                    }
                    stack.push(outline_from(&e));
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == "outline" {
                    let node = outline_from(&e);
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == "outline" && stack.len() > 1 {
                    let node = stack.pop().unwrap_or_default();
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(Error::msg(format!("This OPML file is not valid XML: {e}"))),
        }
    }
    if !saw_opml {
        return Err(Error::msg("This file is not an OPML subscription list"));
    }
    // Close outlines left open by a broken file.
    while stack.len() > 1 {
        let node = stack.pop().unwrap_or_default();
        if let Some(parent) = stack.last_mut() {
            parent.children.push(node);
        }
    }
    Ok(stack.pop().map(|root| root.children).unwrap_or_default())
}

/// The feeds in a parsed file, in file order, with the folders they are in.
pub fn feeds_in(outlines: &[Outline]) -> Pending {
    fn walk(outlines: &[Outline], path: &mut Vec<String>, out: &mut Pending) {
        for node in outlines {
            if let Some(xml_url) = &node.xml_url {
                match net::parse_web_url(xml_url) {
                    Ok(url) => out.feeds.push(FileFeed {
                        url: url.to_string(),
                        title: node.title.clone(),
                        site: node.html_url.as_deref().and_then(|h| net::parse_web_url(h).ok()).map(|u| u.to_string()),
                        path: path.clone(),
                    }),
                    Err(_) => out.invalid += 1,
                }
                // Some exporters nest feeds under a feed. Keep them at the same level.
                walk(&node.children, path, out);
            } else if !node.children.is_empty() {
                path.push(if node.title.is_empty() { "Untitled folder".into() } else { node.title.clone() });
                walk(&node.children, path, out);
                path.pop();
            }
        }
    }
    let mut out = Pending::default();
    walk(outlines, &mut Vec::new(), &mut out);
    out
}

/// Feed addresses that differ only in http or https, a leading "www.", a trailing
/// slash, the case of the host or a #fragment belong to the same feed.
pub fn feed_key(url: &str) -> String {
    let Ok(url) = url::Url::parse(url) else { return url.to_string() };
    let host = url.host_str().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    let query = url.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!("{host}{port}{path}{query}")
}

/// Folder names are matched ignoring case and spaces around them.
fn name_key(title: &str) -> String {
    title.trim().to_lowercase()
}

/// The folders there are, to find a file's folder path among them.
struct Folders {
    /// (id, parent, name key, title), in sidebar order.
    list: Vec<(i64, Option<i64>, String, String)>,
}

impl Folders {
    fn load(conn: &Connection) -> Result<Self> {
        let list = db::folders(conn)?.into_iter().map(|f| (f.id, f.parent_id, name_key(&f.title), f.title)).collect();
        Ok(Folders { list })
    }

    fn child(&self, parent: Option<i64>, title: &str) -> Option<i64> {
        let key = name_key(title);
        self.list.iter().find(|(_, p, k, _)| *p == parent && *k == key).map(|(id, ..)| *id)
    }

    /// The folder at `path`: `Ok(None)` for the top level, `Err(n)` when the last
    /// `n` folders of the path do not exist.
    fn find(&self, path: &[String]) -> std::result::Result<Option<i64>, usize> {
        let mut parent = None;
        for (i, title) in path.iter().enumerate() {
            match self.child(parent, title) {
                Some(id) => parent = Some(id),
                None => return Err(path.len() - i),
            }
        }
        Ok(parent)
    }

    /// "Tech / Linux" for a folder, "No folder" for the top level.
    fn name(&self, mut id: Option<i64>) -> String {
        let mut names = Vec::new();
        while let Some(current) = id {
            let Some((_, parent, _, title)) = self.list.iter().find(|(i, ..)| *i == current) else { break };
            names.push(title.as_str());
            id = *parent;
            if names.len() > MAX_DEPTH {
                break;
            }
        }
        names.reverse();
        if names.is_empty() { "No folder".into() } else { names.join(" / ") }
    }

    /// The folder at `path`, made where missing: new folders go at the end of their
    /// parent and start closed.
    fn make(&mut self, conn: &Connection, path: &[String], stats: &mut ImportStats) -> Result<Option<i64>> {
        let mut parent = None;
        for title in path {
            parent = Some(match self.child(parent, title) {
                Some(id) => id,
                None => {
                    let id = db::create_folder(conn, title.trim(), parent)?;
                    // Imported folders start closed; a large import would fill the sidebar.
                    db::set_folder_expanded(conn, id, false)?;
                    self.list.push((id, parent, name_key(title), title.trim().to_string()));
                    stats.folders += 1;
                    id
                }
            });
        }
        Ok(parent)
    }
}

/// Subscribed feeds by `feed_key`: (id, folder, title).
fn subscribed(conn: &Connection) -> Result<std::collections::HashMap<String, (i64, Option<i64>, String)>> {
    Ok(db::feed_places(conn)?.into_iter().map(|f| (feed_key(&f.url), (f.id, f.folder_id, f.title))).collect())
}

/// Every missing folder along `path`, as name keys from the top, for counting.
fn missing_folders(path: &[String], missing: usize, into: &mut std::collections::HashSet<Vec<String>>) {
    for end in path.len() + 1 - missing..=path.len() {
        into.insert(path[..end].iter().map(|t| name_key(t)).collect());
    }
}

/// Compares the file with the subscriptions: what is new, what is there already,
/// and which subscribed feeds the file puts in another folder.
pub fn preview(conn: &Connection, pending: &Pending, file_name: &str) -> Result<ImportPreview> {
    let folders = Folders::load(conn)?;
    let have = subscribed(conn)?;
    let mut out = ImportPreview { file_name: file_name.into(), invalid: pending.invalid, ..Default::default() };
    let mut seen = std::collections::HashSet::new();
    let (mut keep, mut file) = (std::collections::HashSet::new(), std::collections::HashSet::new());
    for feed in &pending.feeds {
        if !seen.insert(feed_key(&feed.url)) {
            out.repeated += 1;
            continue;
        }
        out.in_file += 1;
        let place = folders.find(&feed.path);
        match have.get(&feed_key(&feed.url)) {
            Some((_, folder, title)) => {
                out.subscribed += 1;
                if place != Ok(*folder) {
                    out.move_count += 1;
                    if out.moves.len() < PREVIEW_ITEMS {
                        let to = if feed.path.is_empty() { "No folder".into() } else { feed.path.join(" / ") };
                        out.moves.push(Move { title: title.clone(), from: folders.name(*folder), to });
                    }
                    if let Err(n) = place {
                        missing_folders(&feed.path, n, &mut file);
                    }
                }
            }
            None => {
                out.new_feeds += 1;
                if out.new_titles.len() < PREVIEW_ITEMS {
                    out.new_titles.push(if feed.title.is_empty() { net::without_credentials(&feed.url) } else { feed.title.clone() });
                }
                if let Err(n) = place {
                    missing_folders(&feed.path, n, &mut keep);
                    missing_folders(&feed.path, n, &mut file);
                }
            }
        }
    }
    out.new_folders_keep = keep.len();
    out.new_folders_file = file.len();
    Ok(out)
}

/// Adds the feeds of the file that are not subscribed yet, in one transaction.
/// With `Layout::File`, subscribed feeds also move to the file's folders.
pub fn apply(conn: &Connection, pending: &Pending, layout: Layout) -> Result<ImportStats> {
    conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<ImportStats> {
        let mut stats = ImportStats { skipped: pending.invalid, ..Default::default() };
        let mut folders = Folders::load(conn)?;
        let have = subscribed(conn)?;
        let mut seen = std::collections::HashSet::new();
        for feed in &pending.feeds {
            let key = feed_key(&feed.url);
            if !seen.insert(key.clone()) {
                stats.skipped += 1;
                continue;
            }
            match have.get(&key) {
                Some((id, folder, _)) => {
                    if layout == Layout::File && folders.find(&feed.path) != Ok(*folder) {
                        let target = folders.make(conn, &feed.path, &mut stats)?;
                        db::move_feed(conn, *id, target, None)?;
                        stats.moved += 1;
                    } else {
                        stats.skipped += 1;
                    }
                }
                None => {
                    let target = folders.make(conn, &feed.path, &mut stats)?;
                    let title = Some(feed.title.as_str()).filter(|t| !t.is_empty());
                    let id = db::insert_feed(conn, &feed.url, target, title, feed.site.as_deref())?;
                    stats.feeds += 1;
                    stats.new_feed_ids.push(id);
                }
            }
        }
        Ok(stats)
    })();
    match result {
        Ok(stats) => {
            conn.execute_batch("COMMIT")?;
            Ok(stats)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Imports a parsed file, keeping the folders of subscribed feeds.
pub fn import(conn: &Connection, outlines: &[Outline]) -> Result<ImportStats> {
    apply(conn, &feeds_in(outlines), Layout::Keep)
}

pub fn export(conn: &Connection) -> Result<String> {
    let folders = db::folders(conn)?;
    let feeds = db::feeds(conn)?;
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let io = |e: std::io::Error| Error::msg(e.to_string());

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None))).map_err(io)?;
    let mut opml = BytesStart::new("opml");
    opml.push_attribute(("version", "2.0"));
    writer.write_event(Event::Start(opml)).map_err(io)?;
    writer.write_event(Event::Start(BytesStart::new("head"))).map_err(io)?;
    writer.write_event(Event::Start(BytesStart::new("title"))).map_err(io)?;
    writer.write_event(Event::Text(BytesText::new("Tidings subscriptions"))).map_err(io)?;
    writer.write_event(Event::End(BytesEnd::new("title"))).map_err(io)?;
    let date = chrono::Utc::now().to_rfc2822();
    writer.write_event(Event::Start(BytesStart::new("dateCreated"))).map_err(io)?;
    writer.write_event(Event::Text(BytesText::new(&date))).map_err(io)?;
    writer.write_event(Event::End(BytesEnd::new("dateCreated"))).map_err(io)?;
    writer.write_event(Event::End(BytesEnd::new("head"))).map_err(io)?;
    writer.write_event(Event::Start(BytesStart::new("body"))).map_err(io)?;

    fn write_level(
        writer: &mut Writer<Vec<u8>>,
        parent: Option<i64>,
        folders: &[db::Folder],
        feeds: &[db::Feed],
    ) -> std::io::Result<()> {
        // Feeds and folders share one order, so write them interleaved.
        enum Child<'a> {
            Folder(&'a db::Folder),
            Feed(&'a db::Feed),
        }
        let mut kids: Vec<(i64, u8, Child)> = folders
            .iter()
            .filter(|f| f.parent_id == parent)
            .map(|f| (f.position, 0, Child::Folder(f)))
            .chain(feeds.iter().filter(|f| f.folder_id == parent).map(|f| (f.position, 1, Child::Feed(f))))
            .collect();
        kids.sort_by_key(|(pos, kind, _)| (*pos, *kind));
        for (_, _, child) in kids {
            let feed = match child {
                Child::Folder(folder) => {
                    let mut el = BytesStart::new("outline");
                    el.push_attribute(("text", folder.title.as_str()));
                    el.push_attribute(("title", folder.title.as_str()));
                    writer.write_event(Event::Start(el))?;
                    write_level(writer, Some(folder.id), folders, feeds)?;
                    writer.write_event(Event::End(BytesEnd::new("outline")))?;
                    continue;
                }
                Child::Feed(feed) => feed,
            };
            let mut el = BytesStart::new("outline");
            el.push_attribute(("type", "rss"));
            el.push_attribute(("text", feed.title.as_str()));
            el.push_attribute(("title", feed.title.as_str()));
            el.push_attribute(("xmlUrl", feed.url.as_str()));
            if let Some(site) = &feed.site_url {
                el.push_attribute(("htmlUrl", site.as_str()));
            }
            writer.write_event(Event::Empty(el))?;
        }
        Ok(())
    }
    write_level(&mut writer, None, &folders, &feeds).map_err(io)?;

    writer.write_event(Event::End(BytesEnd::new("body"))).map_err(io)?;
    writer.write_event(Event::End(BytesEnd::new("opml"))).map_err(io)?;
    String::from_utf8(writer.into_inner()).map_err(|e| Error::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_folders() {
        let xml = r#"<?xml version="1.0"?>
        <opml version="1.0"><head><title>x</title></head><body>
          <outline text="Tech">
            <outline text="Rust" type="rss" xmlUrl="https://blog.rust-lang.org/feed.xml" htmlUrl="https://blog.rust-lang.org/"/>
            <outline text="Deep"><outline title="A &amp; B" xmlUrl="https://ab.example/rss"/></outline>
          </outline>
          <outline text="Loose" xmlUrl="https://loose.example/atom"/>
        </body></opml>"#;
        let nodes = parse(xml).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].title, "Tech");
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[1].children[0].title, "A & B");
        assert_eq!(nodes[1].xml_url.as_deref(), Some("https://loose.example/atom"));
    }

    #[test]
    fn rejects_non_opml() {
        assert!(parse("<html><body></body></html>").is_err());
    }
}
