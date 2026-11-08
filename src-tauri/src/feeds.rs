//! Fetching, parsing and storing feeds.

use std::sync::Arc;

use dom_query::Document;
use feed_rs::model;
use reqwest::header::{ACCEPT, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::StatusCode;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Semaphore;
use url::Url;

use crate::db::{self, FeedMeta, FetchInfo, NewArticle};
use crate::error::{Error, Result};
use crate::net::{self, Net};
use crate::sanitize;
use crate::settings::Settings;
use crate::state::AppState;

pub const FEED_ACCEPT: &str = "application/rss+xml, application/atom+xml, application/feed+json, \
    application/json;q=0.9, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.5";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedUpdated {
    pub feed_id: i64,
    pub new_articles: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProgress {
    pub done: usize,
    pub total: usize,
}

struct InflightGuard<'a> {
    state: &'a AppState,
    id: i64,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.state.inflight.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
    }
}

/// Refreshes several feeds in parallel (bounded by the concurrency setting)
/// and reports progress to the UI.
pub async fn refresh_many(app: &AppHandle, state: &Arc<AppState>, ids: Vec<i64>) {
    if ids.is_empty() {
        return;
    }
    {
        let mut p = state.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.total += ids.len();
        let _ = app.emit("refresh-progress", RefreshProgress { done: p.done, total: p.total });
    }
    let semaphore = Arc::new(Semaphore::new(state.settings().concurrency.max(1) as usize));
    let mut tasks = tokio::task::JoinSet::new();
    for id in ids {
        let (app, state, semaphore) = (app.clone(), state.clone(), semaphore.clone());
        tasks.spawn(async move {
            let _permit = semaphore.acquire_owned().await;
            refresh_one(&app, &state, id).await;
            let event = {
                let mut p = state.progress.lock().unwrap_or_else(|e| e.into_inner());
                p.done += 1;
                let event = RefreshProgress { done: p.done, total: p.total };
                if p.done >= p.total {
                    p.done = 0;
                    p.total = 0;
                }
                event
            };
            let _ = app.emit("refresh-progress", event);
        });
    }
    while tasks.join_next().await.is_some() {}
}

pub async fn refresh_one(app: &AppHandle, state: &AppState, id: i64) -> FeedUpdated {
    {
        let mut inflight = state.inflight.lock().unwrap_or_else(|e| e.into_inner());
        if !inflight.insert(id) {
            return FeedUpdated { feed_id: id, new_articles: 0, error: None };
        }
    }
    let _guard = InflightGuard { state, id };
    let event = match fetch_and_store(state, id).await {
        Ok(new_articles) => FeedUpdated { feed_id: id, new_articles, error: None },
        Err(err) => {
            let message = err.to_string();
            let _ = db::set_fetch_error(&state.db(), id, &message);
            FeedUpdated { feed_id: id, new_articles: 0, error: Some(message) }
        }
    };
    let _ = app.emit("feed-updated", event.clone());
    event
}

pub fn retention_for(mode: Option<&str>, value: Option<i64>, s: &Settings) -> (String, i64) {
    match mode {
        Some("all") => ("all".into(), 0),
        Some("count") => ("count".into(), value.unwrap_or(s.retention_count as i64)),
        Some("days") => ("days".into(), value.unwrap_or(s.retention_days as i64)),
        _ => match s.retention_mode.as_str() {
            "count" => ("count".into(), s.retention_count as i64),
            "days" => ("days".into(), s.retention_days as i64),
            _ => ("all".into(), 0),
        },
    }
}

fn http_error(status: StatusCode) -> Error {
    let text = match status.as_u16() {
        401 | 403 => "The server refused access to this feed".to_string(),
        404 => "The feed was not found on the server (HTTP 404)".to_string(),
        410 => "The feed has been removed by its publisher (HTTP 410)".to_string(),
        429 => "The server is rate limiting requests. Try again later".to_string(),
        500..=599 => format!("The server had an error (HTTP {})", status.as_u16()),
        _ => format!("Server returned HTTP {}", status.as_u16()),
    };
    Error::msg(text)
}

