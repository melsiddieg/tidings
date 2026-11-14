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
    let html = bounded(html);
    let prepared = preprocess(&html, base);
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
        // One tiny side is enough: counting pixels are 1x1, but also 1 pixel by anything.
        if tiny("width") || tiny("height") || is_tracker_image(&current) {
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
    let html = bounded(html);
    let doc = Document::fragment(html.as_ref());
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
    // Articles stored by older versions were not checked for depth.
    let html = bounded(html);
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
        html: b.clean(&html).to_string(),
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

// ---------------------------------------------------------------------------
// Nesting depth
// ---------------------------------------------------------------------------
//
// An HTML parser looks through the elements that are still open for each new
// element, so its time grows with the square of the nesting depth: 50,000 nested
// `<div>` (550 KB) took 19 seconds to clean and show. `too_deep` finds such HTML
// in one pass over the tags, before any parser builds a tree, and `bounded` then
// keeps only its text.
//
// The pass follows the HTML tokenizer exactly (quotes, comments, script, CDATA),
// and the parser's tree rules where they close elements. Where it cannot follow
// them, it keeps elements open: it may count too deep, never too shallow.

/// Elements nest at most this deep. Articles stay far below it; WebKit itself
/// stops nesting at 512.
const MAX_DEPTH: usize = 200;
/// Feed and page HTML is cut at this length before it is parsed.
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;

/// Elements without content.
const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "image", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];
/// Elements whose end tag may be left out. The parser closes one when the next one
/// starts, so they do not nest by themselves. They are not counted; a table counts
/// for the body, row and cell that the parser adds inside it.
const UNNESTED: &[&str] = &[
    "caption", "colgroup", "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc", "tbody",
    "td", "tfoot", "th", "thead", "tr",
];
/// The parser's "special" elements: the end tag of an ordinary element stops at them.
const SPECIAL: &[&str] = &[
    "address", "annotation-xml", "applet", "area", "article", "aside", "base", "basefont", "bgsound",
    "blockquote", "body", "br", "button", "caption", "center", "col", "colgroup", "dd", "desc", "details",
    "dir", "div", "dl", "dt", "embed", "fieldset", "figcaption", "figure", "footer", "foreignobject", "form",
    "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr", "html",
    "iframe", "img", "input", "keygen", "li", "link", "listing", "main", "marquee", "menu", "meta", "mi",
    "mn", "mo", "ms", "mtext", "nav", "noembed", "noframes", "noscript", "object", "ol", "p", "param",
    "plaintext", "pre", "script", "search", "section", "select", "source", "style", "summary", "table",
    "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "title", "tr", "track", "ul", "wbr", "xmp",
];
/// The end tag of a special element stops at these ("scope").
const SCOPE: &[&str] = &[
    "annotation-xml", "applet", "caption", "desc", "foreignobject", "html", "marquee", "mi", "mn", "mo", "ms",
    "mtext", "object", "table", "td", "template", "th", "title",
];
/// HTML elements that end SVG or MathML content when they start inside it.
const BREAKOUT: &[&str] = &[
    "b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt", "em", "embed", "font",
    "h1", "h2", "h3", "h4", "h5", "h6", "head", "hr", "i", "img", "li", "listing", "menu", "meta", "nobr", "ol",
    "p", "pre", "ruby", "s", "small", "span", "strike", "strong", "sub", "sup", "table", "tt", "u", "ul", "var",
];
/// SVG and MathML elements whose content is HTML again.
const INTEGRATION: &[&str] = &["annotation-xml", "desc", "foreignobject", "mi", "mn", "mo", "ms", "mtext", "title"];

enum Token<'a> {
    Text(&'a str),
    /// A start tag; `closed` when it ends with `/>`.
    Start { name: String, closed: bool },
    End(String),
}

/// The tags and text of HTML, read the way the HTML tokenizer reads them.
struct Tokens<'a> {
    html: &'a str,
    at: usize,
    /// Whether `<![CDATA[` starts a CDATA section (inside SVG or MathML).
    cdata: bool,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

