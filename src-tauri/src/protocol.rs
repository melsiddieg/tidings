//! The `tidings://` URI scheme.
//!
//! - `tidings://localhost/img/<base64url>` downloads a remote image in Rust and
//!   returns it. The web view never contacts image hosts itself, so no cookies,
//!   no referrer, and the proxy setting applies to images too.
//! - `tidings://localhost/icon/<feed id>` returns the stored favicon of a feed.
//! - `tidings://localhost/media/<base64url>` streams podcast audio in byte ranges.

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::header::ACCEPT;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

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
        feed_icon(&state, rest)
    } else {
        empty(StatusCode::NOT_FOUND)
    }
}

fn feed_icon(state: &AppState, rest: &str) -> Response<Vec<u8>> {
    let Ok(id) = rest.trim_end_matches('/').parse::<i64>() else {
        return empty(StatusCode::BAD_REQUEST);
    };
    match crate::db::icon(&state.db(), id) {
        Ok(Some((bytes, mime))) => image_response(&mime, bytes, 7 * 86_400),
        _ => empty(StatusCode::NOT_FOUND),
    }
}

/// Decodes and checks a proxied URL: http(s) only, and no private addresses
/// unless the user routes traffic through their own proxy.
fn decode_target(state: &AppState, encoded: &str) -> Result<String, StatusCode> {
    let target = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('/'))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let url = url::Url::parse(&target).map_err(|_| StatusCode::BAD_REQUEST)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StatusCode::FORBIDDEN);
    }
    let proxied = !state.settings().proxy.is_empty();
    if !proxied && !net::host_is_allowed_for_images(&url) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(target)
}

async fn proxy_media(state: &AppState, encoded: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let target = match decode_target(state, encoded) {
        Ok(t) => t,
        Err(code) => return empty(code),
    };
    let source = if state.settings().strip_tracking { media::strip_trackers(&target) } else { target };
    match media::fetch(&state.net(), &state.media, &source, range).await {
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
    let target = match decode_target(state, encoded) {
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
        .header(ACCEPT, "image/avif,image/webp,image/png,image/svg+xml,image/*;q=0.8")
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
    if bytes.len() <= 3 * 1024 * 1024 {
        let entry = Arc::new(CachedImage { mime: mime.clone(), bytes: bytes.clone() });
        state.images.lock().unwrap_or_else(|e| e.into_inner()).put(target, entry);
    }
    image_response(&mime, bytes, 86_400)
}