async fn fetch_and_store(state: &AppState, id: i64) -> Result<usize> {
    let info: FetchInfo = db::fetch_info(&state.db(), id)?;
    let settings = state.settings();
    let net = state.net();

    let feed_url = Url::parse(&info.url).map_err(|_| Error::msg("The feed address is not valid"))?;
    let mut request = net.get(&feed_url, Some(&feed_url)).await?.header(ACCEPT, FEED_ACCEPT);
    if let Some(etag) = &info.etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    if let Some(modified) = &info.last_modified {
        request = request.header(IF_MODIFIED_SINCE, modified);
    }
    let response = request.send().await?;
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        db::set_not_modified(&state.db(), id)?;
        return Ok(0);
    }
    if !status.is_success() {
        return Err(http_error(status));
    }
    let final_url = response.url().clone();
    let header = |name: reqwest::header::HeaderName| {
        response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
    };
    let etag = header(ETAG);
    let last_modified = header(LAST_MODIFIED);
    let bytes = net::read_limited(response, net::MAX_FEED_BYTES).await?;
    let (parse_url, strip) = (final_url.clone(), settings.strip_tracking);
    let parsed = tokio::task::spawn_blocking(move || parse_feed(&bytes, &parse_url, strip))
        .await
        .map_err(|e| Error::msg(e.to_string()))??;

    let (mode, value) =
        retention_for(info.retention_mode.as_deref(), info.retention_value, &settings);
    let cutoff = (mode == "days").then(|| db::now() - value * 86_400);
    let added = {
        let mut conn = state.db();
        let tx = conn.transaction()?;
        let added = db::store_articles(&tx, id, &parsed.articles, cutoff)?;
        db::update_feed_meta(
            &tx,
            id,
            &FeedMeta {
                title: parsed.title.clone(),
                site_url: parsed.site_url.clone(),
                description: parsed.description.clone(),
                etag,
                last_modified,
            },
        )?;
        db::apply_retention(&tx, id, &mode, value)?;
        db::detect_feed_type(&tx, id)?;
        tx.commit()?;
        added
    };

    let icon_due = info.icon_checked.is_none_or(|t| db::now() - t > 7 * 86_400);
    if !info.has_icon && icon_due {
        let site = parsed.site_url.as_deref().or(info.site_url.as_deref());
        let icon = fetch_icon(&net, site, parsed.icon_url.as_deref(), &final_url).await;
        let _ = db::set_icon(&state.db(), id, icon.as_ref().map(|(b, m)| (b.as_slice(), m.as_str())));
    }
    Ok(added)
}

pub struct ParsedFeed {
    pub title: Option<String>,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub articles: Vec<NewArticle>,
}

pub fn parse_raw(bytes: &[u8], url: &Url) -> Result<model::Feed> {
    feed_rs::parser::Builder::new()
        .base_uri(Some(url.as_str()))
        .build()
        .parse(bytes)
        .map_err(|e| {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
            if head.contains("<html") || head.contains("<!doctype html") {
                Error::msg("This address is a web page, not a feed")
            } else {
                Error::msg(format!("Could not read the feed: {e}"))
            }
        })
}

