//! PeerTube videos: playing them inside Tidings, and finding a copy of a video on
//! PeerTube with Sepia Search.
//!
//! A PeerTube instance lists plain video files for each public video. Tidings asks the
//! instance's API for them, picks one, and the web view plays it through the media
//! proxy (`tidings://localhost/media/...`), like podcast audio. No PeerTube JavaScript
//! and no peer-to-peer traffic are involved, and all requests go through net.rs.
//!
//! Search sends the text the user searches for (an article title) to Sepia Search,
//! a PeerTube search engine run by Framasoft. It only runs when the user asks.

use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{Error, Result};
use crate::net::{self, Net};
use crate::sanitize;

pub const SEARCH_API: &str = "https://sepiasearch.org/api/v1/search/videos";

/// Largest picture height played inside Tidings. Higher files only cost bandwidth in a
/// reading pane.
pub const MAX_HEIGHT: u32 = 720;

const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

/// A video file to play, with what the UI shows about it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFile {
    pub title: String,
    /// The file itself; the UI plays it through the media proxy.
    pub url: String,
    /// "720p", "480p", ...
    pub label: String,
    /// Seconds.
    pub duration: u64,
}

/// One Sepia Search result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub title: String,
    /// The watch page on the instance that hosts the video.
    pub url: String,
    pub channel: String,
    pub host: String,
    /// Seconds.
    pub duration: u64,
    /// Thumbnail through the image proxy, or None when images are off.
    pub thumbnail: Option<String>,
    pub published: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiVideo {
    name: String,
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    is_live: bool,
    #[serde(default)]
    files: Vec<ApiFile>,
    #[serde(default)]
    streaming_playlists: Vec<ApiPlaylist>,
}

#[derive(Deserialize)]
struct ApiPlaylist {
    #[serde(default)]
    files: Vec<ApiFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiFile {
    resolution: ApiResolution,
    file_url: Option<String>,
}

#[derive(Deserialize)]
struct ApiResolution {
    id: u32,
    label: String,
}

#[derive(Deserialize)]
struct ApiSearch {
    #[serde(default)]
    data: Vec<ApiHit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiHit {
    name: String,
    url: String,
    #[serde(default)]
    duration: u64,
    thumbnail_url: Option<String>,
    published_at: Option<String>,
    channel: Option<ApiChannel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiChannel {
    display_name: Option<String>,
}

/// The video id in a PeerTube watch URL: `/w/<short id>` or `/videos/watch/<uuid>`.
pub fn video_id(url: &Url) -> Option<String> {
    let path = url.path().trim_end_matches('/');
    let id = path.strip_prefix("/w/").or_else(|| path.strip_prefix("/videos/watch/"))?;
    let valid = (20..=40).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| id.to_string())
}

/// Asks the instance in `page` (a watch URL) for the video's files and picks one.
pub async fn video(net: &Net, page: &Url) -> Result<VideoFile> {
    let id = video_id(page).ok_or_else(|| Error::msg("This is not a PeerTube video link"))?;
    let host = page.host_str().unwrap_or("").to_string();
    let mut api = page.clone();
    api.set_path(&format!("/api/v1/videos/{id}"));
    api.set_query(None);
    api.set_fragment(None);
    let response = net.get(&api, None).await?.header(ACCEPT, "application/json").send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "{host} did not return this video (HTTP {}). It may be private or removed, or the site is not PeerTube.",
            response.status().as_u16()
        )));
    }
    let bytes = net::read_limited(response, MAX_JSON_BYTES).await?;
    pick(&bytes)
}

