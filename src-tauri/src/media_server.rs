//! Loopback HTTP server for podcast audio, used on Linux and the BSDs.
//!
//! WebKitGTK plays media through GStreamer, and GStreamer cannot read the app's
//! `tidings://` scheme (it only has sources for http, file, blob and similar).
//! So there the player loads audio from this tiny server instead. It:
//! - listens only on 127.0.0.1, on a random port;
//! - answers only URLs that start with a random secret token, so other programs
//!   and web pages on this machine cannot use it;
//! - hands every request to the same media proxy as `tidings://localhost/media/`,
//!   so tracker stripping, the proxy setting and private-address checks still apply.
//!
//! Used wherever Tauri renders with WebKitGTK (`cfg(webkitgtk)`, set in build.rs):
//! Linux, FreeBSD and OpenBSD. macOS plays `tidings://` directly.
#![cfg_attr(not(webkitgtk), allow(dead_code))]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tauri::http::{header, Response, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Turns (`<base64url>` path part, `Range` header) into a response.
pub type Handler = Arc<
    dyn Fn(String, Option<String>) -> Pin<Box<dyn Future<Output = Response<Vec<u8>>> + Send>>
        + Send
        + Sync,
>;

const MAX_HEAD: usize = 16 * 1024;
/// Connections that have not yet sent a request with the token, at most. Further
/// ones are closed at once. Any program on this computer can connect, and without
/// a limit each connection would hold a task for as long as it stays open.
const MAX_WAITING: usize = 32;
/// A connection has this long to send its request.
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// A random 48-character hex token from the operating system's random source.
pub fn random_token() -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Starts the server and returns its base URL, `http://127.0.0.1:PORT/TOKEN`.
/// Media URLs are `BASE/media/<base64url>`.
pub async fn start(handler: Handler) -> std::io::Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = random_token()?;
    let base = format!("http://127.0.0.1:{port}/{token}");
    let prefix = format!("/{token}/media/");
    let waiting = Arc::new(Semaphore::new(MAX_WAITING));
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            // Too many connections that have sent nothing useful: drop this one.
            let Ok(permit) = waiting.clone().try_acquire_owned() else { continue };
            let (handler, prefix) = (handler.clone(), prefix.clone());
            tauri::async_runtime::spawn(async move {
                let _ = serve(stream, &prefix, handler, permit).await;
            });
        }
    });
    Ok(base)
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).unwrap_or_default()
}

/// Handles one request per connection (`Connection: close`), which is enough for a media player.
///
/// `waiting` is this connection's place among those that have not sent a request
/// with the token. It is given back once the token is seen, so the player's own
/// connections, which stay open while an episode plays, do not use the places up.
async fn serve(mut stream: TcpStream, prefix: &str, handler: Handler, waiting: OwnedSemaphorePermit) -> std::io::Result<()> {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    let read_head = async {
        loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Ok(false);
            }
            head.extend_from_slice(&chunk[..n]);
            if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() > MAX_HEAD {
                return Ok::<bool, std::io::Error>(true);
            }
        }
    };
    match tokio::time::timeout(HEAD_TIMEOUT, read_head).await {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) | Err(_) => return Ok(()),
        Ok(Err(e)) => return Err(e),
    }
    if !head.windows(4).any(|w| w == b"\r\n\r\n") {
        return write(&mut stream, empty(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE), false).await;
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("");
    let target = first.next().unwrap_or("");
    let range = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim().to_string());

    let is_head = method == "HEAD";
    if method != "GET" && !is_head {
        return write(&mut stream, empty(StatusCode::METHOD_NOT_ALLOWED), false).await;
    }
    let path = target.split('?').next().unwrap_or("");
    let Some(encoded) = path.strip_prefix(prefix).filter(|e| !e.is_empty()) else {
        // Wrong or missing token: reveal nothing.
        return write(&mut stream, empty(StatusCode::NOT_FOUND), false).await;
    };
    let encoded = encoded.to_string();
    drop(waiting);
    let first = handler(encoded.clone(), range.clone()).await;
    match stream_plan(&first, range.as_deref()) {
        Some(plan) => write_stream(&mut stream, first, plan, &encoded, &handler, is_head).await,
        None => write(&mut stream, first, is_head).await,
    }
}

