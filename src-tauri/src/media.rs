//! Audio and video streaming for the podcast player and in-app videos.
//!
//! The web view asks for `tidings://localhost/media/<base64url>` with a `Range`
//! header. Rust fetches that byte range from the audio host and returns it, so
//! the player can seek without downloading the whole file, and the host never
//! talks to the web view (no cookies, no referrer, proxy setting applies).
//!
//! Only single files are played. A playlist (HLS, DASH and the like) is refused:
//! the player would fetch the addresses in it by itself. See `is_playlist`.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lru::LruCache;
use tokio::sync::OnceCell;

use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use url::Url;

use crate::error::{Error, Result};
use crate::net::{self, Net};

/// Largest slice returned for one request, and the size of one cached block. Players
/// ask for the next slice as they go.
pub const CHUNK: u64 = 2 * 1024 * 1024;
/// Blocks fetched in the background after the one just played (6 MB), so the
/// player's next requests are answered from memory instead of waiting on the host.
const READ_AHEAD: u64 = 3;
/// Blocks kept in memory, for all streams together: at most 48 MB.
const CACHED_BLOCKS: usize = 24;
/// Upper bound when a server ignores ranges and we must read from the start.
const MAX_SKIP: u64 = 400 * 1024 * 1024;
/// No file is this large (a million gigabytes). A `Range` past it is refused, so
/// that sums of offsets cannot overflow.
const MAX_OFFSET: u64 = 1 << 50;
/// Files whose start was checked for a playlist are remembered, up to this many.
const CHECKED_FILES: usize = 64;
/// The error for an address that answers with a playlist instead of a file.
const PLAYLIST: &str = "This is a streaming playlist (HLS or DASH), not an audio or video file. \
    Tidings plays single files only";

/// Final URLs after redirects, so each slice does not repeat the redirect chain.
#[derive(Default)]
pub struct ResolvedUrls(Mutex<HashMap<String, (String, Instant)>>);

type Cell = Arc<OnceCell<Arc<Chunk>>>;

/// Shared state of the media proxy: final URLs after redirects, and recent blocks of
/// the files being played. Block `i` holds bytes `i * CHUNK ..= (i + 1) * CHUNK - 1`.
pub struct Media {
    resolved: ResolvedUrls,
    blocks: Mutex<LruCache<(String, u64), Cell>>,
    /// Files whose first block was fetched and is not a playlist.
    checked: Mutex<LruCache<String, ()>>,
}

impl Default for Media {
    fn default() -> Self {
        Media {
            resolved: ResolvedUrls::default(),
            blocks: Mutex::new(LruCache::new(NonZeroUsize::new(CACHED_BLOCKS).unwrap())),
            checked: Mutex::new(LruCache::new(NonZeroUsize::new(CHECKED_FILES).unwrap())),
        }
    }
}

impl Media {
    fn cell(&self, source: &str, index: u64) -> Cell {
        let mut blocks = self.blocks.lock().unwrap_or_else(|e| e.into_inner());
        let key = (source.to_string(), index);
        if let Some(cell) = blocks.get(&key) {
            return cell.clone();
        }
        let cell = Cell::default();
        blocks.put(key, cell.clone());
        cell
    }

    fn has(&self, source: &str, index: u64) -> bool {
        self.blocks.lock().unwrap_or_else(|e| e.into_inner()).contains(&(source.to_string(), index))
    }

    fn is_checked(&self, source: &str) -> bool {
        self.checked.lock().unwrap_or_else(|e| e.into_inner()).get(source).is_some()
    }

    fn set_checked(&self, source: &str) {
        self.checked.lock().unwrap_or_else(|e| e.into_inner()).put(source.to_string(), ());
    }
}

/// Whether `err` says that the address holds a playlist (see `is_playlist`).
pub fn is_playlist_error(err: &Error) -> bool {
    matches!(err, Error::Msg(text) if text == PLAYLIST)
}

