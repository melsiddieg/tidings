//! HTML cleaning. Feed content is untrusted: we remove scripts, styles, event
//! handlers, iframes and forms before anything reaches the web view.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dom_query::Document;
use url::Url;

/// Query parameters that only exist to track readers.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid", "mc_eid", "igshid",
    "yclid", "_hsenc", "_hsmi", "mkt_tok", "oly_anon_id", "oly_enc_id", "vero_id", "vero_conv",
    "rb_clickid", "s_cid", "wt_mc", "at_medium", "at_campaign", "ga_source", "ga_medium",
    "ga_campaign", "_ga", "_gl", "twclid", "ttclid", "li_fat_id", "srsltid", "ncid", "sr_share",
];

/// Image URLs that are known tracking pixels or ad beacons.
const TRACKER_IMAGES: &[&str] = &[
    "feeds.feedburner.com/~r/",
    "feeds.feedburner.com/~ff/",
    "feedproxy.google.com/~r/",
    "pixel.wp.com",
    "stats.wordpress.com",
    "pixel.quantserve.com",
    "google-analytics.com",
    "googletagmanager.com",
    "doubleclick.net",
    "googlesyndication.com",
    "scorecardresearch.com",
    "feedsportal.com",
    "pixel.tapad.com",
    "assoc-amazon.com",
    "analytics.twitter.com",
    "facebook.com/tr",
    "medium.com/_/stat",
    "counter.theconversation",
    "piwik.",
    "matomo.",
];

pub fn is_tracking_param(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_") || TRACKING_PARAMS.contains(&key.as_str())
}

pub fn strip_tracking_params(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else { return raw.to_string() };
    if url.query().is_none() {
        return raw.to_string();
    }
    let all: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let kept: Vec<&(String, String)> = all.iter().filter(|(k, _)| !is_tracking_param(k)).collect();
    if kept.len() == all.len() {
        return raw.to_string();
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(k, v)| (k, v)));
    }
    url.to_string()
}

fn is_tracker_image(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    TRACKER_IMAGES.iter().any(|t| lower.contains(t))
}

/// The tag and attribute allow-list shared by the ingest and render passes.
fn builder<'a>() -> Builder<'a> {
    let tags: HashSet<&str> = [
        "a", "abbr", "acronym", "article", "aside", "b", "bdi", "bdo", "blockquote", "br",
        "caption", "center", "cite", "code", "col", "colgroup", "data", "dd", "del", "details",
        "dfn", "div", "dl", "dt", "em", "figcaption", "figure", "footer", "h1", "h2", "h3", "h4",
        "h5", "h6", "header", "hgroup", "hr", "i", "img", "ins", "kbd", "li", "mark", "ol", "p",
        "pre", "q", "rp", "rt", "rtc", "ruby", "s", "samp", "section", "small", "span", "strike",
        "strong", "sub", "summary", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "time",
        "tr", "tt", "u", "ul", "var", "wbr",
    ]
    .into_iter()
    .collect();

    let mut tag_attrs: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut allow = |tag: &'a str, attrs: &[&'a str]| {
        tag_attrs.insert(tag, attrs.iter().copied().collect());
    };
    allow("a", &["href", "hreflang"]);
    allow("img", &["src", "alt"]);
    allow("td", &["colspan", "rowspan"]);
    allow("th", &["colspan", "rowspan", "scope"]);
    allow("ol", &["start", "reversed", "type"]);
    allow("time", &["datetime"]);
    allow("del", &["datetime"]);
    allow("ins", &["datetime"]);
    allow("details", &["open"]);
    allow("abbr", &["title"]);

    let mut b = Builder::default();
    b.tags(tags)
        .tag_attributes(tag_attrs)
        .generic_attributes(["title", "lang", "dir"].into_iter().collect())
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .link_rel(Some("noopener noreferrer nofollow"))
        .strip_comments(true);
    b
}

/// Cleans feed or page HTML before it is stored.
/// Relative links are made absolute using `base`.
pub fn clean(html: &str, base: Option<&Url>, strip_tracking: bool) -> String {
    let prepared = preprocess(html, base);
    let mut b = builder();
    b.url_relative(match base {
        Some(base) => UrlRelative::RewriteWithBase(base.clone()),
        None => UrlRelative::Deny,
    });
    if strip_tracking {
        b.attribute_filter(|element, attribute, value| {
            if element == "a" && attribute == "href" {
                Some(Cow::Owned(strip_tracking_params(value)))
            } else {
                Some(Cow::Borrowed(value))
            }
        });
    }
    b.clean(&prepared).to_string()
}

