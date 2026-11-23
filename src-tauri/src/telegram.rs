//! Public Telegram channels as feeds. Telegram publishes no feeds, but every public
//! channel has a web preview at `https://t.me/s/<name>` that works without an
//! account and lists its newest posts (about 20). Tidings reads that page and writes
//! the posts out as an Atom feed, which then goes through the same parser, cleaning
//! and image proxy as any other feed (`feeds::parse_raw`). Only t.me is contacted;
//! the posts' images load through the image proxy like all images.

use dom_query::{Document, Selection};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use url::Url;

use crate::error::{Error, Result};
use crate::sanitize;

/// The error for a page without posts or channel: the channel does not exist, is
/// private, or turned its web preview off.
pub const NO_PREVIEW: &str =
    "This Telegram channel has no public web preview: it does not exist, is private, or turned the preview off";

/// Paths on t.me that are not channels.
const NOT_CHANNELS: &[&str] = &[
    "s", "c", "joinchat", "addstickers", "addemoji", "addtheme", "addlist", "share", "proxy", "socks", "iv",
    "login", "setlanguage", "boost", "contact", "confirmphone", "invoice", "giftcode", "bg", "m", "k", "a", "z",
];

/// Whether `url` is on Telegram's link site, where Tidings reads channel pages.
pub fn is_telegram(url: &Url) -> bool {
    matches!(url.host_str(), Some("t.me" | "www.t.me" | "telegram.me" | "www.telegram.me" | "telegram.dog"))
}

