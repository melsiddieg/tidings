//! HTTP clients. All network traffic of the app goes through here, so the privacy
//! settings (proxy, user agent, HTTPS-only, no cookies, no referrer) apply everywhere.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder, Proxy, RequestBuilder, Response};

use url::{Host, Url};

use crate::error::{Error, Result};
use crate::settings::Settings;

// Podcast feeds with long archives can exceed 20 MB (BSD Now is about 19 MB).
pub const MAX_FEED_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_ICON_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct Net {
    /// Used for feeds, web pages and favicons on public hosts. It refuses private
    /// and local addresses in every redirect and, when no proxy is set, in DNS
    /// answers, so a feed cannot make the app send requests into your local network.
    pub web: Client,
    /// Used only for a host the user chose (a feed they subscribed to or an address
    /// they typed) when that host is itself on a private network. See `Net::get`.
    pub local: Client,
    /// Used for images, podcast audio and video, through `Net::public`. Same rules as
    /// `web`, with a shorter timeout.
    image: Client,
    /// Tests only: lets `Net::public` reach the test servers on 127.0.0.1.
    #[cfg(test)]
    pub unguarded: bool,
    /// Used only for the AI model server of briefs, which must be on this computer
    /// or the local network (`check_local_model`). It never uses the proxy: the
    /// request does not leave the local network, and Tor could not reach it anyway.
    /// No redirects, so the server cannot send the request anywhere else.
    model: Client,
    /// A proxy is set. The app then makes no DNS lookups of its own, so that with
    /// Tor every lookup goes through Tor.
    proxied: bool,
}

impl Net {
    pub fn new(settings: &Settings) -> Result<Self> {
        let proxied = !settings.proxy.is_empty();
        let web = public_only(base_builder(settings)?, proxied, 8).build()?;
        let image = public_only(base_builder(settings)?.timeout(Duration::from_secs(25)), proxied, 5).build()?;
        // A private host may redirect within itself (http to https, a moved path),
        // and nowhere else. This client does not check DNS answers (the host it is
        // for is private), so a redirect to another name could lead to any other
        // device on the local network.
        let local = base_builder(settings)?
            .redirect(Policy::custom(|attempt| {
                let first = attempt.previous().first().cloned();
                if attempt.previous().len() >= 8 {
                    attempt.error("too many redirects")
                } else if first.is_some_and(|f| same_site(&f, attempt.url())) {
                    attempt.follow()
                } else {
                    attempt.error(BLOCKED)
                }
            }))
            .build()?;
        let model = Client::builder()
            .user_agent(settings.user_agent.clone())
            .no_proxy()
            .redirect(Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(10))
            // A local name such as `gpu-box.lan` must also resolve to a local address:
            // on someone else's network, its DNS could point the name anywhere.
            .dns_resolver(Arc::new(PrivateOnlyResolver))
            .build()?;
        Ok(Net {
            web,
            local,
            image,
            model,
            proxied,
            #[cfg(test)]
            unguarded: false,
        })
    }