/// Fixes up markup that the sanitizer would otherwise throw away:
/// lazy-loaded images, embedded videos and tracking pixels.
fn preprocess(html: &str, base: Option<&Url>) -> String {
    let doc = Document::fragment(html);

    let images = doc.select("img");
    for img in images.nodes() {
        let src = img.attr("src").map(|s| s.to_string()).unwrap_or_default();
        let lazy = ["data-src", "data-lazy-src", "data-original", "data-orig-file", "data-url"]
            .iter()
            .find_map(|a| img.attr(a).map(|v| v.to_string()).filter(|v| !v.trim().is_empty()));
        if let Some(lazy) = lazy {
            if src.is_empty() || src.starts_with("data:") {
                img.set_attr("src", &lazy);
            }
        }
        if img.attr("src").is_none_or(|s| s.trim().is_empty() || s.starts_with("data:")) {
            let srcset = img.attr("srcset").or_else(|| img.attr("data-srcset"));
            if let Some(first) = srcset.as_deref().and_then(first_srcset_url) {
                img.set_attr("src", &first);
            }
        }
        let tiny = |attr: &str| img.attr(attr).is_some_and(|v| matches!(v.trim(), "0" | "1" | "1px"));
        let current = img.attr("src").map(|s| s.to_string()).unwrap_or_default();
        if (tiny("width") && tiny("height")) || is_tracker_image(&current) {
            img.remove_from_parent();
        }
    }

    let embeds = doc.select("iframe, embed, object, video, audio");
    for node in embeds.nodes() {
        let src = node
            .attr("src")
            .or_else(|| node.attr("data"))
            .map(|s| s.to_string())
            .or_else(|| {
                node.find(&["source"])
                    .into_iter()
                    .find_map(|s| s.attr("src").map(|v| v.to_string()))
            });
        let resolved = src.as_deref().and_then(|s| resolve(s, base));
        match resolved {
            Some(url) => {
                let link = embed_link(&url);
                let host = link.host_str().unwrap_or("").trim_start_matches("www.").to_string();
                let label = match node.node_name().as_deref() {
                    Some("audio") => "Audio",
                    Some("video") => "Video",
                    _ if host.contains("youtube") || host.contains("vimeo") => "Video",
                    _ => "Embedded content",
                };
                let poster = node.attr("poster").and_then(|p| resolve(p.as_ref(), base));
                let mut out = format!(
                    "<p><a href=\"{}\">▶ {label} · {}</a></p>",
                    escape(link.as_str()),
                    escape(&host)
                );
                if let Some(poster) = poster {
                    out = format!(
                        "<p><a href=\"{}\"><img src=\"{}\" alt=\"{label}\"></a></p>{out}",
                        escape(link.as_str()),
                        escape(poster.as_str())
                    );
                }
                node.replace_with_html(out);
            }
            None => node.remove_from_parent(),
        }
    }

    doc.select("noscript, script, style, form, button, input, select, textarea").remove();
    doc.html_root().inner_html().to_string()
}

fn first_srcset_url(srcset: &str) -> Option<String> {
    srcset
        .split(',')
        .filter_map(|part| part.split_whitespace().next())
        .find(|u| !u.is_empty() && !u.starts_with("data:"))
        .map(str::to_string)
}

