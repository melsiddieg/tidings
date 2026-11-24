//! The language of an article, for the language filter above the list.
//!
//! It is worked out from the title and excerpt when the article is stored: first by
//! the writing system (Arabic, Chinese, Cyrillic, ...), then, for Latin letters, by
//! common short words ("the", "und", "les", ...). Text that does not show its
//! language clearly gets "" and takes the usual language of its feed
//! (`db::detect_feed_language`), so a release called "v2.4.1" goes with the other
//! articles of its feed. A detector trained on letter groups did worse on this
//! kind of text: it took short English titles for French or Danish, and Chinese
//! posts with some English words in them for Latin languages.

/// The languages the filter can show. "und" (undetermined) is used for articles
/// whose language neither they nor their feed show: "Other" in the UI.
pub const CODES: &[&str] = &[
    "en", "fr", "de", "es", "pt", "it", "nl", "ar", "fa", "ur", "zh", "ja", "ko", "ru", "uk", "el", "he", "th", "hi",
    UNKNOWN,
];

pub const UNKNOWN: &str = "und";

pub fn is_code(code: &str) -> bool {
    CODES.contains(&code)
}

/// How much of the title and excerpt is read. Excerpts are at most 280 characters.
const SAMPLE: usize = 600;

/// How many of a feed's newest articles decide its usual language.
pub const RECENT_ITEMS: i64 = 50;

/// A feed's usual language, from the languages of its newest articles ("" for those
/// whose text does not show it): the one most of them are in, when it is more than
/// half of those that show a language and at least a fifth of all. A feed of
/// releases called "v2.4.1" with one French-looking title is not French. Else the
/// language the feed declares ("en-US"), else "".
pub fn usual(articles: &[&str], declared: Option<&str>) -> &'static str {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for lang in articles {
        let Some(code) = CODES.iter().copied().find(|c| c == lang && *c != UNKNOWN) else { continue };
        match counts.iter_mut().find(|(c, _)| *c == code) {
            Some((_, n)) => *n += 1,
            None => counts.push((code, 1)),
        }
    }
    let known: usize = counts.iter().map(|(_, n)| n).sum();
    if let Some(&(code, top)) = counts.iter().max_by_key(|(_, n)| *n) {
        if top * 2 > known && top * 5 >= articles.len() {
            return code;
        }
    }
    declared.and_then(self::declared).unwrap_or("")
}

/// The language a feed declares, as one of `CODES`: "en-US" and "en_us" are "en",
/// "zh-Hant" is "zh". None for a language the filter does not know.
pub fn declared(tag: &str) -> Option<&'static str> {
    let primary = tag.trim().split(['-', '_']).next()?.to_ascii_lowercase();
    // Hebrew's old code, still in some feeds.
    let primary = if primary == "iw" { "he".to_string() } else { primary };
    CODES.iter().copied().find(|c| *c == primary && *c != UNKNOWN)
}

/// Chinese characters (CJK Unified Ideographs and their extensions), which Chinese
/// and Japanese share.
pub fn is_han(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{2EBEF}' | '\u{30000}'..='\u{3134F}')
}

/// Hiragana and katakana, which only Japanese uses.
pub fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}')
}

/// Arabic letters, also those of Persian and Urdu, which use the same script.
pub fn is_arabic_letter(c: char) -> bool {
    c.is_alphabetic()
        && matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}'
            | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Han,
    Kana,
    Hangul,
    Arabic,
    Cyrillic,
    Greek,
    Hebrew,
    Thai,
    Devanagari,
    Other,
}

fn script(c: char) -> Option<Script> {
    if !c.is_alphabetic() {
        return None;
    }
    Some(match c {
        _ if c.is_ascii_alphabetic() => Script::Latin,
        '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}' => Script::Latin,
        _ if is_han(c) => Script::Han,
        _ if is_kana(c) => Script::Kana,
        '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' => Script::Hangul,
        _ if is_arabic_letter(c) => Script::Arabic,
        '\u{0400}'..='\u{052F}' => Script::Cyrillic,
        '\u{0370}'..='\u{03FF}' | '\u{1F00}'..='\u{1FFF}' => Script::Greek,
        '\u{0590}'..='\u{05FF}' => Script::Hebrew,
        '\u{0E00}'..='\u{0E7F}' => Script::Thai,
        '\u{0900}'..='\u{097F}' => Script::Devanagari,
        _ => Script::Other,
    })
}

