//! The `tidings://` URI scheme.
//!
//! - `tidings://localhost/img/<base64url>` downloads a remote image in Rust and
//!   returns it. The web view never contacts image hosts itself, so no cookies,
//!   no referrer, and the proxy setting applies to images too.
//! - `tidings://localhost/icon/<feed id>` returns the stored favicon of a feed.
//!
//! Both return images re-encoded by images.rs, never the downloaded file.
//! - `tidings://localhost/media/<base64url>` streams podcast audio in byte ranges.

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::header::ACCEPT;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::images::{self, Use};
use crate::media;
use crate::net;
use crate::state::{AppState, CachedImage};

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).unwrap_or_default()
}

fn image_response(mime: &str, bytes: Vec<u8>, max_age: u32) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, format!("max-age={max_age}"))
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'")
        .body(bytes)
        .unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
}

pub async fn handle(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    let state = state.inner().clone();
    let path = request.uri().path().to_string();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if let Some(rest) = path.strip_prefix("/media/") {
        proxy_media(&state, rest, range.as_deref()).await
    } else if let Some(rest) = path.strip_prefix("/img/") {
        proxy_image(&state, rest).await
    } else if let Some(rest) = path.strip_prefix("/icon/") {
        feed_icon(&state, rest).await
    } else {
        empty(StatusCode::NOT_FOUND)
    }
}

async fn feed_icon(state: &AppState, rest: &str) -> Response<Vec<u8>> {
    let Ok(id) = rest.trim_end_matches('/').parse::<i64>() else {
        return empty(StatusCode::BAD_REQUEST);
    };
    // Icons are stored as downloaded, and re-encoded each time they are shown
    // (they are small). The web view keeps them for a week.
    let Ok(Some((bytes, mime))) = crate::db::icon(&state.db(), id) else {
        return empty(StatusCode::NOT_FOUND);
    };
    match images::clean(bytes, &mime, Use::Icon).await {
        Some(clean) => image_response(clean.mime, clean.bytes, 7 * 86_400),
        None => empty(StatusCode::NOT_FOUND),
    }
}

/// Decodes and checks a proxied URL: http(s) only, and no literal private
/// addresses or local names. This holds with a proxy too: a local proxy would
/// otherwise forward the request into your network. (Without a proxy, the image
/// client also checks DNS answers.)
fn decode_target(encoded: &str) -> Result<String, StatusCode> {
    let target = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('/'))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let url = url::Url::parse(&target).map_err(|_| StatusCode::BAD_REQUEST)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StatusCode::FORBIDDEN);
    }
    if !net::host_looks_public(&url) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(target)
}

/// Media for the loopback server on Linux and the BSDs (see media_server.rs); same rules as `tidings://`.
#[cfg_attr(not(webkitgtk), allow(dead_code))]
pub async fn media_response(app: &AppHandle, encoded: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    let state = state.inner().clone();
    proxy_media(&state, encoded, range).await
}

async fn proxy_media(state: &AppState, encoded: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let target = match decode_target(encoded) {
        Ok(t) => t,
        Err(code) => return empty(code),
    };
    let source = if state.settings().strip_tracking { media::strip_trackers(&target) } else { target };
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let result = media::fetch(&state.net(), &state.media, &source, range).await;
    // Development builds log each media request (no URL), to see how the player reads.
    #[cfg(debug_assertions)]
    match &result {
        Ok(c) => eprintln!("media {:?} -> {} {} bytes in {} ms", range, c.status, c.bytes.len(), started.elapsed().as_millis()),
        // Not the error text: reqwest errors contain the URL.
        Err(_) => eprintln!("media {:?} -> error after {} ms", range, started.elapsed().as_millis()),
    }
    match result {
        Ok(chunk) => {
            let mut builder = Response::builder()
                .status(chunk.status)
                .header(header::CONTENT_TYPE, chunk.mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, chunk.bytes.len())
                .header(header::CACHE_CONTROL, "no-store")
                .header("X-Content-Type-Options", "nosniff");
            if let Some(cr) = chunk.content_range {
                builder = builder.header(header::CONTENT_RANGE, cr);
            }
            builder.body(chunk.bytes).unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
        }
        Err(_) => empty(StatusCode::BAD_GATEWAY),
    }
}

async fn proxy_image(state: &AppState, encoded: &str) -> Response<Vec<u8>> {
    let target = match decode_target(encoded) {
        Ok(t) => t,
        Err(code) => return empty(code),
    };

    if let Some(hit) = state.images.lock().unwrap_or_else(|e| e.into_inner()).get(&target).cloned() {
        return image_response(&hit.mime, hit.bytes.clone(), 86_400);
    }

    let net = state.net();
    let response = match net
        .image
        .get(target.as_str())
        // No AVIF: images.rs has no decoder for it, and sites that offer several
        // formats then send one that it can re-encode.
        .header(ACCEPT, "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/*;q=0.5")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return empty(StatusCode::NOT_FOUND),
    };
    let content_type = net::content_type(response.headers());
    let Ok(bytes) = net::read_limited(response, net::MAX_IMAGE_BYTES).await else {
        return empty(StatusCode::PAYLOAD_TOO_LARGE);
    };
    let Some(mime) = net::sniff_image(&bytes, &content_type) else {
        return empty(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    };
    #[cfg(debug_assertions)]
    let (started, size) = (std::time::Instant::now(), bytes.len());
    let clean = images::clean(bytes, &mime, Use::Article).await;
    // Development builds log each re-encoded image (no URL).
    #[cfg(debug_assertions)]
    match &clean {
        Some(c) => eprintln!(
            "img {mime} {size} bytes -> {} {} bytes in {} ms",
            c.mime,
            c.bytes.len(),
            started.elapsed().as_millis()
        ),
        None => eprintln!("img {mime} {size} bytes -> refused"),
    }
    let Some(clean) = clean else {
        return empty(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    };
    if clean.bytes.len() <= 3 * 1024 * 1024 {
        let entry = Arc::new(CachedImage { mime: clean.mime.to_string(), bytes: clean.bytes.clone() });
        state.images.lock().unwrap_or_else(|e| e.into_inner()).put(target, entry);
    }
    image_response(clean.mime, clean.bytes, 86_400)
}
