//! Loopback HTTP server for podcast audio, used on Linux.
//!
//! WebKitGTK plays media through GStreamer, and GStreamer cannot read the app's
//! `tidings://` scheme (it only has sources for http, file, blob and similar).
//! So on Linux the player loads audio from this tiny server instead. It:
//! - listens only on 127.0.0.1, on a random port;
//! - answers only URLs that start with a random secret token, so other programs
//!   and web pages on this machine cannot use it;
//! - hands every request to the same media proxy as `tidings://localhost/media/`,
//!   so tracker stripping, the proxy setting and private-address checks still apply.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tauri::http::{header, Response, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Turns (`<base64url>` path part, `Range` header) into a response.
pub type Handler = Arc<
    dyn Fn(String, Option<String>) -> Pin<Box<dyn Future<Output = Response<Vec<u8>>> + Send>>
        + Send
        + Sync,
>;

const MAX_HEAD: usize = 16 * 1024;

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
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let (handler, prefix) = (handler.clone(), prefix.clone());
            tauri::async_runtime::spawn(async move {
                let _ = serve(stream, &prefix, handler).await;
            });
        }
    });
    Ok(base)
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).unwrap_or_default()
}

/// Handles one request per connection (`Connection: close`), which is enough for a media player.
async fn serve(mut stream: TcpStream, prefix: &str, handler: Handler) -> std::io::Result<()> {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&chunk[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head.len() > MAX_HEAD {
            return write(&mut stream, empty(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE), false).await;
        }
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
    let response = match path.strip_prefix(prefix) {
        Some(encoded) if !encoded.is_empty() => handler(encoded.to_string(), range).await,
        // Wrong or missing token: reveal nothing.
        _ => empty(StatusCode::NOT_FOUND),
    };
    write(&mut stream, response, is_head).await
}

async fn write(stream: &mut TcpStream, response: Response<Vec<u8>>, head_only: bool) -> std::io::Result<()> {
    let status = response.status();
    let mut out = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    );
    for (name, value) in response.headers() {
        if name == header::CONNECTION || name == header::CONTENT_LENGTH {
            continue;
        }
        if let Ok(v) = value.to_str() {
            out.push_str(&format!("{}: {}\r\n", name.as_str(), v));
        }
    }
    let body = response.into_body();
    out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    stream.write_all(out.as_bytes()).await?;
    if !head_only {
        stream.write_all(&body).await?;
    }
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_handler() -> Handler {
        Arc::new(|encoded: String, range: Option<String>| {
            Box::pin(async move {
                let body = format!("{encoded}|{}", range.unwrap_or_default()).into_bytes();
                Response::builder()
                    .status(206)
                    .header(header::CONTENT_TYPE, "audio/mpeg")
                    .header(header::CONTENT_RANGE, "bytes 0-9/100")
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(body)
                    .unwrap()
            })
        })
    }

    #[tokio::test]
    async fn serves_only_with_the_token() {
        let base = start(fake_handler()).await.unwrap();
        assert!(base.starts_with("http://127.0.0.1:"));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ok = client.get(format!("{base}/media/abc")).header("Range", "bytes=5-").send().await.unwrap();
        assert_eq!(ok.status().as_u16(), 206);
        assert_eq!(ok.headers()["content-type"], "audio/mpeg");
        assert_eq!(ok.headers()["content-range"], "bytes 0-9/100");
        assert_eq!(ok.text().await.unwrap(), "abc|bytes=5-");

        let head = client.head(format!("{base}/media/abc")).send().await.unwrap();
        assert_eq!(head.status().as_u16(), 206);

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

    #[test]
    fn tokens_are_random() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }
}