/// The language of an article from its title and excerpt (plain text): one of
/// `CODES` other than "und", or "" when the text does not show it clearly.
pub fn detect(title: &str, excerpt: &str) -> &'static str {
    // Many feeds start the text with the title: it would count twice.
    let excerpt = excerpt.trim_start().strip_prefix(title.trim()).unwrap_or(excerpt);
    let sample: String = title.chars().chain([' ']).chain(excerpt.chars()).take(SAMPLE).collect();
    let sample = without_addresses(&sample);
    let mut counts = [0usize; 11];
    let index = |s: Script| s as usize;
    for c in sample.chars() {
        if let Some(s) = script(c) {
            counts[index(s)] += 1;
        }
    }
    // A Chinese character or a Hangul block holds about as much as a short word, so
    // it counts twice against letters (translate::is_chinese does the same).
    let weight = |s: Script| match s {
        Script::Han | Script::Kana | Script::Hangul => 2,
        _ => 1,
    };
    let han = counts[index(Script::Han)];
    let kana = counts[index(Script::Kana)];
    let score = |s: Script| match s {
        // Japanese mixes Chinese characters and kana: they count together.
        Script::Han | Script::Kana => (han + kana) * 2,
        _ => counts[index(s)] * weight(s),
    };
    // Latin comes last, so it loses ties: a Chinese post that names software in
    // English is Chinese.
    let mut scripts = vec![
        Script::Han,
        Script::Hangul,
        Script::Arabic,
        Script::Cyrillic,
        Script::Greek,
        Script::Hebrew,
        Script::Thai,
        Script::Devanagari,
        Script::Other,
        Script::Latin,
    ];
    scripts.retain(|&s| score(s) >= 6);
    scripts.sort_by_key(|&s| std::cmp::Reverse(score(s)));
    // The main writing system decides. When it cannot (Latin letters that are only
    // names and version numbers), the next one with enough letters does.
    for s in scripts {
        let lang = match s {
            Script::Latin => latin(&sample),
            // Japanese writes its grammar in kana; Chinese text uses none, apart
            // from a quoted Japanese name.
            Script::Han | Script::Kana => {
                if kana >= 2 && kana * 3 >= han {
                    "ja"
                } else {
                    "zh"
                }
            }
            Script::Hangul => "ko",
            Script::Arabic => arabic(&sample),
            Script::Cyrillic => cyrillic(&sample),
            Script::Greek => "el",
            Script::Hebrew => "he",
            Script::Thai => "th",
            Script::Devanagari => "hi",
            Script::Other => "",
        };
        if !lang.is_empty() {
            return lang;
        }
    }
    ""
}

/// `text` without web addresses, file names and handles: their letters are no
/// language ("com" would be Portuguese), and a post that lists its links would
/// count as Latin text.
fn without_addresses(text: &str) -> String {
    let mut kept = Vec::new();
    for word in text.split_whitespace() {
        // Chinese and Japanese put no space before a link: keep the text before it.
        let word = match word.find("://") {
            Some(i) => word[..i].trim_end_matches(|c: char| c.is_ascii_alphanumeric()),
            None => word,
        };
        let address = word.is_ascii()
            && (word.contains('/')
                || word.contains('@')
                || word.as_bytes().windows(2).any(|w| w[0] == b'.' && w[1].is_ascii_alphanumeric()));
        if !word.is_empty() && !address {
            kept.push(word);
        }
    }
    kept.join(" ")
}

/// Arabic, Persian or Urdu, by the letters only one of them uses. Persian writes
/// ی and ک where Arabic writes ي and ك; Urdu adds ٹ ڈ ڑ ں ے.
fn arabic(text: &str) -> &'static str {
    let (mut arabic, mut persian, mut urdu) = (0usize, 0usize, 0usize);
    for c in text.chars() {
        match c {
            'ي' | 'ك' | 'ة' | 'ى' => arabic += 1,
            'ی' | 'ک' | 'پ' | 'چ' | 'ژ' | 'گ' => persian += 1,
            'ٹ' | 'ڈ' | 'ڑ' | 'ں' | 'ے' | 'ۓ' => urdu += 1,
            _ => {}
        }
    }
    if urdu >= 2 && urdu * 2 >= arabic {
        "ur"
    } else if persian > arabic {
        "fa"
    } else {
        "ar"
    }
}