/// A channel's name, the way Telegram allows it: letters, digits and underscores,
/// starting with a letter.
fn is_name(name: &str) -> bool {
    (3..=32).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The channel a Telegram link is about: `t.me/name`, `t.me/name/123` (a post) or
/// `t.me/s/name` (the preview itself).
pub fn channel(url: &Url) -> Option<String> {
    if !is_telegram(url) {
        return None;
    }
    let parts: Vec<&str> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
    let name = match parts.as_slice() {
        ["s", name, ..] => *name,
        [name, ..] if !NOT_CHANNELS.contains(&name.to_ascii_lowercase().as_str()) => *name,
        _ => return None,
    };
    is_name(name).then(|| name.to_string())
}

/// The web preview of a channel, which Tidings stores as the feed's address.
pub fn preview_url(name: &str) -> Url {
    Url::parse(&format!("https://t.me/s/{name}")).expect("a valid address")
}

/// The first `background-image:url('...')` address in a style attribute.
fn background_image(style: &str) -> Option<String> {
    let start = style.find("background-image:url(")? + "background-image:url(".len();
    let rest = &style[start..];
    let end = rest.find(')')?;
    let raw = rest[..end].trim().trim_matches(|c| c == '\'' || c == '"');
    let url = if raw.starts_with("//") { format!("https:{raw}") } else { raw.to_string() };
    url.starts_with("https://").then_some(url)
}

fn text_of(sel: &Selection<'_>) -> String {
    sel.text().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One post of the page.
struct Post {
    link: String,
    published: String,
    title: String,
    author: Option<String>,
    html: String,
}

fn read_post(message: &Selection<'_>, channel_title: &str) -> Option<Post> {
    if message.has_class("service_message") {
        return None;
    }
    let date = message.select(".tgme_widget_message_date").first();
    let published = date.select("time").attr("datetime")?.to_string();
    let link = date
        .attr("href")
        .map(|h| h.to_string())
        .filter(|h| h.starts_with("https://t.me/"))
        .or_else(|| message.attr("data-post").map(|p| format!("https://t.me/{p}")))?;

    let mut html = String::new();
    let forwarded = text_of(&message.select(".tgme_widget_message_forwarded_from_name").first());
    if !forwarded.is_empty() {
        html.push_str(&format!("<p><i>Forwarded from {}</i></p>", escape(&forwarded)));
    }
    // The post's own text (`js-message_text`), not the quote of a post it replies to
    // (`js-message_reply_text`). It may sit in a wrapper such as `media_supported_cont`.
    let text = message.select(".tgme_widget_message_text.js-message_text").first();
    let text_html = if text.exists() { text.inner_html().to_string() } else { String::new() };
    for photo in message.select(".tgme_widget_message_photo_wrap").iter() {
        if let Some(src) = photo.attr("style").and_then(|s| background_image(&s)) {
            html.push_str(&format!("<p><img src=\"{}\" alt=\"\"></p>", escape(&src)));
        }
    }
    let mut kind = if html.contains("<img") { "Photo" } else { "" };
    for video in message.select(".tgme_widget_message_video_player").iter() {
        let thumb = video.select(".tgme_widget_message_video_thumb").attr("style").and_then(|s| background_image(&s));
        if let Some(src) = thumb {
            html.push_str(&format!("<p><img src=\"{}\" alt=\"\"></p>", escape(&src)));
        }
        html.push_str(&format!("<p><a href=\"{}\">Video: watch it on Telegram</a></p>", escape(&link)));
        kind = "Video";
    }
    if !text_html.is_empty() {
        html.push_str(&format!("<p>{text_html}</p>"));
    }
    for document in message.select(".tgme_widget_message_document_title").iter() {
        let name = text_of(&document);
        if !name.is_empty() {
            html.push_str(&format!("<p>File: {}</p>", escape(&name)));
            kind = if kind.is_empty() { "File" } else { kind };
        }
    }
    let poll = text_of(&message.select(".tgme_widget_message_poll_question").first());
    if !poll.is_empty() {
        html.push_str(&format!("<p>Poll: {}</p>", escape(&poll)));
        kind = if kind.is_empty() { "Poll" } else { kind };
    }
    let preview = message.select("a.tgme_widget_message_link_preview").first();
    if let Some(href) = preview.attr("href").map(|h| h.to_string()).filter(|h| h.starts_with("http")) {
        let site = text_of(&preview.select(".link_preview_site_name"));
        let title = text_of(&preview.select(".link_preview_title"));
        let label = [site, title].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(": ");
        if !label.is_empty() {
            html.push_str(&format!("<blockquote><a href=\"{}\">{}</a></blockquote>", escape(&href), escape(&label)));
        }
    }
    if html.is_empty() {
        return None;
    }
    // Posts have no titles: the first line of the text, or what the post holds.
    let first_line = text_html.split("<br").next().map(sanitize::plain_text).unwrap_or_default();
    let title = if !first_line.trim().is_empty() {
        sanitize::truncate(first_line.trim(), 120)
    } else if !kind.is_empty() {
        kind.to_string()
    } else {
        format!("Post in {channel_title}")
    };
    let author = Some(text_of(&message.select(".tgme_widget_message_from_author").first())).filter(|a| !a.is_empty());
    Some(Post { link, published, title, author, html })
}

/// `&`, `<`, `>` and `"` for text placed in the post HTML built here.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The posts of a channel's web preview as an Atom feed, newest first.
pub fn to_atom(html: &str, page: &Url) -> Result<String> {
    // Deeply nested elements make the parser slow (`sanitize::too_deep`).
    if sanitize::too_deep(html) {
        return Err(Error::msg(NO_PREVIEW));
    }
    let doc = Document::from(html);
    let info = doc.select(".tgme_channel_info");
    let messages = doc.select(".tgme_widget_message[data-post]");
    if !info.exists() && !messages.exists() {
        return Err(Error::msg(NO_PREVIEW));
    }
    let name = channel(page).unwrap_or_default();
    let meta = |property: &str| {
        doc.select(&format!("meta[property=\"{property}\"]")).attr("content").map(|c| c.trim().to_string()).filter(|c| !c.is_empty())
    };
    let title = Some(text_of(&doc.select(".tgme_channel_info_header_title").first()))
        .filter(|t| !t.is_empty())
        .or_else(|| meta("og:title"))
        .unwrap_or_else(|| name.clone());
    let description = Some(text_of(&doc.select(".tgme_channel_info_description").first()))
        .filter(|t| !t.is_empty())
        .or_else(|| meta("og:description"));
    let icon = meta("og:image").filter(|i| i.starts_with("https://"));
    let mut posts: Vec<Post> = messages.iter().filter_map(|m| read_post(&m, &title)).collect();
    // The page lists the oldest first.
    posts.reverse();

    let io = |e: std::io::Error| Error::msg(e.to_string());
    let mut w = Writer::new(Vec::new());
    let text = |w: &mut Writer<Vec<u8>>, tag: &str, value: &str| -> std::io::Result<()> {
        w.write_event(Event::Start(BytesStart::new(tag)))?;
        w.write_event(Event::Text(BytesText::new(value)))?;
        w.write_event(Event::End(BytesEnd::new(tag)))
    };
    let link = |w: &mut Writer<Vec<u8>>, href: &str| -> std::io::Result<()> {
        let mut el = BytesStart::new("link");
        el.push_attribute(("rel", "alternate"));
        el.push_attribute(("href", href));
        w.write_event(Event::Empty(el))
    };
    (|| -> std::io::Result<()> {
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        let mut feed = BytesStart::new("feed");
        feed.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
        w.write_event(Event::Start(feed))?;
        text(&mut w, "id", page.as_str())?;
        text(&mut w, "title", &title)?;
        if let Some(d) = &description {
            text(&mut w, "subtitle", d)?;
        }
        if !name.is_empty() {
            link(&mut w, &format!("https://t.me/{name}"))?;
        }
        if let Some(i) = &icon {
            text(&mut w, "icon", i)?;
        }
        if let Some(newest) = posts.first() {
            text(&mut w, "updated", &newest.published)?;
        }
        for post in &posts {
            w.write_event(Event::Start(BytesStart::new("entry")))?;
            text(&mut w, "id", &post.link)?;
            text(&mut w, "title", &post.title)?;
            link(&mut w, &post.link)?;
            text(&mut w, "published", &post.published)?;
            text(&mut w, "updated", &post.published)?;
            if let Some(author) = &post.author {
                w.write_event(Event::Start(BytesStart::new("author")))?;
                text(&mut w, "name", author)?;
                w.write_event(Event::End(BytesEnd::new("author")))?;
            }
            let mut content = BytesStart::new("content");
            content.push_attribute(("type", "html"));
            w.write_event(Event::Start(content))?;
            w.write_event(Event::Text(BytesText::new(&post.html)))?;
            w.write_event(Event::End(BytesEnd::new("content")))?;
            w.write_event(Event::End(BytesEnd::new("entry")))?;
        }
        w.write_event(Event::End(BytesEnd::new("feed")))
    })()
    .map_err(io)?;
    String::from_utf8(w.into_inner()).map_err(|e| Error::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_links_are_recognised() {
        let name = |s: &str| channel(&Url::parse(s).unwrap());
        assert_eq!(name("https://t.me/durov").as_deref(), Some("durov"));
        assert_eq!(name("https://t.me/durov/569").as_deref(), Some("durov"));
        assert_eq!(name("https://t.me/s/durov").as_deref(), Some("durov"));
        assert_eq!(name("https://telegram.me/Telegram").as_deref(), Some("Telegram"));
        assert_eq!(name("https://t.me/joinchat/AbCdEf"), None);
        assert_eq!(name("https://t.me/+AbCdEfGh"), None);
        assert_eq!(name("https://t.me/c/123456/7"), None, "a private channel");
        assert_eq!(name("https://t.me/addstickers/pack"), None);
        assert_eq!(name("https://t.me/1abc"), None);
        assert_eq!(name("https://example.com/durov"), None);
        assert_eq!(preview_url("durov").as_str(), "https://t.me/s/durov");
    }

    #[test]
    fn background_images_are_read() {
        assert_eq!(
            background_image("width:484px;background-image:url('https://cdn4.telesco.pe/file/a.jpg')").as_deref(),
            Some("https://cdn4.telesco.pe/file/a.jpg")
        );
        assert_eq!(background_image("background-image:url('//telegram.org/img/e.png')").as_deref(), Some("https://telegram.org/img/e.png"));
        assert_eq!(background_image("background-image:url('javascript:alert(1)')"), None);
        assert_eq!(background_image("color:red"), None);
    }
}
