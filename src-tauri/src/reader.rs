//! Reader mode: downloads a web page in Rust, extracts the article text and
//! cleans it. The page's own scripts, trackers and cookies never run.

use reqwest::header::ACCEPT;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::net::{self, Net};
use crate::sanitize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    pub url: String,
    pub title: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub excerpt: Option<String>,
    pub published: Option<String>,
    pub dir: Option<String>,
    /// Cleaned HTML with absolute URLs (not yet rendered).
    pub html: String,
}

pub async fn extract_url(net: &Net, raw_url: &str, strip_tracking: bool) -> Result<Extracted> {
    let url = net::parse_web_url(raw_url)?;
    let response = net
        .feed
        .get(url.as_str())
        .header(ACCEPT, "text/html, application/xhtml+xml;q=0.9, */*;q=0.5")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "The page returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let final_url = response.url().clone();
    let content_type = net::content_type(response.headers());
    if !content_type.is_empty() && !content_type.contains("html") && !content_type.contains("xml") {
        return Err(Error::msg("This link is not a web page (it may be a file or media)"));
    }
    let bytes = net::read_limited(response, net::MAX_PAGE_BYTES).await?;
    let html = net::decode_html(&bytes, &content_type);

    // Parsing is CPU work and the DOM types are not thread-safe; run it on a blocking thread.
    tokio::task::spawn_blocking(move || extract_html(&html, &final_url, strip_tracking))
        .await
        .map_err(|e| Error::msg(e.to_string()))?
}

pub fn extract_html(html: &str, url: &url::Url, strip_tracking: bool) -> Result<Extracted> {
    let mut readability = dom_smoothie::Readability::new(html, Some(url.as_str()), None)
        .map_err(|e| Error::msg(format!("Could not read this page: {e}")))?;
    let article = readability
        .parse()
        .map_err(|_| Error::msg("Could not find readable article text on this page"))?;
    let clean = sanitize::clean(&article.content, Some(url), strip_tracking);
    if sanitize::plain_text(&clean).chars().count() < 40 {
        return Err(Error::msg("Could not find readable article text on this page"));
    }
    let tidy = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    Ok(Extracted {
        url: url.to_string(),
        title: article.title.trim().to_string(),
        byline: tidy(article.byline),
        site_name: tidy(article.site_name),
        excerpt: tidy(article.excerpt),
        published: tidy(article.published_time),
        dir: tidy(article.dir),
        html: clean,
    })
}