/// Russian or Ukrainian, by the letters only one of them uses. Other languages
/// written in Cyrillic (Bulgarian, Serbian, ...) use neither set and stay unknown.
fn cyrillic(text: &str) -> &'static str {
    let (mut russian, mut ukrainian) = (0usize, 0usize);
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ы' | 'э' | 'ё' => russian += 1,
            'і' | 'ї' | 'є' | 'ґ' => ukrainian += 1,
            _ => {}
        }
    }
    if ukrainian > russian {
        "uk"
    } else if russian > 0 {
        "ru"
    } else {
        ""
    }
}

/// The most common short words of each language written in Latin letters. Words of
/// one letter are left out: "a" and "i" are English as much as anything else. Words
/// that two languages share count for both ("do", "as"), so only the others decide.
const WORDS: &[(&str, &[&str])] = &[
    ("en", &[
        "the", "and", "of", "to", "in", "is", "for", "on", "with", "that", "this", "are", "was", "as", "at", "by",
        "from", "be", "it", "an", "or", "have", "has", "not", "you", "your", "we", "our", "how", "what", "why",
        "new", "will", "can", "its", "about", "after", "into", "more", "over", "than", "their", "they", "which",
        "who", "but", "all", "been", "were", "when", "there", "just", "do", "no",
    ]),
    ("fr", &[
        "le", "la", "les", "des", "du", "de", "et", "est", "une", "un", "pour", "que", "qui", "dans", "sur", "pas",
        "avec", "au", "aux", "ce", "cette", "ces", "sont", "par", "mais", "ou", "nous", "vous", "il",
        "elle", "son", "sa", "ses", "leur", "été", "être", "en", "comme", "aussi",
    ]),
    ("de", &[
        "der", "die", "das", "und", "ist", "nicht", "mit", "den", "von", "zu", "ein", "eine", "einen", "auf",
        "für", "sich", "dem", "des", "im", "auch", "wie", "oder", "über", "bei", "nach", "wird", "werden", "sind",
        "es", "ich", "wir", "sie", "aus", "noch", "um", "nur", "kann", "hat", "mehr", "zum", "zur", "dass", "als",
        "in", "an",
    ]),
    ("es", &[
        "el", "la", "los", "las", "de", "que", "en", "del", "por", "con", "para", "una", "un", "es", "se", "como",
        "más", "pero", "sus", "su", "al", "lo", "está", "este", "esta", "son", "sobre", "también", "fue", "han",
        "hay", "ya", "muy", "sin", "entre", "cuando", "desde", "nos",
    ]),
    ("pt", &[
        "os", "as", "de", "que", "em", "do", "da", "dos", "das", "para", "com", "uma", "um", "não", "no", "na",
        "nos", "nas", "ao", "pelo", "pela", "mais", "se", "por", "como", "mas", "foi", "são", "sua", "seu",
        "você", "isso", "também", "está", "ser", "ter", "já", "muito",
    ]),
    ("it", &[
        "il", "lo", "la", "gli", "le", "di", "che", "con", "una", "un", "non", "sono", "del", "della",
        "dei", "delle", "nel", "nella", "al", "alla", "da", "si", "come", "più", "anche", "ma", "questo",
        "questa", "ha", "ci", "se", "tra", "essere", "stato",
    ]),
    ("nl", &[
        "de", "het", "een", "van", "en", "is", "dat", "op", "niet", "met", "voor", "zijn", "ook", "aan", "worden",
        "naar", "bij", "maar", "door", "wordt", "er", "die", "dit", "om", "te", "als", "nog", "wel", "geen", "ze",
        "hij", "ik", "je", "we", "uit", "over", "kan", "heeft", "zo", "tot", "al",
    ]),
];

