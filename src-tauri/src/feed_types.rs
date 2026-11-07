//! Feed types: podcasts, videos, code & releases, discussions, newsletters and
//! articles. Detected locally from the feed's address and the enclosures of its
//! recent items (nothing is looked up online). The user can override it per feed.

use url::Url;

pub const PODCAST: &str = "podcast";
pub const VIDEO: &str = "video";
pub const CODE: &str = "code";
pub const DISCUSSION: &str = "discussion";
pub const NEWSLETTER: &str = "newsletter";
pub const ARTICLE: &str = "article";

pub const TYPES: &[&str] = &[PODCAST, VIDEO, CODE, DISCUSSION, NEWSLETTER, ARTICLE];

/// How many recent items are looked at for enclosures.
pub const RECENT_ITEMS: i64 = 20;

pub fn is_type(value: &str) -> bool {
    TYPES.contains(&value)
}

fn extension(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    path.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default()
}

/// Same rule as the player's `isAudio` in the UI.
pub fn is_audio(mime: Option<&str>, url: Option<&str>) -> bool {
    if mime.is_some_and(|m| m.trim().to_ascii_lowercase().starts_with("audio/")) {
        return true;
    }
    url.is_some_and(|u| matches!(extension(u).as_str(), "mp3" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "flac" | "wav"))
}

pub fn is_video(mime: Option<&str>, url: Option<&str>) -> bool {
    if mime.is_some_and(|m| m.trim().to_ascii_lowercase().starts_with("video/")) {
        return true;
    }
    url.is_some_and(|u| matches!(extension(u).as_str(), "mp4" | "m4v" | "webm" | "mov" | "mkv"))
}

/// Detects a feed's type. `enclosures` are (MIME type, URL) of its recent items,
/// one entry per item (both `None` when an item has no enclosure).
pub fn detect(feed_url: &str, site_url: Option<&str>, enclosures: &[(Option<String>, Option<String>)]) -> &'static str {
    // Media first: a feed where at least half the recent items carry audio is a
    // podcast, wherever it is hosted.
    let items = enclosures.len();
    if items > 0 {
        let count = |test: fn(Option<&str>, Option<&str>) -> bool| {
            enclosures.iter().filter(|(mime, url)| test(mime.as_deref(), url.as_deref())).count()
        };
        let audio = count(is_audio);
        let video = count(is_video);
        if audio > 0 && audio * 2 >= items {
            return PODCAST;
        }
        if video > 0 && video * 2 >= items {
            return VIDEO;
        }
    }
    std::iter::once(feed_url)
        .chain(site_url)
        .find_map(type_from_url)
        .unwrap_or(ARTICLE)
}

