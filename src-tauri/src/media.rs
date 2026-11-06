//! Podcast audio streaming for the built-in player.
//!
//! The web view asks for `tidings://localhost/media/<base64url>` with a `Range`
//! header. Rust fetches that byte range from the audio host and returns it, so
//! the player can seek without downloading the whole file, and the host never
//! talks to the web view (no cookies, no referrer, proxy setting applies).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use url::Url;

use crate::error::{Error, Result};
use crate::net::{self, Net};

/// Largest slice returned for one request. Players ask for the next slice as they go.
pub const CHUNK: u64 = 2 * 1024 * 1024;
/// Upper bound when a server ignores ranges and we must read from the start.
const MAX_SKIP: u64 = 400 * 1024 * 1024;

/// Final URLs after redirects, so each slice does not repeat the redirect chain.
#[derive(Default)]
pub struct ResolvedUrls(Mutex<HashMap<String, (String, Instant)>>);

impl ResolvedUrls {
    fn get(&self, key: &str) -> Option<String> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key)
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(20 * 60))
            .map(|(u, _)| u.clone())
    }
    fn put(&self, key: &str, value: &str) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 200 {
            map.clear();
        }
        map.insert(key.to_string(), (value.to_string(), Instant::now()));
    }
    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

/// How many leading path segments of a known podcast tracking redirect to drop.
/// These services sit in front of the real audio URL to count downloads.
fn tracker_prefix(host: &str, seg: &[&str]) -> Option<usize> {
    let s = |i: usize| seg.get(i).copied().unwrap_or("");
    match host {
        "dts.podtrac.com" | "podtrac.com" if s(0).starts_with("redirect.") => Some(1),
        "www.podtrac.com" if s(0) == "pts" && s(1).starts_with("redirect.") => Some(2),
        "chtbl.com" | "chrt.fm" | "tracking.swap.fm" if s(0) == "track" => Some(2),
        "pdst.fm" | "prfx.byspotify.com" if s(0) == "e" => Some(1),
        "mgln.ai" if s(0) == "e" => Some(2),
        "op3.dev" if s(0) == "e" || s(0).starts_with("e,") => Some(1),
        "pfx.vpixl.com" => Some(1),
        "arttrk.com" if s(0) == "p" => Some(2),
        "verifi.podscribe.com" | "pscrb.fm" if s(0) == "rss" && s(1) == "p" => Some(2),
        "claritaspod.com" if s(0) == "measure" => Some(1),
        "clrtpod.com" if s(0) == "m" => Some(1),
        _ => None,
    }
}

/// Removes known tracking redirects from a podcast enclosure URL.
/// `dts.podtrac.com/redirect.mp3/mgln.ai/e/211/rss.art19.com/x.mp3` → `https://rss.art19.com/x.mp3`
pub fn strip_trackers(raw: &str) -> String {
    let mut current = raw.to_string();
    for _ in 0..8 {
        let Ok(url) = Url::parse(&current) else { break };
        let Some(host) = url.host_str() else { break };
        let path = url.path().trim_start_matches('/');
        let segments: Vec<&str> = path.split('/').collect();
        let Some(skip) = tracker_prefix(host, &segments) else { break };
        let rest = segments.get(skip..).map(|r| r.join("/")).unwrap_or_default();
        // Some prefixes keep the scheme inside the path ("https:/host/..." or "https://host/...").
        let (scheme, rest) = if let Some(r) = rest.strip_prefix("http:/") {
            ("http", r.trim_start_matches('/'))
        } else if let Some(r) = rest.strip_prefix("https:/") {
            ("https", r.trim_start_matches('/'))
        } else {
            ("https", rest.as_str())
        };
        let mut next = format!("{scheme}://{rest}");
        if let Some(q) = url.query() {
            next.push('?');
            next.push_str(q);
        }
        match Url::parse(&next) {
            Ok(u) if u.host_str().is_some_and(|h| h.contains('.')) => current = u.to_string(),
            _ => break,
        }
    }
    current
}

/// A parsed `Range: bytes=start-end` header (only the first range is used).
pub fn parse_range(header: Option<&str>) -> (u64, Option<u64>) {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return (0, None) };
    let first = spec.split(',').next().unwrap_or("");
    let (a, b) = first.split_once('-').unwrap_or((first, ""));
    match (a.trim().parse::<u64>(), b.trim().parse::<u64>()) {
        (Ok(start), Ok(end)) if end >= start => (start, Some(end)),
        (Ok(start), _) => (start, None),
        _ => (0, None),
    }
}

pub struct Chunk {
    pub status: u16,
    pub mime: String,
    pub bytes: Vec<u8>,
    /// "bytes start-end/total" for 206 responses.
    pub content_range: Option<String>,
}

