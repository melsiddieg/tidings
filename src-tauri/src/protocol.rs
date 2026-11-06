//! The `tidings://` URI scheme.
//!
//! - `tidings://localhost/img/<base64url>` downloads a remote image in Rust and
//!   returns it. The web view never contacts image hosts itself, so no cookies,
//!   no referrer, and the proxy setting applies to images too.
//! - `tidings://localhost/icon/<feed id>` returns the stored favicon of a feed.

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::header::ACCEPT;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

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
    if let Some(rest) = path.strip_prefix("/img/") {
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

async fn proxy_image(state: &AppState, encoded: &str) -> Response<Vec<u8>> {
    let Some(target) = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('/'))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let Ok(url) = url::Url::parse(&target) else { return empty(StatusCode::BAD_REQUEST) };
    if !matches!(url.scheme(), "http" | "https") {
        return empty(StatusCode::FORBIDDEN);
    }
    let proxied = !state.settings().proxy.is_empty();
    if !proxied && !net::host_is_allowed_for_images(&url) {
        return empty(StatusCode::FORBIDDEN);
    }

    if let Some(hit) = state.images.lock().unwrap_or_else(|e| e.into_inner()).get(&target).cloned() {
        return image_response(&hit.mime, hit.bytes.clone(), 86_400);
    }

    let net = state.net();
    let response = match net
        .image
        .get(url.as_str())
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
