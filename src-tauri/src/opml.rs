//! OPML import and export. Folders map to nested <outline> elements.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use rusqlite::Connection;
use serde::Serialize;

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
    pub feeds: usize,
    pub folders: usize,
    pub skipped: usize,
    #[serde(skip)]
    pub new_feed_ids: Vec<i64>,
}

fn outline_from(e: &BytesStart<'_>) -> Outline {
    let mut out = Outline::default();
    let mut text = String::new();
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_ascii_lowercase();
        let Ok(value) = attr.unescape_value() else { continue };
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

pub fn parse(xml: &str) -> Result<Vec<Outline>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    // Stack of open outlines. Index 0 is a virtual root.
    let mut stack: Vec<Outline> = vec![Outline::default()];
    let mut saw_opml = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"opml" => saw_opml = true,
                b"outline" => stack.push(outline_from(&e)),
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"outline" {
                    let node = outline_from(&e);
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"outline" && stack.len() > 1 {
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

pub fn import(conn: &Connection, outlines: &[Outline]) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    conn.execute_batch("BEGIN")?;
    let result = import_level(conn, outlines, None, &mut stats);
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    Ok(stats)
}

fn import_level(
    conn: &Connection,
    outlines: &[Outline],
    parent: Option<i64>,
    stats: &mut ImportStats,
) -> Result<()> {
    for node in outlines {
        if let Some(xml_url) = &node.xml_url {
            match net::parse_web_url(xml_url) {
                Ok(url) => {
                    if db::feed_id_by_url(conn, url.as_str())?.is_some() {
                        stats.skipped += 1;
                    } else {
                        let title = Some(node.title.as_str()).filter(|t| !t.is_empty());
                        let site = node.html_url.as_deref().and_then(|h| net::parse_web_url(h).ok());
                        let id = db::insert_feed(conn, url.as_str(), parent, title, site.as_ref().map(|u| u.as_str()))?;
                        stats.feeds += 1;
                        stats.new_feed_ids.push(id);
                    }
                }
                Err(_) => stats.skipped += 1,
            }
            // Some exporters nest feeds under a feed. Keep them at the same level.
            import_level(conn, &node.children, parent, stats)?;
        } else if !node.children.is_empty() {
            let title = if node.title.is_empty() { "Untitled folder" } else { node.title.as_str() };
            let (folder, created) = db::find_or_create_folder(conn, title, parent)?;
            if created {
                stats.folders += 1;
            }
            import_level(conn, &node.children, Some(folder), stats)?;
        }
    }
    Ok(())
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
        for folder in folders.iter().filter(|f| f.parent_id == parent) {
            let mut el = BytesStart::new("outline");
            el.push_attribute(("text", folder.title.as_str()));
            el.push_attribute(("title", folder.title.as_str()));
            writer.write_event(Event::Start(el))?;
            write_level(writer, Some(folder.id), folders, feeds)?;
            writer.write_event(Event::End(BytesEnd::new("outline")))?;
        }
        for feed in feeds.iter().filter(|f| f.folder_id == parent) {
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