fn media_type(header: &str, url: &str) -> String {
    let h = header.split(';').next().unwrap_or("").trim();
    if h.starts_with("audio/") || h.starts_with("video/") {
        return h.to_string();
    }
    let lower = url.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or("");
    for (ext, mime) in [(".m4a", "audio/mp4"), (".mp4", "audio/mp4"), (".aac", "audio/aac"), (".ogg", "audio/ogg"), (".oga", "audio/ogg"), (".opus", "audio/ogg"), (".flac", "audio/flac"), (".wav", "audio/wav")] {
        if path.ends_with(ext) {
            return mime.to_string();
        }
    }
    "audio/mpeg".to_string()
}

/// Fetches one slice of an audio file. `range` is the player's `Range` header.
pub async fn fetch(net: &Net, cache: &ResolvedUrls, source: &str, range: Option<&str>) -> Result<Chunk> {
    let (start, end) = parse_range(range);
    let end = end.unwrap_or(start + CHUNK - 1).min(start + CHUNK - 1);

    let mut retried = false;
    loop {
        let url = cache.get(source).unwrap_or_else(|| source.to_string());
        let response = net
            .image
            .get(&url)
            .header(ACCEPT, "audio/*, video/*;q=0.8, */*;q=0.5")
            .header(RANGE, format!("bytes={start}-{end}"))
            .timeout(Duration::from_secs(60))
            .send()
            .await;
        let response = match response {
            Ok(r) => r,
            Err(_) if !retried && url != source => {
                // A cached CDN link may have expired; resolve again once.
                cache.forget(source);
                retried = true;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let status = response.status();
        if !status.is_success() {
            if !retried && url != source {
                cache.forget(source);
                retried = true;
                continue;
            }
            if status == StatusCode::RANGE_NOT_SATISFIABLE {
                return Ok(Chunk { status: 416, mime: "audio/mpeg".into(), bytes: Vec::new(), content_range: None });
            }
            return Err(Error::msg(format!("The audio host returned HTTP {}", status.as_u16())));
        }
        cache.put(source, response.url().as_str());
        let mime = media_type(&net::content_type(response.headers()), response.url().as_str());

        if status == StatusCode::PARTIAL_CONTENT {
            let content_range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = net::read_limited(response, CHUNK as usize + 64 * 1024).await?;
            return Ok(Chunk { status: 206, mime, bytes, content_range });
        }

        // The server ignored the range and sent the whole file. Read up to the
        // requested slice and answer as if it had honored the range.
        let total = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if start > MAX_SKIP {
            return Err(Error::msg("This audio host does not support seeking this far"));
        }
        let mut response = response;
        let mut offset = 0u64;
        let mut out = Vec::new();
        while let Some(piece) = response.chunk().await? {
            let piece_start = offset;
            offset += piece.len() as u64;
            if offset <= start {
                continue;
            }
            let from = start.saturating_sub(piece_start) as usize;
            out.extend_from_slice(&piece[from..]);
            if out.len() as u64 >= end - start + 1 {
                out.truncate((end - start + 1) as usize);
                break;
            }
        }
        let real_end = start + out.len().saturating_sub(1) as u64;
        let total = total.map(|t| t.to_string()).unwrap_or_else(|| "*".into());
        return Ok(Chunk {
            status: 206,
            mime,
            bytes: out,
            content_range: Some(format!("bytes {start}-{real_end}/{total}")),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_podcast_trackers() {
        assert_eq!(
            strip_trackers("https://dts.podtrac.com/redirect.mp3/mgln.ai/e/211/rss.art19.com/episodes/b6a6.mp3"),
            "https://rss.art19.com/episodes/b6a6.mp3"
        );
        assert_eq!(
            strip_trackers("https://chrt.fm/track/ABC123/traffic.libsyn.com/show/ep1.mp3?dest-id=9"),
            "https://traffic.libsyn.com/show/ep1.mp3?dest-id=9"
        );
        assert_eq!(
            strip_trackers("https://op3.dev/e/https://example.com/a.mp3"),
            "https://example.com/a.mp3"
        );
        assert_eq!(
            strip_trackers("https://www.podtrac.com/pts/redirect.mp3/pdst.fm/e/chtbl.com/track/X/cdn.example.org/ep.m4a"),
            "https://cdn.example.org/ep.m4a"
        );
        let plain = "https://aphid.fireside.fm/d/1437767933/c91b/280e.mp3";
        assert_eq!(strip_trackers(plain), plain);
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range(Some("bytes=0-")), (0, None));
        assert_eq!(parse_range(Some("bytes=100-199")), (100, Some(199)));
        assert_eq!(parse_range(Some("bytes=5-1")), (5, None));
        assert_eq!(parse_range(None), (0, None));
        assert_eq!(media_type("application/octet-stream", "https://x/a.m4a?x=1"), "audio/mp4");
    }
}
