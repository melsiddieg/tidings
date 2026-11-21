//! Filter rules: "when the title contains X, mark as read / star / delete".
//!
//! A rule runs on each new article when a refresh stores it (`db::store_articles`),
//! and on existing articles only when the user asks (`db::apply_rule`).
//!
//! Matching is plain text, never a regular expression, so no rule can make matching
//! slow. A rule holds phrases separated by commas; one is enough. A phrase matches
//! whole words, in any case: "ai" does not match "said". A `*` at its end matches
//! the start of a word: "crypto*" matches "cryptocurrency". Chinese, Japanese and
//! Korean have no spaces between words, so a phrase in them matches anywhere.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A rule's words, at most this many characters, in at most `MAX_PHRASES` phrases.
const MAX_WORDS: usize = 1_000;
const MAX_PHRASES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// 0 for a rule that is not stored yet.
    pub id: i64,
    pub enabled: bool,
    /// The feed or the folder (with the folders inside it) the rule is for. Neither:
    /// all feeds.
    pub feed_id: Option<i64>,
    pub folder_id: Option<i64>,
    /// "title", "text" (title or text), "author" or "link".
    pub field: String,
    pub words: String,
    /// "read", "star" or "delete".
    pub action: String,
    /// How many articles the rule matched.
    pub hits: i64,
}

impl Rule {
    /// The rule with its words tidied, or why it cannot be saved.
    pub fn validated(mut self) -> Result<Rule> {
        if !matches!(self.field.as_str(), "title" | "text" | "author" | "link") {
            return Err(Error::msg("Choose what the rule looks at"));
        }
        if !matches!(self.action.as_str(), "read" | "star" | "delete") {
            return Err(Error::msg("Choose what the rule does"));
        }
        if self.feed_id.is_some() && self.folder_id.is_some() {
            return Err(Error::msg("A rule is for one feed or one folder"));
        }
        self.words = phrases(&self.words).join(", ");
        if self.words.is_empty() {
            return Err(Error::msg("Enter the words the rule looks for"));
        }
        if self.words.chars().count() > MAX_WORDS || self.words.split(", ").count() > MAX_PHRASES {
            return Err(Error::msg("A rule can hold at most 50 phrases and 1000 characters"));
        }
        Ok(self)
    }

    /// Whether `article` matches the rule.
    pub fn matches(&self, article: &Article) -> bool {
        let haystack = match self.field.as_str() {
            "title" => article.title.clone(),
            "text" => format!("{}\n{}", article.title, article.text),
            "author" => article.author.clone(),
            "link" => article.url.clone(),
            _ => return false,
        };
        // One space between words, as in the phrases.
        let haystack = haystack.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        phrases(&self.words).iter().any(|p| phrase_in(&haystack, p))
    }
}

/// What a rule can look at in an article.
pub struct Article {
    pub title: String,
    /// The article's text, without markup.
    pub text: String,
    pub author: String,
    pub url: String,
}

/// The phrases of a rule's words, lowercase.
fn phrases(words: &str) -> Vec<String> {
    words
        .split([',', '，', '、', '\n'])
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase())
        .filter(|p| !p.is_empty() && p != "*")
        .collect()
}

/// Letters of scripts written without spaces between words.
fn unspaced(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}'
        | '\u{FF66}'..='\u{FF9F}' | '\u{20000}'..='\u{3134F}')
}

/// Whether two characters next to each other are in one word.
fn joined(a: Option<char>, b: Option<char>) -> bool {
    let word = |c: char| c.is_alphanumeric() && !unspaced(c);
    matches!((a, b), (Some(a), Some(b)) if word(a) && word(b))
}

/// Whether the lowercase `phrase` is in the lowercase `haystack` as whole words.
fn phrase_in(haystack: &str, phrase: &str) -> bool {
    let (needle, prefix) = match phrase.strip_suffix('*') {
        Some(start) => (start.trim_end(), true),
        None => (phrase, false),
    };
    let Some(first) = needle.chars().next() else { return false };
    let last = needle.chars().next_back();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let starts_word = !joined(haystack[..start].chars().next_back(), Some(first));
        let ends_word = prefix || !joined(last, haystack[end..].chars().next());
        if starts_word && ends_word {
            return true;
        }
        from = start + first.len_utf8();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(field: &str, words: &str) -> Rule {
        Rule {
            id: 0,
            enabled: true,
            feed_id: None,
            folder_id: None,
            field: field.into(),
            words: words.into(),
            action: "read".into(),
            hits: 0,
        }
    }

    fn article(title: &str, text: &str) -> Article {
        Article { title: title.into(), text: text.into(), author: "Jane Doe".into(), url: "https://example.com/sponsored/1".into() }
    }

    #[test]
    fn phrases_match_whole_words_in_any_case() {
        let ai = rule("title", "AI");
        assert!(ai.matches(&article("New AI model released", "")));
        assert!(ai.matches(&article("ai: what next?", "")));
        assert!(!ai.matches(&article("She said no", "")));
        assert!(!ai.matches(&article("Mainframe news", "")));
        // Several phrases: one is enough. Phrases of several words keep their order.
        let crypto = rule("title", "bitcoin, crypto*, \"web3\", stock market");
        assert!(crypto.matches(&article("Bitcoin hits a new high", "")));
        assert!(crypto.matches(&article("Cryptocurrency rules", "")));
        assert!(crypto.matches(&article("The  STOCK\nmarket", "")));
        assert!(!crypto.matches(&article("Market stock", "")));
        assert!(!crypto.matches(&article("Encrypto", "")));
        // Letters outside ASCII.
        assert!(rule("title", "ÉTÉ").matches(&article("Un été chaud", "")));
        assert!(!rule("title", "été").matches(&article("Les étés", "")));
    }

    #[test]
    fn chinese_matches_inside_sentences() {
        let ad = rule("text", "广告，推广");
        assert!(ad.matches(&article("今日新闻", "本文包含推广内容。")));
        assert!(rule("title", "人工智能").matches(&article("中国人工智能发展报告", "")));
        assert!(!rule("title", "人工智能").matches(&article("机器学习", "")));
        // Japanese and Korean too.
        assert!(rule("title", "広告").matches(&article("これは広告です", "")));
    }

    #[test]
    fn fields() {
        let a = article("Title", "Body text about Rust");
        assert!(!rule("title", "rust").matches(&a));
        assert!(rule("text", "rust").matches(&a));
        assert!(rule("author", "doe").matches(&a));
        assert!(rule("link", "sponsored").matches(&a));
        assert!(!rule("link", "example.co").matches(&a));
    }

    #[test]
    fn rules_are_checked_before_saving() {
        let tidy = rule("title", "  Bitcoin ,, crypto*  ,  ").validated().unwrap();
        assert_eq!(tidy.words, "bitcoin, crypto*");
        assert!(rule("title", " , * ").validated().is_err());
        assert!(rule("body", "x").validated().is_err());
        assert!(Rule { action: "explode".into(), ..rule("title", "x") }.validated().is_err());
        assert!(Rule { feed_id: Some(1), folder_id: Some(2), ..rule("title", "x") }.validated().is_err());
        assert!(rule("title", &"x, ".repeat(60)).validated().is_err());
    }
}
