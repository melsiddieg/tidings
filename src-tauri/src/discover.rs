//! Finds the feed behind an address the user typed: a feed, or any page of a site,
//! such as its home page, an article, a video, a profile or a repository.
//!
//! The steps, in order. The first one that finds a feed ends the search.
//! 1. Known sites whose feed address follows from the page address (`site_feeds`):
//!    YouTube channels and playlists, Reddit, GitHub, Medium, and podcasts on
//!    Apple Podcasts or Overcast.
//! 2. The address itself, when it is a feed.
//! 3. The feeds the page names in `<link rel="alternate">`. For a YouTube video,
//!    its channel's feed.
//! 4. Links in the page that look like feeds (`feed_like_address`, link text).
//! 5. The feeds the site's home page names, when the address is another page.
//! 6. Common feed paths, such as `/feed` and `/index.xml`. Also when the page
//!    itself is refused (a bot check): the feed often is not.
//!
//! Steps 1 and 4 to 6 guess, so each guess is loaded and parsed before it is
//! offered, and the guesses of one step stop after `GUESS_TIME`. Every request
//! starts with `Net::get`. Requests go to the site the user typed, to addresses its
//! pages link to, and for known sites to the same service's feed address. For an
//! Apple Podcasts or Overcast address, Apple's lookup service (itunes.apple.com)
//! gives the podcast's feed address.

use std::time::Duration;

use dom_query::Document;
use reqwest::header::ACCEPT;
use serde::Serialize;
use tokio::time::{timeout_at, Instant};
use url::Url;

use crate::error::{Error, Result};
use crate::feeds;
use crate::net::{self, Net};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub url: String,
    pub title: String,
    pub kind: String,
    /// The feed is on the local network: the UI asks before it subscribes.
    pub local: bool,
}

const FEED_TYPES: &[(&str, &str)] = &[
    ("application/rss+xml", "RSS"),
    ("application/atom+xml", "Atom"),
    ("application/feed+json", "JSON Feed"),
    ("application/rdf+xml", "RSS"),
    ("application/x-rss+xml", "RSS"),
    ("text/xml", "RSS"),
];

const PAGE_ACCEPT: &str = "text/html, application/xhtml+xml, application/rss+xml, application/atom+xml, \
application/feed+json, application/xml;q=0.9, */*;q=0.5";

/// The guesses of one step stop after this time.
const GUESS_TIME: Duration = Duration::from_secs(30);
/// At most this many links of a page are loaded to see whether they are feeds.
const MAX_LINK_GUESSES: usize = 4;
/// Where sites often keep their feed when the page does not name it.
const COMMON_PATHS: &[&str] = &["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml"];

/// Fetches `url` and returns it as a candidate if it parses as a feed.
/// `trusted` is the address the user allowed on the local network; see `Net::get`.
async fn try_feed(net: &Net, url: &Url, trusted: Option<&Url>) -> Option<Candidate> {
    let request = net.get(url, trusted).await.ok()?;
    let response = request.header(ACCEPT, feeds::FEED_ACCEPT).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let final_url = response.url().clone();
    let bytes = net::read_limited(response, net::MAX_FEED_BYTES).await.ok()?;
    let feed = feeds::parse_raw(&bytes, &final_url).ok()?;
    Some(candidate(&final_url, feed, trusted))
}

fn candidate(url: &Url, feed: feed_rs::model::Feed, trusted: Option<&Url>) -> Candidate {
    Candidate {
        local: is_local(url, trusted),
        url: url.to_string(),
        title: feed.title.map(|t| t.content.trim().to_string()).unwrap_or_default(),
        kind: if crate::telegram::is_telegram(url) { "Telegram" } else { kind_of(&feed.feed_type) }.into(),
    }
}

/// Loads each address in turn, while time is left, and returns those that are
/// feeds. With `first`, it stops at the first feed.
async fn probe(net: &Net, urls: Vec<Url>, trusted: Option<&Url>, first: bool) -> Vec<Candidate> {
    let deadline = Instant::now() + GUESS_TIME;
    let mut out: Vec<Candidate> = Vec::new();
    for url in urls {
        if Instant::now() >= deadline {
            break;
        }
        if let Ok(Some(found)) = timeout_at(deadline, try_feed(net, &url, trusted)).await {
            if !out.iter().any(|c| c.url == found.url) {
                out.push(found);
            }
            if first {
                break;
            }
        }
    }
    out
}