/// Whether a file starts like a playlist or a manifest instead of audio or video
/// data: HLS (`#EXTM3U`), DASH and Smooth Streaming (XML), SDP, PLS, ASX and so on.
///
/// A playlist is a list of addresses that the player then fetches by itself.
/// GStreamer and AVFoundation make those requests directly: not through net.rs,
/// not through the proxy setting, and without the private-address checks. A feed
/// could so reach `http://192.168.1.1/...`, or make the player connect without
/// Tor. So a playlist must never reach the player.
///
/// All these formats are text, and no audio or video format starts with text
/// (MP3, MP4, Ogg, WebM, FLAC, WAV and AAC all have bytes below 0x20 or invalid
/// UTF-8 within their first bytes). So the rule is: a file that starts with text
/// is refused.
fn is_playlist(head: &[u8]) -> bool {
    let head = &head[..head.len().min(1024)];
    // Text in UTF-16, with or without a byte order mark: `<` or `#EXT` first.
    let wide = head.strip_prefix(&[0xFF, 0xFE]).or_else(|| head.strip_prefix(&[0xFE, 0xFF])).unwrap_or(head);
    for first in [0, 1] {
        // `first` is where the ASCII byte of each two-byte character sits.
        let ascii: Vec<u8> = wide
            .chunks_exact(2)
            .take(4)
            .take_while(|pair| pair[1 - first] == 0 && pair[first].is_ascii_graphic())
            .map(|pair| pair[first])
            .collect();
        if ascii.len() == 4 && (ascii[0] == b'<' || ascii.eq_ignore_ascii_case(b"#EXT")) {
            return true;
        }
    }
    let text = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head).trim_ascii_start();
    // A playlist followed by other bytes is still a playlist to a player.
    let starts = |marker: &[u8]| text.len() >= marker.len() && text[..marker.len()].eq_ignore_ascii_case(marker);
    if starts(b"<") || starts(b"#EXTM3U") || starts(b"[playlist]") || starts(b"v=0\n") || starts(b"v=0\r") {
        return true;
    }
    // Any other text: a plain list of addresses, for example.
    let control = |b: &u8| *b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C);
    if text.is_empty() || text.iter().any(control) {
        return false;
    }
    match std::str::from_utf8(text) {
        Ok(_) => true,
        // Cut in the middle of a character at the end of `head`.
        Err(e) => e.error_len().is_none() && e.valid_up_to() > 0,
    }
}

/// Content types of playlists. The player may go by the type alone.
fn is_playlist_type(content_type: &str) -> bool {
    let t = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    ["mpegurl", "dash+xml", "vnd.ms-sstr+xml", "application/sdp", "x-scpls", "xspf+xml", "smil", "x-ms-asx", "x-ms-wax", "x-ms-wvx"]
        .iter()
        .any(|playlist| t.contains(playlist))
}

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
            // Only a public host may come out: the address behind the prefix is the
            // feed's own text, and `http://127.0.0.1/...` there must not be used.
            Ok(u) if u.host_str().is_some_and(|h| h.contains('.')) && net::host_looks_public(&u) => {
                current = u.to_string()
            }
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
    // .mp4 and the like may hold a picture (PeerTube serves its files as octet-stream).
    // An <audio> element plays video/mp4 too, so podcasts in .mp4 still work.
    for (ext, mime) in [(".m4a", "audio/mp4"), (".mp4", "video/mp4"), (".m4v", "video/mp4"), (".webm", "video/webm"), (".mov", "video/quicktime"), (".aac", "audio/aac"), (".ogg", "audio/ogg"), (".oga", "audio/ogg"), (".opus", "audio/ogg"), (".flac", "audio/flac"), (".wav", "audio/wav")] {
        if path.ends_with(ext) {
            return mime.to_string();
        }
    }
    "audio/mpeg".to_string()
}