/// A language written in Latin letters, by its common words: the language with the
/// most of them, when it has at least two more than any other. "" otherwise.
fn latin(text: &str) -> &'static str {
    // Each word, with the languages that use it as bits (bit i: WORDS[i]). One
    // lookup per word: the upgrade to v10 reads every stored article.
    static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, u8>> = std::sync::OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for (i, (_, words)) in WORDS.iter().enumerate() {
            for word in *words {
                *map.entry(*word).or_insert(0u8) |= 1 << i;
            }
        }
        map
    });
    let lower = text.to_lowercase();
    let mut hits = [0usize; WORDS.len()];
    for word in lower.split(|c: char| !c.is_alphabetic()).filter(|w| w.chars().nth(1).is_some()) {
        if let Some(bits) = index.get(word) {
            for (i, hit) in hits.iter_mut().enumerate() {
                if bits & (1 << i) != 0 {
                    *hit += 1;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..WORDS.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(hits[i]));
    let (best, second) = (hits[order[0]], hits[order[1]]);
    if best >= 2 && best >= second + 2 {
        WORDS[order[0]].0
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_systems_name_the_language() {
        assert_eq!(detect("开源模型改变人工智能的发展", ""), "zh");
        assert_eq!(detect("OpenAI 发布了新的模型，性能大幅提升", ""), "zh");
        assert_eq!(detect("台灣的科技產業持續成長", ""), "zh");
        assert_eq!(detect("Vercel日本法人の代表に花尾和成が就任いたしました", ""), "ja");
        assert_eq!(detect("서울에서 열린 인공지능 회의", ""), "ko");
        assert_eq!(detect("الذكاء الاصطناعي يغير طريقة عملنا", ""), "ar");
        assert_eq!(detect("هوش مصنوعی چگونه کار می‌کند", ""), "fa");
        assert_eq!(detect("پاکستان میں بارش کے بعد سیلاب کا خطرہ ہے", ""), "ur");
        assert_eq!(detect("Новая версия вышла сегодня", "Это первый выпуск за год."), "ru");
        assert_eq!(detect("Нова версія вийшла сьогодні", "Це перший випуск за рік."), "uk");
        assert_eq!(detect("Η νέα έκδοση κυκλοφόρησε σήμερα", ""), "el");
        assert_eq!(detect("הגרסה החדשה יצאה היום", ""), "he");
        assert_eq!(detect("ข่าววันนี้ เกี่ยวกับเทคโนโลยี", ""), "th");
        assert_eq!(detect("आज की ताज़ा ख़बरें", ""), "hi");
    }

    #[test]
    fn latin_languages_are_told_by_their_words() {
        assert_eq!(detect("How to build a compiler in Rust", ""), "en");
        assert_eq!(
            detect("Nvidia reinstates Jensen Huang as CEO", "The board said on Monday that the decision was final."),
            "en"
        );
        assert_eq!(detect("Musique Nomade : 20 ans à faire voyager la musique et la culture des artistes", ""), "fr");
        assert_eq!(detect("Die neue Version ist da", "Sie bringt mehr Tempo und weniger Fehler mit sich."), "de");
        assert_eq!(detect("El gobierno anunció las nuevas medidas", "Los cambios entran en vigor por la mañana."), "es");
        assert_eq!(detect("O governo anunciou as novas medidas", "Os preços não vão subir para os clientes."), "pt");
        assert_eq!(detect("Il governo ha annunciato le nuove misure", "Gli aumenti non sono previsti per questo anno."), "it");
        assert_eq!(detect("De nieuwe versie is er", "Het is een grote stap voor het project en de gebruikers."), "nl");
    }

    #[test]
    fn unclear_text_is_left_to_the_feed() {
        assert_eq!(detect("v2.54.1", ""), "");
        assert_eq!(detect("Rust in production", ""), "");
        assert_eq!(detect("如题", ""), "");
        assert_eq!(detect("", ""), "");
        // Bulgarian uses neither the Russian nor the Ukrainian letters.
        assert_eq!(detect("Новата версия излезе днес", "Тя е по-бърза от старата."), "");
    }

    #[test]
    fn mixed_text_goes_by_the_main_writing_system() {
        // Chinese posts about software name it in English.
        assert_eq!(detect("Neovim 悬浮滚动条 scrollbar.nvim", "scrollbar.nvim 插件介绍：为 Neovim 提供悬浮滚动条功能。"), "zh");
        assert_eq!(
            detect("Kimi Code Desktop 1.5.0-canary.20261124032414-d22ffe8", "目标版本：1.5.0；来源：main @ d22ffe8。CDN 需手动刷新"),
            "zh"
        );
        // A Chinese title with a Japanese name in brackets.
        assert_eq!(detect("【钢琴独奏谱】世界第一的公主殿下 CPK! Remix（超时空辉夜姬插入曲）", "视频预览 【自购分享 | 官方钢琴独奏谱】世界第一的公主殿下 (かぐや & 月見ヤチヨ)"), "zh");
        // An Arabic post with an English name.
        assert_eq!(detect("تنوين بودكاست | Tanween Podcasts", "حلقة جديدة عن تاريخ الخط العربي"), "ar");
    }
}