fn resolve(src: &str, base: Option<&Url>) -> Option<Url> {
    let src = src.trim();
    let url = match base {
        Some(base) => base.join(src).ok()?,
        None => Url::parse(src).ok()?,
    };
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// Turns embed URLs (youtube.com/embed/ID) into normal watch URLs.
fn embed_link(url: &Url) -> Url {
    let host = url.host_str().unwrap_or("");
    if host.contains("youtube.com") || host.contains("youtube-nocookie.com") {
        if let Some(id) = url.path().strip_prefix("/embed/") {
            let id: String = id.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            if let Ok(watch) = Url::parse(&format!("https://www.youtube.com/watch?v={id}")) {
                return watch;
            }
        }
    }
    if host == "player.vimeo.com" {
        if let Some(id) = url.path().strip_prefix("/video/") {
            if let Ok(page) = Url::parse(&format!("https://vimeo.com/{id}")) {
                return page;
            }
        }
    }
    url.clone()
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Plain text paragraphs to HTML.
pub fn text_to_html(text: &str) -> String {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", escape(p).replace('\n', "<br>")))
        .collect()
}

/// Plain text from HTML, with whitespace collapsed.
pub fn plain_text(html: &str) -> String {
    let doc = Document::fragment(html);
    let text = doc.root().formatted_text();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > max_chars / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches([',', '.', ';', ':', ' ']))
}

/// First usable image URL in cleaned HTML (used as list thumbnail).
pub fn first_image(html: &str) -> Option<String> {
    let doc = Document::fragment(html);
    let src = doc.select("img").iter().find_map(|img| img.attr("src").map(|s| s.to_string()));
    src.filter(|s| s.starts_with("https://") || s.starts_with("http://"))
}

pub fn proxy_image_url(url: &str) -> String {
    format!("tidings://localhost/img/{}", URL_SAFE_NO_PAD.encode(url))
}

pub struct Rendered {
    pub html: String,
    pub blocked_images: usize,
}

/// Prepares stored HTML for display. Images are either routed through the
/// local image proxy or removed. The HTML is cleaned again as a second line of defense.
pub fn render(html: &str, load_images: bool) -> Rendered {
    let images = html.matches("<img").count();
    let mut b = builder();
    b.url_relative(UrlRelative::Deny);
    if load_images {
        b.attribute_filter(|element, attribute, value| {
            if element == "img" && attribute == "src" {
                Some(Cow::Owned(proxy_image_url(value)))
            } else {
                Some(Cow::Borrowed(value))
            }
        });
    } else {
        b.rm_tags(["img"]);
    }
    Rendered {
        html: b.clean(html).to_string(),
        blocked_images: if load_images { 0 } else { images },
    }
}

/// Stable 64-bit FNV-1a hash, used to detect changed articles.
pub fn hash(parts: &[&str]) -> i64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for byte in part.as_bytes().iter().chain(std::iter::once(&0u8)) {
            h ^= *byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_scripts_and_handlers() {
        let base = Url::parse("https://example.com/post/").unwrap();
        let out = clean(
            r#"<p onclick="evil()">Hi<script>alert(1)</script><img src="a.png" onerror="x()"></p><iframe src="https://www.youtube.com/embed/abc123"></iframe><a href="javascript:alert(1)">x</a>"#,
            Some(&base),
            true,
        );
        assert!(!out.contains("script"));
        assert!(!out.contains("onclick"));
        assert!(!out.contains("onerror"));
        assert!(!out.contains("javascript"));
        assert!(out.contains("https://example.com/post/a.png"));
        assert!(out.contains("https://www.youtube.com/watch?v=abc123"));
        assert!(!out.contains("<iframe"));
    }

    #[test]
    fn strips_tracking() {
        assert_eq!(
            strip_tracking_params("https://a.com/x?utm_source=rss&id=3&fbclid=1"),
            "https://a.com/x?id=3"
        );
        assert_eq!(strip_tracking_params("https://a.com/x?utm_source=rss"), "https://a.com/x");
        assert_eq!(strip_tracking_params("https://a.com/x?id=1"), "https://a.com/x?id=1");
    }

    #[test]
    fn render_proxies_or_blocks_images() {
        let html = r#"<p>a</p><img src="https://img.example.com/a.jpg" alt="x">"#;
        let shown = render(html, true);
        assert!(shown.html.contains("tidings://localhost/img/"));
        let blocked = render(html, false);
        assert_eq!(blocked.blocked_images, 1);
        assert!(!blocked.html.contains("<img"));
    }

    #[test]
    fn lazy_images_and_pixels() {
        let base = Url::parse("https://example.com/").unwrap();
        let out = clean(
            r#"<img src="data:image/gif;base64,R0" data-src="/real.jpg"><img src="https://pixel.wp.com/g.gif"><img src="/p.gif" width="1" height="1">"#,
            Some(&base),
            false,
        );
        assert!(out.contains("https://example.com/real.jpg"));
        assert!(!out.contains("pixel.wp.com"));
        assert!(!out.contains("p.gif"));
    }

    #[test]
    fn plain_text_and_truncate() {
        assert_eq!(plain_text("<p>Hello</p><p>world &amp; you</p>"), "Hello world & you");
        assert_eq!(truncate("one two three four", 9), "one two…");
    }
}