pub fn parse_feed(bytes: &[u8], url: &Url, strip_tracking: bool) -> Result<ParsedFeed> {
    let feed = parse_raw(bytes, url)?;
    let title = feed.title.as_ref().map(text_plain).filter(|t| !t.is_empty());
    let site_url = feed
        .links
        .iter()
        .filter(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .filter(|l| !l.media_type.as_deref().is_some_and(|t| t.contains("xml") || t.contains("json")))
        .filter_map(|l| url.join(&l.href).ok())
        .find(|u| matches!(u.scheme(), "http" | "https") && u != url)
        .map(|u| u.to_string());
    let description = feed
        .description
        .as_ref()
        .map(text_plain)
        .filter(|d| !d.is_empty())
        .map(|d| sanitize::truncate(&d, 500));
    let icon_url = feed.icon.as_ref().map(|i| i.uri.clone());
    let articles = feed.entries.iter().map(|e| entry_to_article(e, url, strip_tracking)).collect();
    Ok(ParsedFeed { title, site_url, description, icon_url, articles })
}

fn is_html_type(t: &impl ToString) -> bool {
    let t = t.to_string();
    t.contains("html")
}

fn text_plain(text: &model::Text) -> String {
    if is_html_type(&text.content_type) {
        sanitize::plain_text(&text.content)
    } else {
        text.content.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

fn web_url(raw: &str, base: &Url) -> Option<String> {
    let url = base.join(raw.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

fn pick_link(links: &[model::Link], base: &Url) -> Option<String> {
    let usable = |l: &&model::Link| l.rel.as_deref() != Some("enclosure") && l.rel.as_deref() != Some("self");
    links
        .iter()
        .filter(usable)
        .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .or_else(|| links.iter().find(usable))
        .and_then(|l| web_url(&l.href, base))
}

fn media_html(entry: &model::Entry) -> String {
    let mut html = String::new();
    for media in &entry.media {
        if let Some(thumb) = media.thumbnails.first() {
            html.push_str(&format!("<p><img src=\"{}\" alt=\"\"></p>", sanitize::escape(&thumb.image.uri)));
        }
        if let Some(desc) = &media.description {
            if is_html_type(&desc.content_type) {
                html.push_str(&desc.content);
            } else {
                html.push_str(&sanitize::text_to_html(&desc.content));
            }
        }
    }
    html
}

fn enclosure(entry: &model::Entry, base: &Url) -> (Option<String>, Option<String>) {
    if let Some(link) = entry.links.iter().find(|l| l.rel.as_deref() == Some("enclosure")) {
        return (web_url(&link.href, base), link.media_type.clone());
    }
    for media in &entry.media {
        for content in &media.content {
            let mime = content.content_type.as_ref().map(|t| t.to_string());
            let is_av = mime.as_deref().is_some_and(|m| m.starts_with("audio/") || m.starts_with("video/"));
            if is_av {
                if let Some(url) = &content.url {
                    return (web_url(url.as_str(), base), mime);
                }
            }
        }
    }
    (None, None)
}

fn media_image(entry: &model::Entry) -> Option<String> {
    for media in &entry.media {
        if let Some(thumb) = media.thumbnails.first() {
            return Some(thumb.image.uri.clone());
        }
        for content in &media.content {
            let is_image = content.content_type.as_ref().is_some_and(|t| t.to_string().starts_with("image/"));
            if is_image {
                if let Some(url) = &content.url {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

fn entry_to_article(entry: &model::Entry, feed_url: &Url, strip_tracking: bool) -> NewArticle {
    let mut link = pick_link(&entry.links, feed_url);
    if strip_tracking {
        link = link.map(|l| sanitize::strip_tracking_params(&l));
    }
    let base = entry
        .base
        .as_deref()
        .and_then(|b| feed_url.join(b).ok())
        .or_else(|| link.as_deref().and_then(|l| Url::parse(l).ok()))
        .unwrap_or_else(|| feed_url.clone());

    let body = entry.content.as_ref().and_then(|c| {
        let text = c.body.as_deref()?.trim();
        if text.is_empty() {
            return None;
        }
        Some(if is_html_type(&c.content_type) || text.contains('<') {
            text.to_string()
        } else {
            sanitize::text_to_html(text)
        })
    });
    let summary = entry.summary.as_ref().and_then(|s| {
        let text = s.content.trim();
        if text.is_empty() {
            return None;
        }
        Some(if is_html_type(&s.content_type) || text.contains('<') {
            text.to_string()
        } else {
            sanitize::text_to_html(text)
        })
    });
    let raw = body.or(summary).unwrap_or_else(|| media_html(entry));
    let content = sanitize::clean(&raw, Some(&base), strip_tracking);
    let text = sanitize::plain_text(&content);
    let excerpt = sanitize::truncate(&text, 280);

    let title = entry
        .title
        .as_ref()
        .map(text_plain)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| if text.is_empty() { "Untitled".into() } else { sanitize::truncate(&text, 90) });
    let authors: Vec<String> = entry
        .authors
        .iter()
        .filter_map(|p| p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string))
        .collect();
    let author = (!authors.is_empty()).then(|| authors.join(", "));

    let image_url = media_image(entry)
        .and_then(|u| web_url(&u, &base))
        .or_else(|| sanitize::first_image(&content));
    let (enclosure_url, enclosure_type) = enclosure(entry, &base);
    let published = entry.published.or(entry.updated).map(|d| d.timestamp());
    let hash = sanitize::hash(&[&title, link.as_deref().unwrap_or(""), &content]);
    let guid = if entry.id.trim().is_empty() {
        link.clone().unwrap_or_else(|| format!("hash:{hash}"))
    } else {
        entry.id.clone()
    };

    NewArticle {
        guid,
        url: link,
        title,
        author,
        excerpt,
        content,
        image_url,
        enclosure_url,
        enclosure_type,
        published,
        hash,
    }
}

// ---------------------------------------------------------------------------
// Favicons
// ---------------------------------------------------------------------------

fn icon_links(html: &str, page: &Url) -> Vec<Url> {
    let doc = Document::from(html);
    let mut scored: Vec<(i32, Url)> = Vec::new();
    for link in doc.select("link[rel][href]").iter() {
        let rel = link.attr("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
        let rels: Vec<&str> = rel.split_whitespace().collect();
        let sizes = link.attr("sizes").map(|s| s.to_string()).unwrap_or_default();
        let kind = link.attr("type").map(|s| s.to_string()).unwrap_or_default();
        let score = if rels.contains(&"icon") {
            if kind.contains("svg") || sizes.contains("32") || sizes.contains("64") { 4 } else { 3 }
        } else if rels.contains(&"apple-touch-icon") || rels.contains(&"apple-touch-icon-precomposed") {
            2
        } else {
            continue;
        };
        if let Some(url) = link.attr("href").and_then(|h| page.join(h.trim()).ok()) {
            scored.push((score, url));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, u)| u).collect()
}

async fn download_icon(net: &Net, url: &Url, feed_url: &Url) -> Option<(Vec<u8>, String)> {
    let response = net.get(url, Some(feed_url)).await.ok()?.header(ACCEPT, "image/*").send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = net::content_type(response.headers());
    let bytes = net::read_limited(response, net::MAX_ICON_BYTES).await.ok()?;
    let mime = net::sniff_image(&bytes, &content_type)?;
    Some((bytes, mime))
}

pub async fn fetch_icon(
    net: &Net,
    site_url: Option<&str>,
    feed_icon: Option<&str>,
    feed_url: &Url,
) -> Option<(Vec<u8>, String)> {
    // A feed controls its site link and icon URLs. Only follow them to public hosts,
    // or to the feed's own host (for feeds on a private network). `Net::get`
    // also checks DNS answers.
    let allowed = |u: &Url| net::host_looks_public(u) || net::same_host(u, feed_url);
    let site = site_url
        .and_then(|s| Url::parse(s).ok())
        .filter(|u| allowed(u))
        .unwrap_or_else(|| feed_url.clone());
    let mut candidates: Vec<Url> = Vec::new();

    let request = net.get(&site, Some(feed_url)).await;
    if let Ok(response) = async { request?.header(ACCEPT, "text/html").send().await.map_err(Error::from) }.await {
        if response.status().is_success() {
            let page = response.url().clone();
            let content_type = net::content_type(response.headers());
            if let Ok(bytes) = net::read_limited(response, 2 * 1024 * 1024).await {
                candidates.extend(icon_links(&net::decode_html(&bytes, &content_type), &page));
            }
        }
    }
    if let Some(icon) = feed_icon.and_then(|i| feed_url.join(i).ok()) {
        candidates.push(icon);
    }
    for base in [&site, feed_url] {
        if let Ok(url) = base.join("/favicon.ico") {
            candidates.push(url);
        }
    }

    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        if !matches!(candidate.scheme(), "http" | "https")
            || !allowed(&candidate)
            || !seen.insert(candidate.to_string())
        {
            continue;
        }
        if seen.len() > 6 {
            break;
        }
        if let Some(icon) = download_icon(net, &candidate, feed_url).await {
            return Some(icon);
        }
    }
    None
}