/// Well-known sites and URL shapes.
fn type_from_url(raw: &str) -> Option<&'static str> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let path = url.path().to_ascii_lowercase();
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));

    const VIDEO_SITES: &[&str] = &["youtube.com", "youtu.be", "vimeo.com", "dailymotion.com", "odysee.com"];
    const CODE_SITES: &[&str] = &["github.com", "gitlab.com", "codeberg.org", "sr.ht", "bitbucket.org", "gitea.com"];
    const DISCUSSION_SITES: &[&str] = &["reddit.com", "lobste.rs", "hnrss.org", "lemmy.world", "lemmy.ml"];
    const NEWSLETTER_SITES: &[&str] = &["substack.com", "buttondown.com", "buttondown.email", "beehiiv.com"];
    const PODCAST_HOSTS: &[&str] = &[
        "anchor.fm", "libsyn.com", "megaphone.fm", "simplecast.com", "transistor.fm", "buzzsprout.com",
        "podbean.com", "captivate.fm", "fireside.fm", "acast.com", "omny.fm", "spreaker.com", "podhome.fm",
    ];

    if VIDEO_SITES.iter().any(|d| on(d)) {
        return Some(VIDEO);
    }
    // Forges, including self-hosted GitLab, Gitea and Forgejo.
    if CODE_SITES.iter().any(|d| on(d))
        || path.ends_with("/releases.atom")
        || path.ends_with("/tags.atom")
        || path.ends_with("/commits.atom")
        || path.contains("/commits/")
    {
        return Some(CODE);
    }
    // Forums, link aggregators, Discourse (/latest.rss, /top.rss) and Mastodon profiles (/@name.rss).
    if DISCUSSION_SITES.iter().any(|d| on(d))
        || host == "news.ycombinator.com"
        || path.ends_with("/latest.rss")
        || path.ends_with("/top.rss")
        || (path.starts_with("/@") && path.ends_with(".rss"))
    {
        return Some(DISCUSSION);
    }
    if NEWSLETTER_SITES.iter().any(|d| on(d)) || host.starts_with("newsletter.") {
        return Some(NEWSLETTER);
    }
    if PODCAST_HOSTS.iter().any(|d| on(d)) {
        return Some(PODCAST);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(n: usize) -> Vec<(Option<String>, Option<String>)> {
        vec![(None, None); n]
    }

    fn item(mime: &str, url: &str) -> (Option<String>, Option<String>) {
        (Some(mime.to_string()).filter(|m| !m.is_empty()), Some(url.to_string()))
    }

    #[test]
    fn detects_by_address() {
        let d = |url: &str| detect(url, None, &[]);
        assert_eq!(d("https://www.youtube.com/feeds/videos.xml?channel_id=UC1"), VIDEO);
        assert_eq!(d("https://github.com/ollama/ollama/releases.atom"), CODE);
        assert_eq!(d("https://github.com/deepseek-ai/FlashMLA/commits/main.atom"), CODE);
        assert_eq!(d("https://git.example.org/team/app/tags.atom"), CODE);
        assert_eq!(d("https://git.sr.ht/~user/repo/log/rss.xml"), CODE);
        assert_eq!(d("https://www.reddit.com/r/LocalLLaMA/top/.rss?t=week"), DISCUSSION);
        assert_eq!(d("https://lobste.rs/t/openbsd.rss"), DISCUSSION);
        assert_eq!(d("https://discuss.haiku-os.org/top.rss?period=weekly"), DISCUSSION);
        assert_eq!(d("https://mastodon.social/@someone.rss"), DISCUSSION);
        assert_eq!(d("https://importai.substack.com/feed"), NEWSLETTER);
        assert_eq!(d("https://newsletter.semianalysis.com/feed"), NEWSLETTER);
        assert_eq!(d("https://feeds.fireside.fm/linuxunplugged/rss"), PODCAST);
        assert_eq!(d("https://www.freebsd.org/news/feed.xml"), ARTICLE);
        // github.blog and GitHub Pages sites are blogs, not code.
        assert_eq!(d("https://github.blog/feed/"), ARTICLE);
        assert_eq!(d("https://someone.github.io/feed.xml"), ARTICLE);
        // The site address counts too.
        assert_eq!(detect("https://feeds.example/x", Some("https://gitlab.com/group/project"), &[]), CODE);
    }

    #[test]
    fn detects_by_enclosures() {
        // Half or more of the recent items carry audio: a podcast, on any host.
        let mut items = none(2);
        items.push(item("audio/mpeg", "https://cdn.example/1.mp3"));
        items.push(item("", "https://cdn.example/2.m4a?x=1"));
        assert_eq!(detect("https://jupiter.example/allshows", None, &items), PODCAST);
        // Media beats the address: a podcast feed hosted on a "code" site is still a podcast.
        assert_eq!(detect("https://github.com/x/y/feed.xml", None, &items), PODCAST);

        // One episode among many posts is still a blog.
        let mut items = none(5);
        items.push(item("audio/mpeg", "https://cdn.example/1.mp3"));
        assert_eq!(detect("https://blog.example/feed", None, &items), ARTICLE);

        let videos = vec![item("video/mp4", "https://cdn.example/a.mp4"), item("", "https://cdn.example/b.webm")];
        assert_eq!(detect("https://peertube.example/feeds/videos.xml", None, &videos), VIDEO);

        // Images are not media.
        let images = vec![item("image/jpeg", "https://cdn.example/a.jpg"); 3];
        assert_eq!(detect("https://photos.example/feed", None, &images), ARTICLE);
    }

    #[test]
    fn audio_rule_matches_the_player() {
        assert!(is_audio(Some("Audio/MPEG"), None));
        assert!(is_audio(None, Some("https://x/ep.opus#t=10")));
        assert!(!is_audio(Some("video/mp4"), Some("https://x/a.mp4")));
        assert!(!is_audio(None, Some("https://x/mp3/page.html")));
        assert!(is_type("podcast") && !is_type("podcasts") && !is_type(""));
    }
}
