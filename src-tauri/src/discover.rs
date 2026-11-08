//! Finds the feed behind an address the user typed. Accepts a feed URL or a
//! normal web page that links to its feed.

use dom_query::Document;
use reqwest::header::ACCEPT;
use serde::Serialize;
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
}

const FEED_TYPES: &[(&str, &str)] = &[
    ("application/rss+xml", "RSS"),
    ("application/atom+xml", "Atom"),
    ("application/feed+json", "JSON Feed"),
    ("application/rdf+xml", "RSS"),
    ("application/x-rss+xml", "RSS"),
    ("text/xml", "RSS"),
];

/// Fetches `url` and returns it as a candidate if it parses as a feed.
/// `typed` is the address the user typed; see `Net::get`.
async fn try_feed(net: &Net, url: &Url, typed: &Url) -> Option<Candidate> {
    let request = net.get(url, Some(typed)).await.ok()?;
    let response = request.header(ACCEPT, feeds::FEED_ACCEPT).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let final_url = response.url().clone();
    let bytes = net::read_limited(response, net::MAX_FEED_BYTES).await.ok()?;
    let feed = feeds::parse_raw(&bytes, &final_url).ok()?;
    Some(Candidate {
        url: final_url.to_string(),
        title: feed.title.map(|t| t.content.trim().to_string()).unwrap_or_default(),
        kind: kind_of(&feed.feed_type).into(),
    })
}

fn kind_of(kind: &feed_rs::model::FeedType) -> &'static str {
    match kind {
        feed_rs::model::FeedType::Atom => "Atom",
        feed_rs::model::FeedType::JSON => "JSON Feed",
        _ => "RSS",
    }
}

fn feed_links(html: &str, page: &Url) -> Vec<Candidate> {
    let doc = Document::from(html);
    let page_title = doc.select("title").text().trim().to_string();
    let mut out: Vec<Candidate> = Vec::new();
    for link in doc.select("link[href]").iter() {
        let rel = link.attr("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
        if !rel.split_whitespace().any(|r| r == "alternate" || r == "feed") {
            continue;
        }
        let kind_attr = link.attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
        let Some((_, kind)) = FEED_TYPES.iter().find(|(t, _)| kind_attr.starts_with(t)) else {
            continue;
        };
        let Some(url) = link.attr("href").and_then(|h| page.join(h.trim()).ok()) else { continue };
        if !matches!(url.scheme(), "http" | "https") || out.iter().any(|c| c.url == url.as_str()) {
            continue;
        }
        let title = link
            .attr("title")
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| page_title.clone());
        out.push(Candidate { url: url.to_string(), title, kind: kind.to_string() });
    }
    out
}

pub async fn discover(net: &Net, input: &str) -> Result<Vec<Candidate>> {
    let url = net::parse_web_url(input)?;
    let response = net
        .get(&url, Some(&url))
        .await?
        .header(ACCEPT, "text/html, application/xhtml+xml, application/rss+xml, application/atom+xml, application/feed+json, application/xml;q=0.9, */*;q=0.5")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "The address returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let final_url = response.url().clone();
    let content_type = net::content_type(response.headers());
    let bytes = net::read_limited(response, net::MAX_FEED_BYTES).await?;

    if let Ok(feed) = feeds::parse_raw(&bytes, &final_url) {
        return Ok(vec![Candidate {
            url: final_url.to_string(),
            title: feed.title.map(|t| t.content.trim().to_string()).unwrap_or_default(),
            kind: kind_of(&feed.feed_type).into(),
        }]);
    }

    let html = net::decode_html(&bytes, &content_type);
    let mut found = feed_links(&html, &final_url);
    if found.is_empty() {
        for path in ["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml"] {
            let Ok(guess) = final_url.join(path) else { continue };
            if let Some(candidate) = try_feed(net, &guess, &url).await {
                found.push(candidate);
                break;
            }
        }
    }
    if found.is_empty() {
        return Err(Error::msg("No feed was found at this address"));
    }
    Ok(found)
}