/// `(start, end, total)` from a `Content-Range: bytes start-end/total` header.
fn content_range(response: &Response<Vec<u8>>) -> Option<(u64, u64, u64)> {
    let value = response.headers().get(header::CONTENT_RANGE)?.to_str().ok()?;
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// One response covering more than the media proxy's first slice.
struct StreamPlan {
    /// Answer `200` with the whole file (the request had no `Range`) instead of `206`.
    whole: bool,
    start: u64,
    /// Last byte already in the first slice.
    first_end: u64,
    /// Last byte to send.
    last: u64,
    total: u64,
}

/// The media proxy answers in slices of a few megabytes, which is fine for a
/// player that asks for the next range itself (WKWebView on macOS does). WebKitGTK's
/// GStreamer source does not: it takes the slice's length as the length of the
/// whole file, so episodes stopped after the first slice. When the request is
/// open-ended (no `Range`, or `bytes=N-`), answer it like a normal web server:
/// one response up to the end of the file, filled slice by slice.
fn stream_plan(first: &Response<Vec<u8>>, range: Option<&str>) -> Option<StreamPlan> {
    if first.status() != StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let (start, first_end, total) = content_range(first)?;
    let (_, wanted_end) = crate::media::parse_range(range);
    let last = wanted_end.unwrap_or(u64::MAX).min(total.checked_sub(1)?);
    let consistent = first_end >= start && first_end - start + 1 == first.body().len() as u64;
    let whole = range.is_none();
    (consistent && first_end < last && (!whole || start == 0)).then_some(StreamPlan { whole, start, first_end, last, total })
}

fn head_text<'a>(
    status: StatusCode,
    headers: impl Iterator<Item = (&'a header::HeaderName, &'a header::HeaderValue)>,
    content_length: u64,
) -> String {
    let mut out = format!("HTTP/1.1 {} {}\r\n", status.as_u16(), status.canonical_reason().unwrap_or(""));
    for (name, value) in headers {
        if name == header::CONNECTION || name == header::CONTENT_LENGTH {
            continue;
        }
        if let Ok(v) = value.to_str() {
            out.push_str(&format!("{}: {}\r\n", name.as_str(), v));
        }
    }
    out.push_str(&format!("Content-Length: {content_length}\r\nConnection: close\r\n\r\n"));
    out
}

async fn write(stream: &mut TcpStream, response: Response<Vec<u8>>, head_only: bool) -> std::io::Result<()> {
    let head = head_text(response.status(), response.headers().iter(), response.body().len() as u64);
    stream.write_all(head.as_bytes()).await?;
    if !head_only {
        stream.write_all(response.body()).await?;
    }
    stream.shutdown().await
}