    /// Starts a GET request for an image, audio or video file. This is the one place
    /// these requests start, and it refuses literal private addresses and local names
    /// itself: the callers rewrite URLs (tracker removal, cached redirects, addresses
    /// from yt-dlp and PeerTube), and a literal IP address never reaches the DNS
    /// resolver that guards names.
    pub fn public(&self, url: &str) -> Result<RequestBuilder> {
        let parsed = Url::parse(url).map_err(|_| Error::msg("This is not a valid web address"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(Error::msg("Only http:// and https:// addresses are supported"));
        }
        #[cfg(test)]
        if self.unguarded {
            return Ok(self.local.get(parsed.as_str()));
        }
        if !host_looks_public(&parsed) {
            return Err(Error::blocked());
        }
        Ok(self.image.get(parsed.as_str()))
    }

    /// A `Net` that reaches nothing outside this computer, for when the saved proxy
    /// setting cannot be used at startup. Falling back to a direct connection would
    /// send everything around the proxy (Tor) without the user knowing. Every client
    /// gets a proxy address where nothing listens, so requests fail before they leave.
    pub fn offline() -> Result<Self> {
        let settings = Settings { proxy: "http://127.0.0.1:1".into(), ..Settings::default() };
        Net::new(&settings)
    }

    /// Starts a request to the AI model server for briefs. See `check_local_model`.
    pub fn local_model(&self, method: reqwest::Method, url: &Url) -> Result<RequestBuilder> {
        check_local_model(url, self.proxied)?;
        Ok(self.model.request(method, url.as_str()))
    }

    /// Starts a GET request for a feed, page or favicon. `trusted` is an address on
    /// the local network that the user has allowed by name: a feed whose `local`
    /// column is 1, or an address they confirmed in the Add Feed dialog. Pass None
    /// for everything else. Only `url`s on the same host and port as `trusted` use
    /// the client that may reach private addresses; DNS plays no part in that
    /// choice, so a name cannot earn local access by resolving to a private address.
    /// Everything else, such as links inside articles or a site link a feed points
    /// to, must be public.
    pub async fn get(&self, url: &Url, trusted: Option<&Url>) -> Result<RequestBuilder> {
        if trusted.is_some_and(|t| same_site(t, url)) {
            return Ok(self.local.get(url.as_str()));
        }
        // Literal IP addresses never reach the DNS resolver, so check them here.
        if !host_looks_public(url) {
            return Err(Error::blocked());
        }
        Ok(self.web.get(url.as_str()))
    }

    /// True when `url` points to this computer or a private network: a private or
    /// loopback IP, a local name such as `nas` or `printer.local`, or (without a
    /// proxy) a name whose DNS answers are all private.
    pub async fn is_private_host(&self, url: &Url) -> bool {
        self.private_host(url).await.unwrap_or(false)
    }

    /// Like `is_private_host`, but None when the answer is not known now: the name
    /// could not be looked up (offline, for example).
    pub async fn private_host(&self, url: &Url) -> Option<bool> {
        match url.host() {
            Some(Host::Ipv4(ip)) => Some(!is_public_ip(IpAddr::V4(ip))),
            Some(Host::Ipv6(ip)) => Some(!is_public_ip(IpAddr::V6(ip))),
            Some(Host::Domain(name)) => {
                if is_local_name(name) {
                    return Some(true);
                }
                if self.proxied {
                    return Some(false);
                }
                let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name, 0)).await.ok()?.collect();
                if addrs.is_empty() {
                    return None;
                }
                Some(addrs.iter().all(|a| !is_public_ip(a.ip())))
            }
            None => Some(false),
        }
    }
}

/// Text of the error when a request is refused. `error.rs` looks for it.
pub const BLOCKED: &str = "blocked: the address is on a private or local network";

/// Makes a client refuse private and local addresses. Every redirect is checked,
/// because a public host could redirect to a literal private IP, which skips DNS.
/// Without a proxy, DNS answers are checked too. With a proxy, the proxy does the
/// lookups, so only the literal check applies.
fn public_only(builder: ClientBuilder, proxied: bool, max_redirects: usize) -> ClientBuilder {
    let builder = builder.redirect(Policy::custom(move |attempt| {
        if attempt.previous().len() >= max_redirects {
            attempt.error("too many redirects")
        } else if host_looks_public(attempt.url()) {
            attempt.follow()
        } else {
            attempt.error(BLOCKED)
        }
    }));
    if proxied {
        builder
    } else {
        builder.dns_resolver(Arc::new(PublicOnlyResolver))
    }
}

/// Same host and port. The one exception is the usual move between http on port 80
/// and https on port 443. Another port is another service on that machine, which
/// the user did not allow.
pub fn same_site(a: &Url, b: &Url) -> bool {
    let standard = |u: &Url| matches!((u.scheme(), u.port_or_known_default()), ("http", Some(80)) | ("https", Some(443)));
    a.host().is_some()
        && a.host() == b.host()
        && (a.port_or_known_default() == b.port_or_known_default() || (standard(a) && standard(b)))
}

/// An address without its user name and password, for display.
pub fn without_credentials(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(mut url) if !url.username().is_empty() || url.password().is_some() => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.to_string()
        }
        _ => raw.to_string(),
    }
}