/// Picks the file to play from the instance's answer: the tallest picture up to
/// `MAX_HEIGHT`, else the smallest one above it. Plain files ("web videos") come before
/// the HLS copies of the same height; an HLS copy is one fragmented MP4 file, which
/// also plays in a `<video>` element.
fn pick(json: &[u8]) -> Result<VideoFile> {
    let video: ApiVideo =
        serde_json::from_slice(json).map_err(|_| Error::msg("This site did not answer like a PeerTube site"))?;
    if video.is_live {
        return Err(Error::msg("This is a live stream, which only plays in your video player"));
    }
    let files: Vec<(u32, &ApiFile, String)> = video
        .files
        .iter()
        .chain(video.streaming_playlists.iter().flat_map(|p| p.files.iter()))
        .filter_map(|f| {
            let url = net::parse_web_url(f.file_url.as_deref()?).ok()?;
            Some((f.resolution.id, f, url.to_string()))
        })
        .collect();
    // Height 0 is "Audio only": use it only when nothing else exists.
    let pictures = files.iter().filter(|(h, ..)| *h > 0);
    let chosen = pictures
        .clone()
        .filter(|(h, ..)| *h <= MAX_HEIGHT)
        .fold(None, |best: Option<&(u32, &ApiFile, String)>, f| match best {
            Some(b) if b.0 >= f.0 => Some(b),
            _ => Some(f),
        })
        .or_else(|| pictures.min_by_key(|(h, ..)| *h))
        .or_else(|| files.first());
    let (_, file, url) = chosen.ok_or_else(|| Error::msg("This video has no file that Tidings can play"))?;
    Ok(VideoFile {
        title: video.name.chars().take(300).collect(),
        url: url.clone(),
        label: file.resolution.label.chars().take(20).collect(),
        duration: video.duration,
    })
}

/// Searches Sepia Search for PeerTube videos matching `query`.
pub async fn search(net: &Net, query: &str, load_images: bool) -> Result<Vec<SearchHit>> {
    let query: String = query.trim().chars().take(200).collect();
    if query.is_empty() {
        return Err(Error::msg("Nothing to search for"));
    }
    let mut url = Url::parse(SEARCH_API).map_err(|e| Error::msg(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("search", &query)
        .append_pair("start", "0")
        .append_pair("count", "10")
        .append_pair("sort", "-match")
        .append_pair("nsfw", "false")
        .append_pair("isLive", "false");
    let response = net.get(&url, None).await?.header(ACCEPT, "application/json").send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("Sepia Search returned HTTP {}", response.status().as_u16())));
    }
    let bytes = net::read_limited(response, MAX_JSON_BYTES).await?;
    hits(&bytes, load_images)
}