/// Whether a feed address needs the user's consent for the local network: its host
/// is private by its own text, or it is the host the user just allowed (a name
/// that resolves to a private address).
fn is_local(url: &Url, trusted: Option<&Url>) -> bool {
    !net::host_looks_public(url) || trusted.is_some_and(|t| net::same_site(t, url))
}

fn kind_of(kind: &feed_rs::model::FeedType) -> &'static str {
    match kind {
        feed_rs::model::FeedType::Atom => "Atom",
        feed_rs::model::FeedType::JSON => "JSON Feed",
        _ => "RSS",
    }
}

// ---------------------------------------------------------------------------
// Known sites
// ---------------------------------------------------------------------------

/// A feed that follows from a page address on a known site.
#[derive(Debug, PartialEq)]
enum Known {
    Feed(Url),
    /// A podcast's number in Apple's directory. Apple's lookup service gives its feed.
    ApplePodcast(String),
}

/// `text` is 1 to `max` ASCII letters, digits or characters of `extra`.
fn plain(text: &str, extra: &str, max: usize) -> bool {
    (1..=max).contains(&text.len()) && text.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// A YouTube channel id: "UC" and 22 more characters.
fn is_channel_id(id: &str) -> bool {
    id.len() == 24 && id.starts_with("UC") && plain(id, "_-", 24)
}

/// First path parts on GitHub that are not user names.
const GITHUB_PAGES: &[&str] = &[
    "about", "apps", "codespaces", "collections", "contact", "customer-stories", "enterprise", "events", "explore",
    "features", "issues", "login", "marketplace", "new", "notifications", "orgs", "organizations", "pricing", "pulls",
    "readme", "search", "security", "settings", "signup", "site", "sponsors", "team", "topics", "trending",
];

/// First path parts on Medium that are not publications.
const MEDIUM_PAGES: &[&str] =
    &["_", "about", "creators", "jobs-at-medium", "m", "me", "membership", "p", "plans", "policy", "search", "topics"];

/// The feeds that follow from `url` on a known site, best first. None for an
/// address that already looks like a feed: that one is loaded as it is.
fn site_feeds(url: &Url) -> Vec<Known> {
    let Some(full_host) = url.host_str() else { return Vec::new() };
    if feed_like_address(url) {
        return Vec::new();
    }
    let host = full_host.strip_prefix("www.").or_else(|| full_host.strip_prefix("m.")).unwrap_or(full_host);
    let parts: Vec<&str> = url.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();
    let feed = |address: String| Url::parse(&address).ok().map(Known::Feed);
    let youtube = |key: &str, value: &str| {
        Url::parse_with_params("https://www.youtube.com/feeds/videos.xml", &[(key, value)]).ok().map(Known::Feed)
    };
    let found = match (host, parts.as_slice()) {
        ("youtube.com" | "music.youtube.com", ["channel", id, ..]) if is_channel_id(id) => vec![youtube("channel_id", id)],
        ("youtube.com", ["user", name, ..]) if plain(name, "_-.", 100) => vec![youtube("user", name)],
        ("youtube.com" | "music.youtube.com", ["playlist"]) => url
            .query_pairs()
            .find(|(key, _)| key == "list")
            .filter(|(_, list)| plain(list, "_-", 64))
            .map(|(_, list)| vec![youtube("playlist_id", &list)])
            .unwrap_or_default(),

        ("reddit.com" | "old.reddit.com" | "new.reddit.com" | "np.reddit.com", ["r", name, ..]) if plain(name, "_+", 200) => {
            vec![feed(format!("https://www.reddit.com/r/{name}/.rss"))]
        }
        ("reddit.com" | "old.reddit.com" | "new.reddit.com" | "np.reddit.com", ["u" | "user", name, ..])
            if plain(name, "_-", 20) =>
        {
            vec![feed(format!("https://www.reddit.com/user/{name}/.rss"))]
        }

        ("github.com", [owner]) if plain(owner, "-", 39) && !GITHUB_PAGES.contains(owner) => {
            vec![feed(format!("https://github.com/{owner}.atom"))]
        }
        ("github.com", [owner, repo, ..]) if plain(owner, "-", 39) && !GITHUB_PAGES.contains(owner) => {
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            if plain(repo, "._-", 100) && !repo.starts_with('.') {
                vec![
                    feed(format!("https://github.com/{owner}/{repo}/releases.atom")),
                    feed(format!("https://github.com/{owner}/{repo}/commits.atom")),
                ]
            } else {
                Vec::new()
            }
        }

        ("medium.com", [user, ..]) if user.strip_prefix('@').is_some_and(|name| plain(name, "_.-", 100)) => {
            vec![feed(format!("https://medium.com/feed/{user}"))]
        }
        ("medium.com", ["tag", tag, ..]) if plain(tag, "_-", 100) => vec![feed(format!("https://medium.com/feed/tag/{tag}"))],
        ("medium.com", [publication, ..]) if plain(publication, "_-", 100) && !MEDIUM_PAGES.contains(publication) => {
            vec![feed(format!("https://medium.com/feed/{publication}"))]
        }
        (h, _) if h.strip_suffix(".medium.com").is_some_and(|name| plain(name, "-", 100)) => {
            vec![feed(format!("https://{h}/feed"))]
        }

        // A public Telegram channel: its web preview (telegram.rs).
        ("t.me" | "telegram.me" | "telegram.dog", _) => {
            crate::telegram::channel(url).map(|name| vec![Some(Known::Feed(crate::telegram::preview_url(&name)))]).unwrap_or_default()
        }

        ("podcasts.apple.com" | "itunes.apple.com", _) => parts
            .iter()
            .rev()
            .find_map(|part| part.strip_prefix("id"))
            .filter(|id| plain(id, "", 20) && id.chars().all(|c| c.is_ascii_digit()))
            .map(|id| vec![Some(Known::ApplePodcast(id.to_string()))])
            .unwrap_or_default(),
        ("overcast.fm", [first, ..]) => first
            .strip_prefix("itunes")
            .filter(|id| plain(id, "", 20) && id.chars().all(|c| c.is_ascii_digit()))
            .map(|id| vec![Some(Known::ApplePodcast(id.to_string()))])
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    found.into_iter().flatten().collect()
}

/// The feed of a podcast in Apple's directory, from Apple's lookup service.
async fn apple_podcast_feed(net: &Net, id: &str) -> Option<Url> {
    let lookup = Url::parse_with_params("https://itunes.apple.com/lookup", &[("id", id), ("entity", "podcast")]).ok()?;
    let response = net.get(&lookup, None).await.ok()?.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = net::read_limited(response, 1024 * 1024).await.ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json["results"]
        .as_array()?
        .iter()
        .find_map(|result| result["feedUrl"].as_str())
        .and_then(|address| Url::parse(address).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
}

/// The feeds of a known site that load, best first.
async fn known_feeds(net: &Net, known: Vec<Known>) -> Vec<Candidate> {
    let mut urls = Vec::new();
    for item in known {
        match item {
            Known::Feed(url) => urls.push(url),
            Known::ApplePodcast(id) => urls.extend(apple_podcast_feed(net, &id).await),
        }
    }
    probe(net, urls, None, false).await
}

/// The channel of a YouTube video page: the first `"channelId":"UC…"` in the
/// page's data, which is the video's own.
fn youtube_channel(html: &str) -> Option<String> {
    const KEY: &str = "\"channelId\":\"";
    html.match_indices(KEY).find_map(|(at, _)| {
        let id = html[at + KEY.len()..].split('"').next()?;
        is_channel_id(id).then(|| id.to_string())
    })
}

fn is_youtube(url: &Url) -> bool {
    url.host_str().is_some_and(|h| matches!(h, "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"))
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// Whether an address looks like a feed by its path (`/feed`, `.rss`, `index.xml`,
/// `/feeds/…`), its host (`feeds.example.com`) or its query (`?feed=rss2`).
fn feed_like_address(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let path = path.trim_end_matches('/');
    let ends = [".rss", ".atom", ".rdf", ".xml", "/feed", "/rss", "/atom", "feed.json", "/feeds/posts/default"];
    ends.iter().any(|end| path.ends_with(end))
        || path.starts_with("/feed/")
        || path.contains("/feeds/")
        || url.host_str().is_some_and(|h| h.starts_with("feeds.") || h.starts_with("feed."))
        || url.query_pairs().any(|(key, value)| {
            key == "feed" || (matches!(&*key, "format" | "type") && matches!(&*value.to_ascii_lowercase(), "rss" | "atom" | "feed"))
        })
}

/// Whether a link's text or title calls it a feed: the word "RSS" or "Atom", or "feed" alone.
fn feed_like_text(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    text.split(|c: char| !c.is_alphanumeric()).any(|word| matches!(word, "rss" | "atom"))
        || matches!(text.as_str(), "feed" | "web feed" | "json feed")
}

/// A link title that names no feed, such as "RSS" or "Atom 1.0".
fn generic_title(title: &str) -> bool {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .all(|word| word.is_empty() || matches!(word, "rss" | "atom" | "feed" | "feeds" | "xml" | "json" | "1" | "2" | "0" | "10" | "20"))
}

/// What a page says about its feeds: the ones it names in `<link>`, and links in
/// its text that may be feeds, best first.
struct Page {
    named: Vec<Candidate>,
    guesses: Vec<Url>,
}

fn read_page(html: &str, page: &Url) -> Page {
    // Deeply nested elements make the parser slow (`sanitize::too_deep`).
    if crate::sanitize::too_deep(html) {
        return Page { named: Vec::new(), guesses: Vec::new() };
    }
    let doc = Document::from(html);
    let mut named: Vec<Candidate> = Vec::new();
    for link in doc.select("link[href]").iter() {
        let rel = link.attr("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
        if !rel.split_whitespace().any(|r| r == "alternate" || r == "feed") {
            continue;
        }
        // The type alone, without parameters: `text/xml+oembed` is not a feed.
        let kind_attr = link.attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
        let kind_attr = kind_attr.split(';').next().unwrap_or_default().trim();
        let Some((_, kind)) = FEED_TYPES.iter().find(|(t, _)| kind_attr == *t) else {
            continue;
        };
        let Some(url) = link.attr("href").and_then(|h| page.join(h.trim()).ok()) else { continue };
        if !matches!(url.scheme(), "http" | "https") || named.iter().any(|c| c.url == url.as_str()) {
            continue;
        }
        // Not the page's title, which on an article is the article's: without a
        // name of its own, the feed keeps the title it gives itself.
        let title = link.attr("title").map(|t| t.trim().to_string()).filter(|t| !generic_title(t)).unwrap_or_default();
        named.push(Candidate { local: !net::host_looks_public(&url), url: url.to_string(), title, kind: kind.to_string() });
    }
    // A site's own feed before the feeds of an article's comments.
    named.sort_by_key(|c| {
        let text = format!("{} {}", c.url, c.title).to_lowercase();
        text.contains("comment") || text.contains("评论")
    });

    let (mut by_address, mut by_text) = (Vec::new(), Vec::new());
    for a in doc.select("a[href]").iter() {
        let Some(mut url) = a.attr("href").and_then(|h| page.join(h.trim()).ok()) else { continue };
        url.set_fragment(None);
        if !matches!(url.scheme(), "http" | "https") || url == *page {
            continue;
        }
        let words = [a.text().to_string(), a.attr("title").map(|t| t.to_string()).unwrap_or_default()];
        let class = a.attr("class").map(|c| c.to_ascii_lowercase()).unwrap_or_default();
        if feed_like_address(&url) {
            by_address.push(url);
        } else if words.iter().any(|w| feed_like_text(w)) || class.contains("rss") {
            by_text.push(url);
        }
    }
    let mut guesses: Vec<Url> = Vec::new();
    for url in by_address.into_iter().chain(by_text) {
        if guesses.len() < MAX_LINK_GUESSES && !guesses.contains(&url) {
            guesses.push(url);
        }
    }
    Page { named, guesses }
}

/// The feeds a page names in `<link rel="alternate">`, a site's own before those of comments.
pub(crate) fn feed_links(html: &str, page: &Url) -> Vec<Candidate> {
    read_page(html, page).named
}

/// The feeds a site's home page names.
async fn home_page_feeds(net: &Net, page: &Url, trusted: Option<&Url>) -> Vec<Candidate> {
    let Ok(home) = page.join("/") else { return Vec::new() };
    let load = async {
        let response = net.get(&home, trusted).await.ok()?.header(ACCEPT, PAGE_ACCEPT).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let final_url = response.url().clone();
        let content_type = net::content_type(response.headers());
        let bytes = net::read_limited(response, net::MAX_PAGE_BYTES).await.ok()?;
        Some(feed_links(&net::decode_html(&bytes, &content_type), &final_url))
    };
    timeout_at(Instant::now() + GUESS_TIME, load).await.ok().flatten().unwrap_or_default()
}

fn common_paths(page: &Url) -> Vec<Url> {
    COMMON_PATHS.iter().filter_map(|path| page.join(path).ok()).collect()
}

/// `allow_local` is the user's answer to "this address is on your local network,
/// connect to it?". Without it, an address on the local network (by its own text,
/// or because its name resolves there) is not contacted; the error asks the UI to
/// put the question (`net::LOCAL_CONSENT`).
pub async fn discover(net: &Net, input: &str, allow_local: bool) -> Result<Vec<Candidate>> {
    let url = net::parse_web_url(input)?;
    if !allow_local && !net::host_looks_public(&url) {
        return Err(net::needs_consent(&url));
    }
    let trusted = allow_local.then_some(&url);

    // 1. A known site.
    let known = site_feeds(&url);
    if !known.is_empty() {
        let found = known_feeds(net, known).await;
        if !found.is_empty() {
            return Ok(found);
        }
    }

    // 2. The address itself.
    let sent = net.get(&url, trusted).await?.header(ACCEPT, PAGE_ACCEPT).send().await;
    let response = match sent {
        Ok(response) => response,
        // Refused because the name resolves to a private address: ask the user.
        Err(_) if !allow_local && net.is_private_host(&url).await => return Err(net::needs_consent(&url)),
        Err(e) => return Err(e.into()),
    };
    if !response.status().is_success() {
        let status = response.status().as_u16();
        // A bot check or a moved page: the site's feed may still be there.
        let found = probe(net, common_paths(&url), trusted, true).await;
        if !found.is_empty() {
            return Ok(found);
        }
        return Err(Error::msg(format!("The address returned HTTP {status}")));
    }
    let final_url = response.url().clone();
    let content_type = net::content_type(response.headers());
    let bytes = net::read_limited(response, net::MAX_FEED_BYTES).await?;
    if let Ok(feed) = feeds::parse_raw(&bytes, &final_url) {
        return Ok(vec![candidate(&final_url, feed, trusted)]);
    }

    // 3. The feeds the page names, or a YouTube video's channel.
    let html = net::decode_html(&bytes, &content_type);
    let page = read_page(&html, &final_url);
    if !page.named.is_empty() {
        return Ok(page.named);
    }
    if is_youtube(&final_url) {
        if let Some(channel) = youtube_channel(&html) {
            let known = site_feeds(&Url::parse(&format!("https://www.youtube.com/channel/{channel}")).expect("a valid address"));
            let found = known_feeds(net, known).await;
            if !found.is_empty() {
                return Ok(found);
            }
        }
    }
    // A redirect to a known site, such as a short link.
    if final_url != url {
        let known = site_feeds(&final_url);
        if !known.is_empty() {
            let found = known_feeds(net, known).await;
            if !found.is_empty() {
                return Ok(found);
            }
        }
    }

    // 4. Links in the page that look like feeds.
    let found = probe(net, page.guesses, trusted, false).await;
    if !found.is_empty() {
        return Ok(found);
    }

    // 5. The home page, for an article or another page of the site.
    if final_url.path() != "/" || final_url.query().is_some() {
        let found = home_page_feeds(net, &final_url, trusted).await;
        if !found.is_empty() {
            return Ok(found);
        }
    }

    // 6. Common paths.
    let found = probe(net, common_paths(&final_url), trusted, true).await;
    if !found.is_empty() {
        return Ok(found);
    }
    Err(Error::msg("No feed was found at this address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feeds_of(address: &str) -> Vec<String> {
        site_feeds(&Url::parse(address).unwrap())
            .into_iter()
            .map(|known| match known {
                Known::Feed(url) => url.to_string(),
                Known::ApplePodcast(id) => format!("apple:{id}"),
            })
            .collect()
    }

    #[test]
    fn known_sites_give_their_feeds() {
        let channel = "https://www.youtube.com/feeds/videos.xml?channel_id=UC4QobU6STFB0P71PMvOGN5A";
        assert_eq!(feeds_of("https://www.youtube.com/channel/UC4QobU6STFB0P71PMvOGN5A/videos"), [channel]);
        assert_eq!(feeds_of("https://m.youtube.com/channel/UC4QobU6STFB0P71PMvOGN5A"), [channel]);
        assert_eq!(
            feeds_of("https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"),
            ["https://www.youtube.com/feeds/videos.xml?playlist_id=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"]
        );
        assert_eq!(feeds_of("https://www.youtube.com/user/google"), ["https://www.youtube.com/feeds/videos.xml?user=google"]);
        // A Telegram channel, from its link, a post or its preview.
        for link in ["https://t.me/durov", "https://t.me/durov/569", "https://t.me/s/durov", "https://telegram.me/durov"] {
            assert_eq!(feeds_of(link), ["https://t.me/s/durov"], "{link}");
        }
        assert!(feeds_of("https://t.me/joinchat/AbCdEf").is_empty());
        // A handle or a video needs the page.
        assert!(feeds_of("https://www.youtube.com/@YouTube").is_empty());
        assert!(feeds_of("https://www.youtube.com/watch?v=jNQXAC9IVRw").is_empty());
        assert!(feeds_of("https://www.youtube.com/channel/not-a-channel").is_empty());

        assert_eq!(feeds_of("https://old.reddit.com/r/rust/comments/abc/some_post/"), ["https://www.reddit.com/r/rust/.rss"]);
        assert_eq!(feeds_of("https://www.reddit.com/u/spez"), ["https://www.reddit.com/user/spez/.rss"]);
        // Already a feed: loaded as it is.
        assert!(feeds_of("https://www.reddit.com/r/rust/.rss").is_empty());

        assert_eq!(
            feeds_of("https://github.com/rust-lang/rust/issues"),
            ["https://github.com/rust-lang/rust/releases.atom", "https://github.com/rust-lang/rust/commits.atom"]
        );
        assert_eq!(
            feeds_of("https://github.com/tauri-apps/tauri.git"),
            ["https://github.com/tauri-apps/tauri/releases.atom", "https://github.com/tauri-apps/tauri/commits.atom"]
        );
        assert_eq!(feeds_of("https://github.com/torvalds"), ["https://github.com/torvalds.atom"]);
        assert!(feeds_of("https://github.com/features/actions").is_empty());
        assert!(feeds_of("https://github.com/rust-lang/rust/releases.atom").is_empty());

        assert_eq!(feeds_of("https://medium.com/@ev/some-story-123"), ["https://medium.com/feed/@ev"]);
        assert_eq!(feeds_of("https://medium.com/tag/rust"), ["https://medium.com/feed/tag/rust"]);
        assert_eq!(feeds_of("https://medium.com/better-programming/a-story"), ["https://medium.com/feed/better-programming"]);
        assert_eq!(feeds_of("https://someone.medium.com/a-story"), ["https://someone.medium.com/feed"]);
        assert!(feeds_of("https://medium.com/feed/@ev").is_empty());
        assert!(feeds_of("https://medium.com/m/signin").is_empty());

        assert_eq!(feeds_of("https://podcasts.apple.com/us/podcast/the-daily/id1200361736"), ["apple:1200361736"]);
        assert_eq!(feeds_of("https://podcasts.apple.com/us/podcast/the-daily/id1200361736?i=1000"), ["apple:1200361736"]);
        assert_eq!(feeds_of("https://overcast.fm/itunes1200361736/the-daily"), ["apple:1200361736"]);
        assert!(feeds_of("https://overcast.fm/+AbCdEf").is_empty());

        assert!(feeds_of("https://example.com/blog/post").is_empty());
    }

    #[test]
    fn a_youtube_video_names_its_channel() {
        let html = r#"<script>var ytInitialPlayerResponse = {"videoDetails":{"videoId":"jNQXAC9IVRw","channelId":"UC4QobU6STFB0P71PMvOGN5A"}};
            var ytInitialData = {"channelId":"UCBR8-60-B28hp2BmDPdntcQ"};</script>"#;
        assert_eq!(youtube_channel(html).as_deref(), Some("UC4QobU6STFB0P71PMvOGN5A"));
        assert_eq!(youtube_channel(r#"{"channelId":"nope"} {"channelId":"UCBR8-60-B28hp2BmDPdntcQ"}"#).as_deref(), Some("UCBR8-60-B28hp2BmDPdntcQ"));
        assert_eq!(youtube_channel("<p>no data</p>"), None);
    }

    #[test]
    fn pages_name_and_link_their_feeds() {
        let page = Url::parse("https://blog.example/2024/05/a-post").unwrap();
        let html = r#"<title>A post</title>
            <link rel="alternate" type="application/rss+xml" title="A post » Comments Feed" href="/2024/05/a-post/feed/">
            <link rel="alternate" type="application/rss+xml" title="Blog » Feed" href="/feed/">
            <link rel="alternate" type="application/json+oembed" href="/oembed">
            <link rel="alternate" type="text/xml+oembed" href="/oembed?format=xml">
            <link rel="alternate" type="application/atom+xml; charset=utf-8" href="/atom.xml">"#;
        let named = feed_links(html, &page);
        assert_eq!(
            named.iter().map(|c| (c.url.as_str(), c.title.as_str())).collect::<Vec<_>>(),
            [("https://blog.example/feed/", "Blog » Feed"), ("https://blog.example/atom.xml", ""), ("https://blog.example/2024/05/a-post/feed/", "A post » Comments Feed")]
        );
        assert!(generic_title("RSS") && generic_title("Atom 1.0") && generic_title(" RSS 2.0 Feed ") && generic_title(""));
        assert!(!generic_title("Blog » Feed") && !generic_title("Podcast (MP3)"));

        let html = r#"<a href="/about">About</a><a href="https://twitter.com/blog">Twitter</a>
            <a href="/subscribe" class="icon-rss"></a><a href="/index.xml#top">Subscribe</a>
            <a href="https://feeds.feedburner.com/blog">Updates</a><a title="RSS feed" href="/posts">Posts</a>
            <a href="/index.xml">again</a><a href="mailto:me@blog.example">RSS by mail</a>"#;
        let page = read_page(html, &page);
        assert!(page.named.is_empty());
        assert_eq!(
            page.guesses.iter().map(Url::as_str).collect::<Vec<_>>(),
            ["https://blog.example/index.xml", "https://feeds.feedburner.com/blog", "https://blog.example/subscribe", "https://blog.example/posts"]
        );
    }

    #[test]
    fn feed_addresses_are_recognised() {
        for yes in [
            "https://a.example/feed", "https://a.example/feed/", "https://a.example/rss.xml", "https://a.example/index.xml",
            "https://a.example/blog/atom", "https://a.example/feed/@user", "https://a.example/feeds/posts/default",
            "https://a.example/?feed=rss2", "https://a.example/news?format=RSS", "https://feeds.example.com/show",
            "https://a.example/feed.json", "https://a.example/r/x/.rss",
        ] {
            assert!(feed_like_address(&Url::parse(yes).unwrap()), "{yes}");
        }
        for no in ["https://a.example/", "https://a.example/feedback", "https://a.example/rss-reader-review", "https://a.example/?q=feed"] {
            assert!(!feed_like_address(&Url::parse(no).unwrap()), "{no}");
        }
        assert!(feed_like_text(" RSS ") && feed_like_text("Atom feed") && feed_like_text("RSS 订阅") && feed_like_text("Feed"));
        assert!(!feed_like_text("Feedback") && !feed_like_text("News feed settings") && !feed_like_text("Newsletter"));
    }
}
