use serde::{Serialize, Serializer};

/// One error type for the whole backend. It is sent to the UI as a plain string.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{}", describe_http(.0))]
    Http(#[from] reqwest::Error),
    #[error("File error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    #[error("{0}")]
    Msg(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Error::Msg(text.into())
    }

    /// A request refused by the private-network rule in net.rs.
    pub fn blocked() -> Self {
        Error::Msg(BLOCKED_TEXT.into())
    }
}

const BLOCKED_TEXT: &str = "Blocked: this address is on a private or local network. \
    Tidings only connects to such addresses for feeds you add yourself.";

fn describe_http(err: &reqwest::Error) -> String {
    // A request refused by the private-network rule in net.rs. Check this first:
    // reqwest reports it as a connect or redirect error.
    let mut source: Option<&dyn std::error::Error> = Some(err);
    while let Some(inner) = source {
        if inner.to_string().contains(crate::net::BLOCKED) {
            return BLOCKED_TEXT.into();
        }
        source = inner.source();
    }
    if err.is_timeout() {
        return "The server took too long to respond".into();
    }
    if err.is_connect() {
        return "Could not connect to the server".into();
    }
    if err.is_redirect() {
        return "Too many redirects".into();
    }
    if let Some(status) = err.status() {
        return format!("Server returned HTTP {status}");
    }
    // Walk the source chain so the user sees the real cause (DNS, TLS, proxy...).
    let mut msg = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(inner) = source {
        msg = format!("{msg}: {inner}");
        source = inner.source();
    }
    msg
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
