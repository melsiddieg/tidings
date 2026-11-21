//! Starred articles in one file: HTML for a browser, or Markdown for a notes app.
//!
//! The text of each article is its stored HTML (the full text when it was loaded),
//! cleaned again by `sanitize::export`. Links keep only http, https and mailto
//! addresses. Images are left out unless the user asks for them. Then they load
//! from their sites when the file is opened: the HTML file's content policy allows
//! images and nothing else from the web.

use chrono::{Local, TimeZone};
use dom_query::Document;

use crate::db::ArticleRow;
use crate::sanitize::{self, escape};

const STYLE: &str = "body{max-width:42rem;margin:2rem auto;padding:0 1rem;font:17px/1.6 Georgia,serif;color:#222;background:#fff}\
h1,h2{line-height:1.25}h2{margin-bottom:.2rem}article{border-top:1px solid #ddd;margin-top:2.5rem;padding-top:1rem}\
.meta{color:#666;font:14px/1.4 system-ui,sans-serif}img{max-width:100%;height:auto}pre{overflow:auto}\
blockquote{margin-left:0;padding-left:1rem;border-left:3px solid #ddd;color:#555}\
@media (prefers-color-scheme:dark){body{background:#1c1c1e;color:#ddd}a{color:#8ab4f8}.meta{color:#999}\
article,blockquote{border-color:#444}blockquote{color:#bbb}}";

fn date(t: i64) -> String {
    Local.timestamp_opt(t, 0).single().map(|d| d.format("%-d %B %Y").to_string()).unwrap_or_default()
}

/// A link address that can be written into the file: http, https or mailto.
fn link(url: Option<&str>) -> Option<String> {
    let url = url::Url::parse(url?.trim()).ok()?;
    matches!(url.scheme(), "http" | "https" | "mailto").then(|| url.to_string())
}

fn title(row: &ArticleRow) -> &str {
    let title = row.title.trim();
    if title.is_empty() { "Untitled" } else { title }
}

/// The feed, the author and the date, as one line.
fn byline(row: &ArticleRow, feed: &str) -> String {
    let author = row.author.as_deref().map(str::trim).filter(|a| !a.is_empty());
    [Some(feed.trim()).filter(|f| !f.is_empty()), author, Some(date(row.published).as_str())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn body(row: &ArticleRow, images: bool) -> String {
    let stored = row.full_content.as_deref().or(row.content.as_deref()).unwrap_or("");
    sanitize::export(stored, images)
}

fn heading(count: usize) -> String {
    let articles = if count == 1 { "1 article".to_string() } else { format!("{count} articles") };
    format!("{articles}, exported from Tidings on {}.", date(crate::db::now()))
}

/// The starred articles as one HTML page.
pub fn html(articles: &[(ArticleRow, String)], images: bool) -> String {
    let policy = if images {
        "default-src 'none'; style-src 'unsafe-inline'; img-src http: https:"
    } else {
        "default-src 'none'; style-src 'unsafe-inline'"
    };
    let mut out = format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Starred articles</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         <h1>Starred articles</h1>\n<p class=\"meta\">{}</p>\n",
        escape(&heading(articles.len()))
    );
    for (row, feed) in articles {
        let name = escape(title(row));
        let head = match link(row.url.as_deref()) {
            Some(href) => format!("<a href=\"{}\">{name}</a>", escape(&href)),
            None => name,
        };
        out.push_str(&format!("<article>\n<h2>{head}</h2>\n<p class=\"meta\">{}</p>\n", escape(&byline(row, feed))));
        out.push_str(&body(row, images));
        if let Some(href) = link(row.enclosure_url.as_deref()) {
            let kind = row.enclosure_type.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("file");
            out.push_str(&format!("\n<p class=\"meta\">Attachment: <a href=\"{}\">{}</a></p>", escape(&href), escape(kind)));
        }
        out.push_str("\n</article>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Text with the characters that mean something in Markdown escaped, on one line.
fn md_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// The starred articles as one Markdown file.
pub fn markdown(articles: &[(ArticleRow, String)], images: bool) -> String {
    let mut out = format!("# Starred articles\n\n{}\n", md_text(&heading(articles.len())));
    for (row, feed) in articles {
        let name = md_text(title(row));
        // `<...>`: the address may hold parentheses. `Url` encodes `<`, `>` and spaces.
        let head = match link(row.url.as_deref()) {
            Some(href) => format!("[{name}](<{href}>)"),
            None => name,
        };
        out.push_str(&format!("\n---\n\n## {head}\n\n*{}*\n\n", md_text(&byline(row, feed))));
        let html = body(row, images);
        // The cleaned HTML is cut and flattened when it nests deeply (`bounded`), but
        // check again before the parser sees it.
        let text = if sanitize::too_deep(&html) {
            md_text(&sanitize::plain_text(&html))
        } else {
            Document::fragment(html.as_str()).md(None).to_string()
        };
        out.push_str(text.trim());
        out.push('\n');
        if let Some(href) = link(row.enclosure_url.as_deref()) {
            let kind = row.enclosure_type.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("file");
            out.push_str(&format!("\nAttachment: [{}](<{href}>)\n", md_text(kind)));
        }
    }
    out
}