/// Start of the error that asks the UI for the user's consent before Tidings
/// connects to a host on the local network. The host follows the colon.
pub const LOCAL_CONSENT: &str = "local-network:";

/// The error that asks for consent to connect to `url`'s host.
pub fn needs_consent(url: &Url) -> Error {
    Error::msg(format!("{LOCAL_CONSENT}{}", url.host_str().unwrap_or("")))
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

/// False for every address that is not an ordinary address on the internet: private,
/// loopback, link-local and the other special-purpose ranges, and IPv6 addresses
/// that carry an IPv4 address inside (they can lead to that IPv4 address).
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
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
                || (o[0] == 192 && o[1] == 88 && o[2] == 99) // 6to4 relays
                || (o[0] == 198 && (o[1] & 0xFE) == 18) // benchmarking
                || o[0] == 0
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let s = v6.segments();
            let embedded = |hi: u16, lo: u16| is_public_ip(IpAddr::V4(std::net::Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8)));
            // NAT64 (64:ff9b::/96) and IPv4-compatible (::/96): the IPv4 address is at the end.
            if (s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0]) || (s[..6] == [0, 0, 0, 0, 0, 0] && (s[6] != 0 || s[7] > 1)) {
                return embedded(s[6], s[7]);
            }
            // 6to4 (2002::/16): the IPv4 address follows the prefix.
            if s[0] == 0x2002 {
                return embedded(s[1], s[2]);
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] & 0xffc0) == 0xfec0 // site local (deprecated)
                || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1) // local-use NAT64
                || (s[0] == 0x2001 && s[1] == 0) // Teredo
                || (s[0] == 0x2001 && s[1] == 0xdb8) // documentation
                || (s[0] == 0x100 && s[1..4] == [0, 0, 0])) // discard only
        }
    }
}

/// Returns false for URLs whose host is a literal private or local IP address
/// or a local name. (Literal IPs skip DNS, so the resolver below never sees them.)
pub fn host_looks_public(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
        Some(Host::Domain(name)) => !is_local_name(name),
        None => false,
    }
}

/// The AI model server must be on this computer or the local network: a loopback or
/// private IP address, `localhost`, or a local name such as `gpu-box.lan`. With a
/// proxy set, only addresses that need no DNS lookup (IPs and `localhost`), so no
/// name is looked up outside the proxy.
pub fn check_local_model(url: &Url, proxied: bool) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::msg("The model server address must start with http:// or https://"));
    }
    if host_looks_public(url) {
        return Err(Error::msg(
            "Tidings only uses AI models on this computer or your local network. \
             Use an address like http://127.0.0.1:11434.",
        ));
    }
    if proxied {
        if let Some(Host::Domain(name)) = url.host() {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            if name != "localhost" && !name.ends_with(".localhost") {
                return Err(Error::msg(
                    "With a proxy set, give the model server as an IP address (like 127.0.0.1), \
                     so its name is not looked up outside the proxy.",
                ));
            }
        }
    }
    Ok(())
}

/// Names that only make sense on this computer or a local network:
/// `localhost`, single-label names such as `nas`, and local-only suffixes.
pub fn is_local_name(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    const SUFFIXES: &[&str] =
        &[".localhost", ".local", ".lan", ".home", ".internal", ".intranet", ".home.arpa"];
    name == "localhost" || !name.contains('.') || SUFFIXES.iter().any(|s| name.ends_with(s))
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
            BLOCKED,
        ));
    }
    Ok(addrs)
}

/// Resolves a host name and keeps only private and loopback addresses.
struct PrivateOnlyResolver;

impl Resolve for PrivateOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.filter(|a| !is_public_ip(a.ip())).collect();
            if addrs.is_empty() {
                let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, NOT_LOCAL);
                return Err(Box::new(err) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Text of the error when the model server's name resolves to a public address.
pub const NOT_LOCAL: &str = "the model server's name does not lead to this computer or the local network";

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
pub fn parse_web_url(input: &str) -> Result<Url> {
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
    let url = Url::parse(&fixed).map_err(|_| Error::msg("This is not a valid web address"))?;
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