/// Answers the player's `Range` request for an audio or video file, at most `CHUNK`
/// bytes, from cached blocks. Blocks not in memory are fetched from the host, and the
/// next `READ_AHEAD` blocks are fetched in the background.
pub async fn fetch(net: &Net, media: &Arc<Media>, source: &str, range: Option<&str>) -> Result<Chunk> {
    let (start, end) = parse_range(range);
    if start > MAX_OFFSET {
        return Ok(Chunk { status: 416, mime: "audio/mpeg".into(), bytes: Vec::new(), content_range: None });
    }
    let wanted = end.unwrap_or(u64::MAX).min(start + CHUNK - 1) - start + 1;
    let first = start / CHUNK;
    // The start of a file is checked before any part of it is played, also when
    // the player begins in the middle (see `is_playlist`).
    if first != 0 && !media.is_checked(source) {
        block(net, media, source, 0).await?;
    }
    let head = block(net, media, source, first).await?;
    let total = total_size(&head);
    let offset = (start - first * CHUNK) as usize;
    if head.status == 416 || offset >= head.bytes.len() {
        return Ok(Chunk { status: 416, mime: head.mime.clone(), bytes: Vec::new(), content_range: None });
    }
    let mut bytes = head.bytes[offset..].to_vec();
    let mut last = first;
    // A request that starts late in a block continues into the next one, so the
    // answer is not a few bytes long. A short block is the end of the file.
    let more = total.is_none_or(|t| (first + 1) * CHUNK < t);
    if (bytes.len() as u64) < wanted && head.bytes.len() as u64 == CHUNK && more {
        if let Ok(next) = block(net, media, source, first + 1).await {
            if next.status != 416 {
                bytes.extend_from_slice(&next.bytes);
                last = first + 1;
            }
        }
    }
    bytes.truncate(wanted.min(bytes.len() as u64) as usize);
    read_ahead(net, media, source, last, total);
    let real_end = start + bytes.len() as u64 - 1;
    let total = total.map(|t| t.to_string()).unwrap_or_else(|| "*".into());
    Ok(Chunk {
        status: 206,
        mime: head.mime.clone(),
        bytes,
        content_range: Some(format!("bytes {start}-{real_end}/{total}")),
    })
}

/// One block, from memory or from the host. Callers that ask for the same block at
/// the same time share one download. A failed download is not kept, so the next
/// request tries again.
async fn block(net: &Net, media: &Media, source: &str, index: u64) -> Result<Arc<Chunk>> {
    let cell = media.cell(source, index);
    let block = cell
        .get_or_try_init(|| async {
            let start = index * CHUNK;
            let end = start + CHUNK - 1;
            let mut chunk = fetch_range(net, &media.resolved, source, start, end).await?;
            // Some hosts send less than asked for. Fill the block, so blocks stay
            // back to back and a short block always means the end of the file.
            while chunk.status == 206 && (chunk.bytes.len() as u64) < CHUNK {
                let have = start + chunk.bytes.len() as u64;
                let Some(total) = total_size(&chunk).filter(|t| have < *t) else { break };
                let more = fetch_range(net, &media.resolved, source, have, end).await?;
                if more.status != 206 || more.bytes.is_empty() {
                    break;
                }
                chunk.bytes.extend_from_slice(&more.bytes);
                chunk.content_range = Some(format!("bytes {start}-{}/{total}", start + chunk.bytes.len() as u64 - 1));
            }
            // A playlist is not kept, so no block 0 in memory holds one.
            if index == 0 && is_playlist(&chunk.bytes) {
                return Err(Error::msg(PLAYLIST));
            }
            Ok::<_, Error>(Arc::new(chunk))
        })
        .await?;
    if index == 0 {
        media.set_checked(source);
    }
    Ok(block.clone())
}

fn read_ahead(net: &Net, media: &Arc<Media>, source: &str, after: u64, total: Option<u64>) {
    for index in after + 1..=after + READ_AHEAD {
        if total.is_some_and(|t| index * CHUNK >= t) {
            break;
        }
        if media.has(source, index) {
            continue;
        }
        let (net, media, source) = (net.clone(), media.clone(), source.to_string());
        tokio::spawn(async move {
            let _ = block(&net, &media, &source, index).await;
        });
    }
}

/// The file size from a block's `Content-Range` ("bytes 0-99/1234" → 1234).
fn total_size(chunk: &Chunk) -> Option<u64> {
    chunk.content_range.as_deref()?.rsplit_once('/')?.1.parse().ok()
}

