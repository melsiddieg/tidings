//! HTTP clients. All network traffic of the app goes through here, so the privacy
//! settings (proxy, user agent, HTTPS-only, no cookies, no referrer) apply everywhere.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder, Proxy, Response};

use crate::error::{Error, Result};
use crate::settings::Settings;

// Podcast feeds with long archives can exceed 20 MB (BSD Now is about 19 MB).
pub const MAX_FEED_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_ICON_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct Net {
    /// Used for feeds, web pages and favicons.
    pub feed: Client,
    /// Used for images inside articles. When no proxy is set, it refuses to
    /// connect to private or local addresses, so a feed cannot make the app
    /// probe your local network through an <img> tag.
    pub image: Client,
}

impl Net {
    pub fn new(settings: &Settings) -> Result<Self> {
        let feed = base_builder(settings)?.build()?;
        let mut image = base_builder(settings)?.timeout(Duration::from_secs(25));
        if settings.proxy.is_empty() {
            // Check every hop: a public host could redirect to a literal private IP,
            // which would skip the DNS check below.
            image = image
                .dns_resolver(Arc::new(PublicOnlyResolver))
                .redirect(Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5 {
                        attempt.error("too many redirects")
                    } else if host_is_allowed_for_images(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.error("blocked: redirect to a private address")
                    }
                }));
        }
        Ok(Net { feed, image: image.build()? })
    }
}

fn base_builder(settings: &Settings) -> Result<ClientBuilder> {
    let mut builder = Client::builder()
        .user_agent(settings.user_agent.clone())
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .redirect(Policy::limited(8))
        // Never tell servers which page we came from.
        .referer(false)
        .https_only(settings.https_only)
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .pool_idle_timeout(Duration::from_secs(60));
    if !settings.proxy.is_empty() {
        builder = builder.proxy(Proxy::all(&settings.proxy)?);
    }
    Ok(builder)
}

/// Reads a response body but stops once it grows past `max` bytes.
pub async fn read_limited(mut resp: Response, max: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|len| len as usize > max) {
        return Err(Error::msg("The response is too large"));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > max {
            return Err(Error::msg("The response is too large"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

pub fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Decodes HTML bytes using the charset from the header, a <meta> tag, or UTF-8.
pub fn decode_html(bytes: &[u8], content_type: &str) -> String {
    let from_header = content_type
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("charset="))
        .next()
        .map(|c| c.trim_matches('"').to_string());
    let from_meta = || {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
        let idx = head.find("charset=")?;
        let rest = &head[idx + 8..];
        let rest = rest.trim_start_matches(['"', '\'']);
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))?;
        Some(rest[..end].to_string())
    };
    let label = from_header.or_else(from_meta).unwrap_or_else(|| "utf-8".into());
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // carrier-grade NAT
                || o[0] == 0
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80) // link local
        }
    }
}

/// Returns false for URLs whose host is a literal private or local IP address.
/// (Literal IPs skip DNS, so the resolver below never sees them.)
pub fn host_is_allowed_for_images(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            !(d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local"))
        }
        None => false,
    }
}

/// Resolves a host name and keeps only public addresses.
pub async fn resolve_public(host: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, 0))
        .await?
        .filter(|a| is_public_ip(a.ip()))
        .collect();
    if addrs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "blocked: image host resolves to a private address",
        ));
    }
    Ok(addrs)
}

struct PublicOnlyResolver;

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs = resolve_public(&host)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Accepts only absolute http(s) URLs. `feed://` is treated as https.
pub fn parse_web_url(input: &str) -> Result<url::Url> {
    let trimmed = input.trim();
    let fixed = if let Some(rest) = trimmed.strip_prefix("feed://") {
        format!("https://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("feed:") {
        rest.to_string()
    } else if !trimmed.contains("://") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };
    let url = url::Url::parse(&fixed).map_err(|_| Error::msg("This is not a valid web address"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::msg("Only http:// and https:// addresses are supported"));
    }
    Ok(url)
}

/// Detects the image type from the first bytes. Returns None for anything that
/// is not an image, so an HTML error page can never be served as an image.
pub fn sniff_image(bytes: &[u8], content_type: &str) -> Option<String> {
    let starts = |sig: &[u8]| bytes.len() >= sig.len() && &bytes[..sig.len()] == sig;
    let mime = if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(b"\xFF\xD8\xFF") {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis") {
        "image/avif"
    } else if starts(b"\x00\x00\x01\x00") || starts(b"\x00\x00\x02\x00") {
        "image/x-icon"
    } else if starts(b"BM") {
        "image/bmp"
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
        let head = head.trim_start_matches('\u{feff}').trim_start();
        if (content_type.contains("svg") || head.starts_with("<?xml") || head.starts_with("<svg"))
            && head.contains("<svg")
        {
            // SVG inside <img> cannot run scripts, so it is safe to display.
            "image/svg+xml"
        } else {
            return None;
        }
    };
    Some(mime.to_string())
}