/// Sends `plan.start..=plan.last` as one response. If the audio host fails part
/// way, the connection closes early; the player then asks again from where it stopped.
async fn write_stream(
    stream: &mut TcpStream,
    first: Response<Vec<u8>>,
    plan: StreamPlan,
    encoded: &str,
    handler: &Handler,
    head_only: bool,
) -> std::io::Result<()> {
    let StreamPlan { whole, start, first_end, last, total } = plan;
    let range_name = header::CONTENT_RANGE;
    let range_value = header::HeaderValue::from_str(&format!("bytes {start}-{last}/{total}"))
        .map_err(std::io::Error::other)?;
    let mut headers: Vec<_> = first.headers().iter().filter(|(n, _)| **n != range_name).collect();
    let status = if whole {
        StatusCode::OK
    } else {
        headers.push((&range_name, &range_value));
        StatusCode::PARTIAL_CONTENT
    };
    let head = head_text(status, headers.into_iter(), last - start + 1);
    stream.write_all(head.as_bytes()).await?;
    if head_only {
        return stream.shutdown().await;
    }
    stream.write_all(first.body()).await?;

    let mut next = first_end + 1;
    while next <= last {
        let slice = handler(encoded.to_string(), Some(format!("bytes={next}-{last}"))).await;
        let in_order = slice.status() == StatusCode::PARTIAL_CONTENT
            && content_range(&slice).is_some_and(|(s, _, _)| s == next)
            && !slice.body().is_empty();
        if !in_order {
            return Err(std::io::Error::other("the audio host stopped sending"));
        }
        let body = slice.body();
        let take = body.len().min((last - next + 1) as usize);
        stream.write_all(&body[..take]).await?;
        next += take as u64;
    }
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100-byte "file" served like the media proxy does: at most 10 bytes per answer.
    const FILE_LEN: usize = 100;
    const SLICE: u64 = 10;

    fn file() -> Vec<u8> {
        (0..FILE_LEN).map(|i| i as u8).collect()
    }

    fn sliced_handler() -> Handler {
        Arc::new(|encoded: String, range: Option<String>| {
            Box::pin(async move {
                assert_eq!(encoded, "abc");
                let (start, end) = crate::media::parse_range(range.as_deref());
                let end = end.unwrap_or(u64::MAX).min(start + SLICE - 1).min(FILE_LEN as u64 - 1);
                Response::builder()
                    .status(206)
                    .header(header::CONTENT_TYPE, "audio/mpeg")
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{FILE_LEN}"))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(file()[start as usize..=end as usize].to_vec())
                    .unwrap()
            })
        })
    }

    #[tokio::test]
    async fn streams_whole_ranges_from_slices() {
        let base = start(sliced_handler()).await.unwrap();
        let url = format!("{base}/media/abc");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // No Range: the whole file as 200, like a normal web server.
        let r = client.get(&url).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers()["content-length"], "100");
        assert!(r.headers().get("content-range").is_none());
        assert_eq!(r.bytes().await.unwrap().to_vec(), file());

        // Open-ended range: everything from there to the end.
        let r = client.get(&url).header("Range", "bytes=35-").send().await.unwrap();
        assert_eq!(r.status().as_u16(), 206);
        assert_eq!(r.headers()["content-type"], "audio/mpeg");
        assert_eq!(r.headers()["content-range"], "bytes 35-99/100");
        assert_eq!(r.headers()["content-length"], "65");
        assert_eq!(r.bytes().await.unwrap().to_vec(), file()[35..]);

        // A closed range across several slices.
        let r = client.get(&url).header("Range", "bytes=5-57").send().await.unwrap();
        assert_eq!(r.headers()["content-range"], "bytes 5-57/100");
        assert_eq!(r.bytes().await.unwrap().to_vec(), file()[5..=57]);

        // A range inside one slice is passed through as is.
        let r = client.get(&url).header("Range", "bytes=20-24").send().await.unwrap();
        assert_eq!(r.headers()["content-range"], "bytes 20-24/100");
        assert_eq!(r.bytes().await.unwrap().to_vec(), file()[20..=24]);

        // HEAD reports the full length without a body.
        let r = client.head(&url).header("Range", "bytes=90-").send().await.unwrap();
        assert_eq!(r.status().as_u16(), 206);
        assert_eq!(r.headers()["content-length"], "10");
        let r = client.head(&url).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers()["content-length"], "100");
    }

    #[tokio::test]
    async fn serves_only_with_the_token() {
        let base = start(sliced_handler()).await.unwrap();
        assert!(base.starts_with("http://127.0.0.1:"));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ok = client.get(format!("{base}/media/abc")).header("Range", "bytes=5-").send().await.unwrap();
        assert_eq!(ok.status().as_u16(), 206);

        // Wrong token, missing token, other paths and other methods are refused.
        let port_root = base.rsplit_once('/').unwrap().0;
        for url in [
            format!("{port_root}/wrongtoken/media/abc"),
            format!("{port_root}/media/abc"),
            format!("{base}/img/abc"),
            format!("{base}/media/"),
        ] {
            let r = client.get(&url).send().await.unwrap();
            assert_eq!(r.status().as_u16(), 404, "{url}");
        }
        let post = client.post(format!("{base}/media/abc")).send().await.unwrap();
        assert_eq!(post.status().as_u16(), 405);
    }

    #[tokio::test]
    async fn silent_connections_are_limited() {
        let base = start(sliced_handler()).await.unwrap();
        let (address, token) = base.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let request = format!("GET /{token}/media/abc HTTP/1.1\r\nRange: bytes=5-\r\n\r\n");
        let ask = || async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let _ = stream.write_all(request.as_bytes()).await;
            let mut answer = Vec::new();
            let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut answer)).await;
            answer
        };
        // Every waiting place is taken by a connection that sends nothing. Then a
        // further connection is closed without an answer, even with the token.
        let mut silent = Vec::new();
        for _ in 0..MAX_WAITING {
            silent.push(TcpStream::connect(address).await.unwrap());
        }
        let mut refused = false;
        for _ in 0..50 {
            if ask().await.is_empty() {
                refused = true;
                break;
            }
            // The server has not accepted all the silent ones yet.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(refused);
        // When the silent ones go away, the player is served again.
        drop(silent);
        let mut served = false;
        for _ in 0..50 {
            if ask().await.starts_with(b"HTTP/1.1 206") {
                served = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(served);
    }

    #[test]
    fn tokens_are_random() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }
}