/// Fetches bytes `start..=end` of a file from its host (fewer at the end of the file).
async fn fetch_range(net: &Net, cache: &ResolvedUrls, source: &str, start: u64, end: u64) -> Result<Chunk> {

    let mut retried = false;
    loop {
        let url = cache.get(source).unwrap_or_else(|| source.to_string());
        // `Net::public` refuses private addresses, also for a redirect target kept
        // from an earlier request.
        let response = net
            .public(&url)?
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
            return Err(Error::msg(format!("The media host returned HTTP {}", status.as_u16())));
        }
        cache.put(source, response.url().as_str());
        let content_type = net::content_type(response.headers());
        if is_playlist_type(&content_type) {
            return Err(Error::msg(PLAYLIST));
        }
        let mime = media_type(&content_type, response.url().as_str());

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
            return Err(Error::msg("This media host does not support seeking this far"));
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
    fn playlists_are_told_from_media() {
        // Playlists and manifests, in the forms a player accepts.
        let hls = b"#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nhttp://127.0.0.1:8001/segment.ts\n";
        assert!(is_playlist(hls));
        assert!(is_playlist(b"\xEF\xBB\xBF\r\n  #extm3u\nhttp://192.168.1.1/a.ts"));
        assert!(is_playlist(b"<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\"></MPD>"));
        assert!(is_playlist(b"<MPD><BaseURL>http://10.0.0.1/</BaseURL></MPD>"));
        assert!(is_playlist(b"<SmoothStreamingMedia MajorVersion=\"2\">"));
        assert!(is_playlist(b"<asx version=\"3.0\"><entry><ref href=\"http://10.0.0.1/\"/></entry></asx>"));
        assert!(is_playlist(b"[playlist]\nFile1=http://10.0.0.1/a.mp3\n"));
        assert!(is_playlist(b"v=0\r\no=- 0 0 IN IP4 10.0.0.1\r\n"));
        assert!(is_playlist(b"http://10.0.0.1/a.mp3\nhttp://10.0.0.1/b.mp3\n"));
        // Bytes after the playlist do not hide it.
        let mut padded = hls.to_vec();
        padded.extend_from_slice(&[0u8; 64]);
        assert!(is_playlist(&padded));
        // UTF-16, with and without a byte order mark.
        let utf16 = |text: &str, big: bool| -> Vec<u8> {
            text.encode_utf16().flat_map(|u| if big { u.to_be_bytes() } else { u.to_le_bytes() }).collect()
        };
        assert!(is_playlist(&utf16("#EXTM3U\nhttp://10.0.0.1/a.ts", false)));
        assert!(is_playlist(&utf16("<MPD></MPD>", true)));
        assert!(is_playlist(&[&[0xFF, 0xFE][..], &utf16("<?xml version='1.0'?><MPD/>", false)].concat()));
        // Long text that ends inside a character at the 1024-byte limit.
        assert!(is_playlist(format!("a{}", "é".repeat(600)).as_bytes()));

        // The starts of real audio and video files.
        let media: [&[u8]; 12] = [
            b"ID3\x04\x00\x00\x00\x00\x02\x01TIT2\x00\x00\x00\x0c",
            b"\xFF\xFB\x90\x64\x00\x00\x00\x00",
            b"\xFF\xF1\x50\x80\x02\x1F\xFC",
            b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2mp41",
            // An MP4 whose first box is 60 bytes long: its size is the byte `<`.
            b"\x00\x00\x00\x3Cftypmp42\x00\x00\x00\x00mp42isom",
            b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00",
            b"fLaC\x00\x00\x00\x22\x10\x00\x10\x00",
            b"RIFF$   WAVEfmt \x10\x00\x00\x00\x01\x00\x02\x00",
            b"\x1A\x45\xDF\xA3\x9F\x42\x86\x81\x01",
            b"\x47\x40\x00\x10\x00\x00\xB0\x0D",
            b"\x00\x00\x00\x00\x00\x00\xFF\xFB\x90\x64",
            b"",
        ];
        for head in media {
            assert!(!is_playlist(head), "{head:?}");
        }

        for playlist in ["application/vnd.apple.mpegurl", "application/x-mpegURL; charset=utf-8", "audio/mpegurl", "application/dash+xml", "application/sdp"] {
            assert!(is_playlist_type(playlist), "{playlist}");
        }
        for media in ["audio/mpeg", "video/mp4", "application/octet-stream", "text/plain", ""] {
            assert!(!is_playlist_type(media), "{media}");
        }
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range(Some("bytes=0-")), (0, None));
        assert_eq!(parse_range(Some("bytes=100-199")), (100, Some(199)));
        assert_eq!(parse_range(Some("bytes=5-1")), (5, None));
        assert_eq!(parse_range(None), (0, None));
        assert_eq!(media_type("application/octet-stream", "https://x/a.m4a?x=1"), "audio/mp4");
        assert_eq!(media_type("application/octet-stream", "https://x/v-720-fragmented.mp4"), "video/mp4");
        assert_eq!(media_type("video/webm; codecs=vp9", "https://x/v"), "video/webm");
    }
}