/// Whether `b[at..]` starts with `word` (any case) and then a space, `/` or `>`.
fn word_at(b: &[u8], at: usize, word: &str) -> bool {
    let end = at + word.len();
    b.len() > end && b[at..end].eq_ignore_ascii_case(word.as_bytes()) && (is_space(b[end]) || matches!(b[end], b'/' | b'>'))
}

/// Reads a tag from its name at `start` to its `>`: the end of the name, whether
/// it ends with `/>`, and where the tag ends. A quote starts a value only after
/// `=`, as in the tokenizer. None when the HTML ends inside the tag.
fn read_tag(b: &[u8], start: usize) -> Option<(usize, bool, usize)> {
    let mut i = start;
    while i < b.len() && !is_space(b[i]) && !matches!(b[i], b'/' | b'>') {
        i += 1;
    }
    let name_end = i;
    loop {
        while i < b.len() && is_space(b[i]) {
            i += 1;
        }
        match *b.get(i)? {
            b'>' => return Some((name_end, false, i + 1)),
            b'/' => {
                if *b.get(i + 1)? == b'>' {
                    return Some((name_end, true, i + 2));
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        // An attribute name; its first character may be `=`.
        i += 1;
        while i < b.len() && !is_space(b[i]) && !matches!(b[i], b'/' | b'>' | b'=') {
            i += 1;
        }
        while i < b.len() && is_space(b[i]) {
            i += 1;
        }
        if *b.get(i)? != b'=' {
            continue;
        }
        i += 1;
        while i < b.len() && is_space(b[i]) {
            i += 1;
        }
        match *b.get(i)? {
            quote @ (b'"' | b'\'') => i += b[i + 1..].iter().position(|&c| c == quote)? + 2,
            b'>' => return Some((name_end, false, i + 1)),
            _ => {
                while i < b.len() && !is_space(b[i]) && b[i] != b'>' {
                    i += 1;
                }
            }
        }
    }
}

/// Where a comment that starts at `from` (after `<!--`) ends: after `-->` or `--!>`.
fn comment_end(b: &[u8], from: usize) -> usize {
    if b.get(from) == Some(&b'>') {
        return from + 1;
    }
    if b[from.min(b.len())..].starts_with(b"->") {
        return from + 2;
    }
    let mut k = from;
    while k < b.len() {
        if b[k] == b'-' {
            if b[k + 1..].starts_with(b"->") {
                return k + 3;
            }
            if b[k + 1..].starts_with(b"-!>") {
                return k + 4;
            }
        }
        k += 1;
    }
    b.len()
}

impl<'a> Tokens<'a> {
    fn new(html: &'a str) -> Self {
        Tokens { html, at: 0, cdata: false }
    }

    fn name(&self, from: usize, to: usize) -> String {
        String::from_utf8_lossy(&self.html.as_bytes()[from..to]).to_ascii_lowercase()
    }

    /// Moves past the end tag whose name starts at `from`.
    fn skip_tag(&mut self, from: usize) {
        self.at = read_tag(self.html.as_bytes(), from).map_or(self.html.len(), |(_, _, end)| end);
    }

    /// Moves past the text of `<style>`, `<textarea>` and the like, and its end tag.
    fn skip_raw(&mut self, name: &str) {
        let b = self.html.as_bytes();
        let mut i = self.at;
        while let Some(k) = b[i..].iter().position(|&c| c == b'<') {
            i += k;
            if b.get(i + 1) == Some(&b'/') && word_at(b, i + 2, name) {
                return self.skip_tag(i + 2);
            }
            i += 1;
        }
        self.at = b.len();
    }

    /// Moves past a script and its end tag. Inside `<!--` in a script, `<script>`
    /// hides the next `</script>` from the tokenizer.
    fn skip_script(&mut self) {
        let b = self.html.as_bytes();
        let (mut i, mut escaped, mut double, mut dashes) = (self.at, false, false, 0usize);
        while i < b.len() {
            let c = b[i];
            if escaped {
                if c == b'-' {
                    dashes += 1;
                    i += 1;
                    continue;
                }
                if c == b'>' && dashes >= 2 {
                    (escaped, double) = (false, false);
                }
                dashes = 0;
            }
            if c == b'<' {
                let slash = b.get(i + 1) == Some(&b'/');
                if !double && slash && word_at(b, i + 2, "script") {
                    return self.skip_tag(i + 2);
                }
                if !escaped && b[i + 1..].starts_with(b"!--") {
                    (escaped, dashes) = (true, 2);
                    i += 4;
                    continue;
                }
                if escaped && !double && word_at(b, i + 1, "script") {
                    double = true;
                } else if double && slash && word_at(b, i + 2, "script") {
                    double = false;
                }
            }
            i += 1;
        }
        self.at = b.len();
    }

    fn next(&mut self) -> Option<Token<'a>> {
        let b = self.html.as_bytes();
        loop {
            let start = self.at;
            if start >= b.len() {
                return None;
            }
            // Text runs to the next `<` that starts markup.
            let mut i = start;
            loop {
                match b[i..].iter().position(|&c| c == b'<') {
                    None => {
                        self.at = b.len();
                        return Some(Token::Text(&self.html[start..]));
                    }
                    Some(k) => i += k,
                }
                match b.get(i + 1) {
                    Some(c) if c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?') => break,
                    _ => i += 1,
                }
            }
            if i > start {
                self.at = i;
                return Some(Token::Text(&self.html[start..i]));
            }
            let rest = &b[i..];
            if rest.starts_with(b"<!--") {
                self.at = comment_end(b, i + 4);
            } else if self.cdata && rest.starts_with(b"<![CDATA[") {
                self.at = find(b, i + 9, b"]]>").map_or(b.len(), |k| k + 3);
            } else if rest[1] == b'!' || rest[1] == b'?' {
                self.at = find(b, i + 2, b">").map_or(b.len(), |k| k + 1);
            } else if rest[1] == b'/' {
                match b.get(i + 2) {
                    Some(c) if c.is_ascii_alphabetic() => {
                        let Some((name_end, _, end)) = read_tag(b, i + 2) else {
                            self.at = b.len();
                            return None;
                        };
                        self.at = end;
                        return Some(Token::End(self.name(i + 2, name_end)));
                    }
                    Some(b'>') => self.at = i + 3,
                    Some(_) => self.at = find(b, i + 2, b">").map_or(b.len(), |k| k + 1),
                    None => {
                        self.at = b.len();
                        return Some(Token::Text(&self.html[i..]));
                    }
                }
            } else {
                let Some((name_end, closed, end)) = read_tag(b, i + 1) else {
                    self.at = b.len();
                    return None;
                };
                self.at = end;
                return Some(Token::Start { name: self.name(i + 1, name_end), closed });
            }
        }
    }
}

fn find(b: &[u8], from: usize, what: &[u8]) -> Option<usize> {
    b.get(from..)?.windows(what.len()).position(|w| w == what).map(|k| from + k)
}

struct Open {
    name: String,
    /// An SVG or MathML element.
    foreign: bool,
    /// One whose content is HTML again (see `INTEGRATION`).
    integration: bool,
    weight: usize,
}

/// Which open element the end tag `name` closes, with all after it: the last one
/// of that name, unless the parser would stop before it. None when it closes nothing.
fn closes(open: &[Open], name: &str) -> Option<usize> {
    let stops_at = if SPECIAL.contains(&name) { SCOPE } else { SPECIAL };
    for (at, element) in open.iter().enumerate().rev() {
        if element.name == name {
            return Some(at);
        }
        let passed = element.foreign && !element.integration;
        if !passed && stops_at.contains(&element.name.as_str()) {
            return None;
        }
    }
    None
}

/// Whether the elements in `html` nest deeper than `MAX_DEPTH`. `<noscript>` holds
/// text for a parser with scripting on and markup for one without: both are checked.
pub fn too_deep(html: &str) -> bool {
    deeper_than(html, MAX_DEPTH, true) || deeper_than(html, MAX_DEPTH, false)
}

fn deeper_than(html: &str, max: usize, noscript_is_text: bool) -> bool {
    let mut open: Vec<Open> = Vec::new();
    let mut depth = 0;
    let mut tokens = Tokens::new(html);
    loop {
        tokens.cdata = open.last().is_some_and(|e| e.foreign);
        let Some(token) = tokens.next() else { return false };
        match token {
            Token::Text(_) => {}
            Token::End(name) => {
                if let Some(at) = closes(&open, &name) {
                    depth -= open[at..].iter().map(|e| e.weight).sum::<usize>();
                    open.truncate(at);
                }
            }
            Token::Start { name, closed } => {
                let in_foreign = open.last().is_some_and(|e| e.foreign && !e.integration);
                let foreign = |name: String| Open { integration: INTEGRATION.contains(&name.as_str()), foreign: true, weight: 1, name };
                if in_foreign && !BREAKOUT.contains(&name.as_str()) {
                    // An SVG or MathML element: `/>` closes it.
                    if !closed {
                        open.push(foreign(name));
                        depth += 1;
                    }
                } else {
                    // An HTML element ends SVG or MathML content.
                    while open.last().is_some_and(|e| e.foreign && !e.integration) {
                        depth -= open.pop().map_or(0, |e| e.weight);
                    }
                    match name.as_str() {
                        "svg" | "math" if !closed => {
                            open.push(foreign(name));
                            depth += 1;
                        }
                        "svg" | "math" => {}
                        "script" => tokens.skip_script(),
                        "style" | "xmp" | "iframe" | "noembed" | "noframes" | "textarea" | "title" => tokens.skip_raw(&name),
                        "noscript" if noscript_is_text => tokens.skip_raw(&name),
                        // Everything after it is text.
                        "plaintext" => return false,
                        n if VOID.contains(&n) || UNNESTED.contains(&n) => {}
                        _ => {
                            let weight = if name == "table" { 4 } else { 1 };
                            open.push(Open { name, foreign: false, integration: false, weight });
                            depth += weight;
                        }
                    }
                }
            }
        }
        if depth > max {
            return true;
        }
    }
}

/// The text of `html` as paragraphs, for HTML that nests too deep to parse.
fn flatten(html: &str) -> String {
    const BLOCKS: &[&str] = &[
        "address", "article", "aside", "blockquote", "br", "dd", "details", "div", "dl", "dt", "figcaption",
        "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p",
        "pre", "section", "summary", "table", "td", "th", "tr", "ul",
    ];
    let mut text = String::new();
    let mut tokens = Tokens::new(html);
    while let Some(token) = tokens.next() {
        match token {
            Token::Text(t) => text.push_str(t),
            Token::Start { name, .. } => match name.as_str() {
                "script" => tokens.skip_script(),
                "style" | "noscript" | "template" | "iframe" | "noembed" | "noframes" | "xmp" | "textarea" | "title" => {
                    tokens.skip_raw(&name)
                }
                n if BLOCKS.contains(&n) => text.push_str("\n\n"),
                _ => {}
            },
            Token::End(name) if BLOCKS.contains(&name.as_str()) => text.push_str("\n\n"),
            Token::End(_) => {}
        }
    }
    // The text keeps its character references (`&amp;`); only `<` and `>` could
    // start markup.
    text.split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", p.replace('<', "&lt;").replace('>', "&gt;")))
        .collect()
}

/// Feed or page HTML that is safe to parse: cut at `MAX_HTML_BYTES`, and only its
/// text when its elements nest deeper than `MAX_DEPTH`.
fn bounded(html: &str) -> Cow<'_, str> {
    let mut end = html.len().min(MAX_HTML_BYTES);
    while !html.is_char_boundary(end) {
        end -= 1;
    }
    let html = &html[..end];
    if too_deep(html) {
        Cow::Owned(flatten(html))
    } else {
        Cow::Borrowed(html)
    }
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

    /// The report on 0.9.5: 50,000 nested `<div>` took 19 seconds to clean and show.
    #[test]
    fn deep_nesting_is_shown_as_text() {
        let deep = format!("{}<b>Deep</b> text &amp; more{}", "<div>".repeat(50_000), "</div>".repeat(50_000));
        let started = std::time::Instant::now();
        assert!(too_deep(&deep));
        let out = clean(&deep, None, false);
        assert_eq!(out, "<p>Deep text &amp; more</p>");
        assert_eq!(render(&deep, true).html, out);
        assert_eq!(plain_text(&deep), "Deep text & more");
        eprintln!("50,000 nested elements: {:?}", started.elapsed());

        // Text keeps its paragraphs; scripts and styles are dropped; `<` stays text.
        let deep = format!("{}<p>One < two</p><script>x()</script><style>p{{}}</style><p>Three</p>", "<span>".repeat(300));
        assert_eq!(clean(&deep, None, false), "<p>One &lt; two</p><p>Three</p>");
    }

    #[test]
    fn ordinary_html_is_not_too_deep() {
        let wide = [
            // 150 levels: below the limit.
            format!("{}x{}", "<div>".repeat(150), "</div>".repeat(150)),
            // Elements whose end tag may be left out do not nest.
            "<p>a".repeat(1000),
            format!("<ul>{}</ul>", "<li>a".repeat(1000)),
            format!("<table>{}</table>", "<tr><td>a<td>b".repeat(1000)),
            format!("<dl>{}</dl>", "<dt>a<dd>b".repeat(1000)),
            // Unclosed inline elements, closed by the block around them.
            "<div><span>a<em>b</div>".repeat(1000),
            // An inline SVG: `/>` closes its elements, `</svg>` closes the rest.
            format!("<svg>{}</svg>", "<path d='M0 0'/><g><rect/></g>".repeat(1000)),
            // Markup inside comments, scripts, styles and text areas is not markup.
            format!("<!--{}-->", "<div>".repeat(1000)),
            format!("<div><!-- a --!>{}</div>", "<b>x</b>".repeat(1000)),
            format!("<script>{}</script>", "<div>".repeat(1000)),
            format!("<script><!--<script></script>{}--></script>", "<div>".repeat(1000)),
            format!("<style>{}</style>", "<div>".repeat(1000)),
            format!("<textarea>{}</textarea>", "<div>".repeat(1000)),
            format!("<title>{}</title>", "<div>".repeat(1000)),
            // Attribute values may hold `>` and quotes.
            r#"<div title="a > b" data-x='"'>x</div>"#.repeat(1000),
            // After <plaintext> everything is text.
            format!("<plaintext>{}", "<div>".repeat(1000)),
        ];
        for html in wide {
            assert!(!too_deep(&html), "{}", &html[..80.min(html.len())]);
        }
    }

    /// HTML that a parser nests deeply, though a simpler reading of its tags would not.
    #[test]
    fn hidden_nesting_is_found() {
        let deep = [
            "<div/>".repeat(300),
            // A quote in an attribute name does not start a value.
            r#"<div a"b>"#.repeat(300),
            // `</span>` cannot close a <span> that has a <div> open inside it.
            "<span><div></span>".repeat(300),
            // `->` does not end a comment.
            "<div><!-- -> </div> -->".repeat(300),
            // Inside `<!--` in a script, `<script>` hides the next `</script>`.
            "<div><script><!--<script></script></div>--></script>".repeat(300),
            // With scripting on, <noscript> holds text.
            "<div><noscript></div></noscript>".repeat(300),
            "<div><textarea></div></textarea>".repeat(300),
            // In SVG, CDATA is text.
            format!("<svg>{}", "<g><![CDATA[></g>]]>".repeat(300)),
            // Inside <foreignObject> it is HTML again, where `/>` closes nothing.
            format!("<svg><foreignObject>{}", "<section/>".repeat(300)),
            // Each table adds a body, a row and a cell.
            "<table><tr><td>".repeat(60),
            "<b>".repeat(300),
        ];
        for html in deep {
            assert!(too_deep(&html), "{}", &html[..80.min(html.len())]);
        }
    }

    #[test]
    fn long_html_is_cut() {
        let long = format!("<p>{}</p>", "word ".repeat(1_000_000));
        let out = clean(&long, None, false);
        assert!(out.len() <= MAX_HTML_BYTES + 16, "{}", out.len());
        assert!(out.starts_with("<p>word word"));
        // Cut on a character boundary.
        let long = "é".repeat(MAX_HTML_BYTES);
        assert!(plain_text(&long).chars().all(|c| c == 'é'));
    }

    #[test]
    fn plain_text_and_truncate() {
        assert_eq!(plain_text("<p>Hello</p><p>world &amp; you</p>"), "Hello world & you");
        assert_eq!(truncate("one two three four", 9), "one two…");
    }
}