/// Turns a Sepia Search answer into results. Results without a usable watch URL are dropped.
fn hits(json: &[u8], load_images: bool) -> Result<Vec<SearchHit>> {
    let result: ApiSearch =
        serde_json::from_slice(json).map_err(|_| Error::msg("Sepia Search sent an answer Tidings cannot read"))?;
    Ok(result
        .data
        .into_iter()
        .filter_map(|v| {
            let page = net::parse_web_url(&v.url).ok()?;
            video_id(&page)?;
            let thumbnail = v
                .thumbnail_url
                .filter(|_| load_images)
                .and_then(|t| net::parse_web_url(&t).ok())
                .map(|t| sanitize::proxy_image_url(t.as_str()));
            Some(SearchHit {
                title: v.name.chars().take(300).collect(),
                host: page.host_str().unwrap_or("").to_string(),
                url: page.to_string(),
                channel: v.channel.and_then(|c| c.display_name).unwrap_or_default().chars().take(100).collect(),
                duration: v.duration,
                thumbnail,
                published: v.published_at,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_ids() {
        let id = |s: &str| video_id(&Url::parse(s).unwrap());
        assert_eq!(id("https://framatube.org/w/fXM3xjC2b8k3opZVUK68aN").as_deref(), Some("fXM3xjC2b8k3opZVUK68aN"));
        assert_eq!(
            id("https://tube.example/videos/watch/79299b99-b000-4029-94fb-c7315f6b3bec/").as_deref(),
            Some("79299b99-b000-4029-94fb-c7315f6b3bec")
        );
        assert_eq!(id("https://framatube.org/w/p/fXM3xjC2b8k3opZVUK68aN"), None);
        assert_eq!(id("https://example.com/w/index.php"), None);
        assert_eq!(id("https://example.com/w/short"), None);
    }

    fn file(height: u32, url: &str) -> String {
        format!(r#"{{"resolution":{{"id":{height},"label":"{height}p"}},"fileUrl":"{url}"}}"#)
    }

    #[test]
    fn picks_a_file() {
        let json = format!(
            r#"{{"name":"Talk","duration":286,"isLive":false,
                "files":[{},{},{},{}],
                "streamingPlaylists":[{{"files":[{},{}]}}]}}"#,
            file(1080, "https://cdn.example/1080.mp4"),
            file(480, "https://cdn.example/480.mp4"),
            file(360, "https://cdn.example/360.mp4"),
            file(0, "https://cdn.example/audio.mp4"),
            file(720, "https://cdn.example/hls-720-fragmented.mp4"),
            file(480, "https://cdn.example/hls-480-fragmented.mp4"),
        );
        let f = pick(json.as_bytes()).unwrap();
        assert_eq!((f.url.as_str(), f.label.as_str(), f.duration), ("https://cdn.example/hls-720-fragmented.mp4", "720p", 286));

        // The plain file wins over the HLS copy of the same height.
        let json = format!(
            r#"{{"name":"T","files":[{}],"streamingPlaylists":[{{"files":[{}]}}]}}"#,
            file(480, "https://cdn.example/480.mp4"),
            file(480, "https://cdn.example/hls-480.mp4"),
        );
        assert_eq!(pick(json.as_bytes()).unwrap().url, "https://cdn.example/480.mp4");

        // Only taller files: the smallest. Only audio: the audio.
        let json = format!(r#"{{"name":"T","files":[{},{}]}}"#, file(2160, "https://c.example/4k.mp4"), file(1080, "https://c.example/hd.mp4"));
        assert_eq!(pick(json.as_bytes()).unwrap().url, "https://c.example/hd.mp4");
        let json = format!(r#"{{"name":"T","files":[{}]}}"#, file(0, "https://c.example/a.mp4"));
        assert_eq!(pick(json.as_bytes()).unwrap().url, "https://c.example/a.mp4");

        // Unusable links are skipped; live streams and empty lists are errors.
        let json = format!(r#"{{"name":"T","files":[{}]}}"#, file(480, "javascript:alert(1)"));
        assert!(pick(json.as_bytes()).is_err());
        assert!(pick(br#"{"name":"T","isLive":true}"#).is_err());
        assert!(pick(b"<html>").is_err());
    }

    #[test]
    fn reads_search_results() {
        let json = br#"{"total":2,"data":[
            {"name":"Me at the zoo","url":"https://tube.example/videos/watch/f9680120-e669-414b-a8d2-a5fb8be431df",
             "duration":19,"thumbnailUrl":"https://tube.example/thumb.jpg","publishedAt":"2025-04-25T17:56:36.584Z",
             "channel":{"displayName":"Zoo","host":"tube.example"}},
            {"name":"Not a watch page","url":"https://tube.example/a/b","duration":5},
            {"name":"Bad scheme","url":"ftp://tube.example/videos/watch/f9680120-e669-414b-a8d2-a5fb8be431df"}
        ]}"#;
        let found = hits(json, true).unwrap();
        assert_eq!(found.len(), 1);
        let h = &found[0];
        assert_eq!((h.title.as_str(), h.host.as_str(), h.channel.as_str(), h.duration), ("Me at the zoo", "tube.example", "Zoo", 19));
        assert!(h.thumbnail.as_deref().unwrap().starts_with("tidings://localhost/img/"));
        assert_eq!(hits(json, false).unwrap()[0].thumbnail, None);
        assert!(hits(b"oops", true).is_err());
    }
}
