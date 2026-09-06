//! §16.23: a persona's feed — the machine-read list of posts in a home's
//! bundle — and the timeline made from the feeds of the people you keep.
//!
//! One implementation for both clients: the parser is strict (a closed
//! set, pinned limits, only bundle paths and `ducat:` addresses), the text
//! subset is small enough to render natively, and the pages a publisher
//! writes for the sealed room come from the same document, so a viewer
//! that knows nothing of feeds still reads every post.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const FEED_VERSION: u64 = 1;
pub const FEED_FILE: &str = "feed.json";
pub const MAX_POSTS: usize = 200;
pub const MAX_TEXT: usize = 4000;
pub const MAX_NAME: usize = 80;
pub const MAX_MEDIA: usize = 8;
pub const MAX_FILES: usize = 8;
pub const MAX_PATH: usize = 200;
pub const MAX_ALT: usize = 300;
const FILE_PREFIX: &str = "ducat:file/";

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FeedError {
    #[error("feed: {0}")]
    Refused(String),
}

fn refuse<T>(msg: impl Into<String>) -> Result<T, FeedError> {
    Err(FeedError::Refused(msg.into()))
}

// ----- the document ---------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct FeedMedia {
    /// A file inside the bundle — the thumbnail, or the picture itself
    /// when it is small.
    pub path: String,
    /// The full-size share, `ducat:file/…`, when the bundle holds only a
    /// thumbnail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    pub mime: String,
    pub bytes: u64,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub alt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct FeedFile {
    pub name: String,
    /// `ducat:file/…`, an immutable share.
    pub addr: String,
    pub mime: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct FeedRef {
    pub persona: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct FeedPost {
    pub id: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<u64>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<FeedMedia>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FeedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub re: Option<FeedRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct FeedDoc {
    pub v: u64,
    pub persona: String,
    pub name: String,
    pub updated: u64,
    pub posts: Vec<FeedPost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older: Option<String>,
}

/// One post as it sits in a timeline: whose it is, and the post.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, uniffi::Record)]
pub struct FeedEntry {
    pub persona: String,
    pub name: String,
    pub post: FeedPost,
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A path inside the bundle: relative, no scheme, no `..`, nothing that
/// a room would resolve outside the bundle.
pub fn bundle_path_ok(p: &str) -> bool {
    if p.is_empty() || p.len() > MAX_PATH || p.contains('\\') || p.contains('\0') {
        return false;
    }
    if p.contains("://") || p.starts_with("//") || p.contains(':') {
        return false;
    }
    let trimmed = p.trim_start_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    !trimmed.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..")
}

/// Where a link or an image may point: inside the bundle, or at a
/// `ducat:` address. Anything else reaches past the room.
pub fn target_ok(t: &str) -> bool {
    if t.starts_with("ducat:") {
        return t.len() > 6 && !t.contains(char::is_whitespace);
    }
    bundle_path_ok(t)
}

fn share_addr_ok(a: &str) -> bool {
    a.starts_with(FILE_PREFIX) && a.len() > FILE_PREFIX.len() && !a.contains(char::is_whitespace)
}

pub(crate) fn mime_ok(m: &str) -> bool {
    let mut parts = m.splitn(2, '/');
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => {
            !a.is_empty()
                && !b.is_empty()
                && m.len() <= 100
                && m.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'-' | b'.' | b'+'))
        }
        _ => false,
    }
}

/// Check a document the way a reader must: every limit, every target.
pub fn feed_check(doc: &FeedDoc) -> Result<(), FeedError> {
    if doc.v != FEED_VERSION {
        return refuse(format!("version {} is not {}", doc.v, FEED_VERSION));
    }
    if doc.persona.len() != 64 || !is_lower_hex(&doc.persona) {
        return refuse("persona is not a key");
    }
    if doc.name.chars().count() > MAX_NAME {
        return refuse("name too long");
    }
    if doc.posts.len() > MAX_POSTS {
        return refuse(format!("more than {MAX_POSTS} posts in one page"));
    }
    if let Some(o) = &doc.older {
        if !bundle_path_ok(o) {
            return refuse("older is not a bundle path");
        }
    }
    let mut ids = HashSet::new();
    let mut last_at = u64::MAX;
    for p in &doc.posts {
        if p.id.len() < 8 || p.id.len() > 32 || !is_lower_hex(&p.id) {
            return refuse(format!("post id {:?} is not 8–32 lowercase hex", p.id));
        }
        if !ids.insert(p.id.clone()) {
            return refuse(format!("post id {} appears twice", p.id));
        }
        if p.at > last_at {
            return refuse("posts are not newest first");
        }
        last_at = p.at;
        if p.text.chars().count() > MAX_TEXT {
            return refuse(format!("post {} text too long", p.id));
        }
        if p.media.len() > MAX_MEDIA {
            return refuse(format!("post {} has more than {MAX_MEDIA} media", p.id));
        }
        if p.files.len() > MAX_FILES {
            return refuse(format!("post {} has more than {MAX_FILES} files", p.id));
        }
        for m in &p.media {
            if !bundle_path_ok(&m.path) {
                return refuse(format!("post {} media path {:?} is not inside the bundle", p.id, m.path));
            }
            if let Some(f) = &m.full {
                if !share_addr_ok(f) {
                    return refuse(format!("post {} media full {:?} is not a share", p.id, f));
                }
            }
            if !mime_ok(&m.mime) {
                return refuse(format!("post {} media mime {:?}", p.id, m.mime));
            }
            if m.alt.chars().count() > MAX_ALT {
                return refuse(format!("post {} alt too long", p.id));
            }
        }
        for f in &p.files {
            if f.name.is_empty() || f.name.chars().count() > MAX_PATH || f.name.contains('/') || f.name.contains('\\') {
                return refuse(format!("post {} file name {:?}", p.id, f.name));
            }
            if !share_addr_ok(&f.addr) {
                return refuse(format!("post {} file {:?} is not a share", p.id, f.name));
            }
            if !mime_ok(&f.mime) {
                return refuse(format!("post {} file mime {:?}", p.id, f.mime));
            }
        }
        if let Some(r) = &p.re {
            if r.persona.len() != 64 || !is_hex(&r.persona) || r.id.len() < 8 || r.id.len() > 32 || !is_lower_hex(&r.id) {
                return refuse(format!("post {} answers something that is not a post", p.id));
            }
        }
        // Every target in the text, by the same rule as the fields.
        for b in text_blocks(&p.text) {
            match b {
                FeedBlock::Image { path, .. } => {
                    if !bundle_path_ok(&path) {
                        return refuse(format!("post {} image {:?} is not inside the bundle", p.id, path));
                    }
                }
                FeedBlock::Paragraph { spans } => {
                    for s in spans {
                        if let Some(l) = &s.link {
                            if !target_ok(l) {
                                return refuse(format!("post {} links past the room: {:?}", p.id, l));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Read a feed as a strict reader: the whole document or nothing.
#[uniffi::export]
pub fn feed_parse(json: String) -> Result<FeedDoc, FeedError> {
    if json.len() > 4 * 1024 * 1024 {
        return refuse("feed larger than 4 MiB");
    }
    let doc: FeedDoc = serde_json::from_str(&json).map_err(|e| FeedError::Refused(format!("not a feed: {e}")))?;
    feed_check(&doc)?;
    Ok(doc)
}

/// Write a feed the way it is read: posts newest first, nothing extra.
#[uniffi::export]
pub fn feed_encode(doc: FeedDoc) -> Result<String, FeedError> {
    let mut doc = doc;
    doc.posts.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
    feed_check(&doc)?;
    serde_json::to_string_pretty(&doc).map_err(|e| FeedError::Refused(e.to_string()))
}

/// The timeline: every feed's posts together, newest first; ties break
/// by author key then post id so two readers agree on the order.
#[uniffi::export]
pub fn feed_merge(feeds: Vec<FeedDoc>, limit: u32) -> Vec<FeedEntry> {
    let mut out: Vec<FeedEntry> = feeds
        .into_iter()
        .flat_map(|d| {
            let (persona, name) = (d.persona, d.name);
            d.posts.into_iter().map(move |p| FeedEntry { persona: persona.clone(), name: name.clone(), post: p })
        })
        .collect();
    out.sort_by(|a, b| b.post.at.cmp(&a.post.at).then_with(|| a.persona.cmp(&b.persona)).then_with(|| b.post.id.cmp(&a.post.id)));
    out.truncate(limit as usize);
    out
}

// ----- the text subset -------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, uniffi::Record)]
pub struct FeedSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// A bundle path or a `ducat:` URI; nothing else survives the check.
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, uniffi::Enum)]
#[serde(tag = "kind")]
pub enum FeedBlock {
    Paragraph { spans: Vec<FeedSpan> },
    Image { path: String, alt: String },
}

/// Paragraphs, bold, italic, links and images — and nothing else is
/// markup. A renderer draws every other character as itself.
#[uniffi::export]
pub fn feed_blocks(text: String) -> Vec<FeedBlock> {
    text_blocks(&text)
}

pub(crate) fn text_blocks(text: &str) -> Vec<FeedBlock> {
    let mut out = Vec::new();
    for para in text.replace("\r\n", "\n").split("\n\n") {
        let para = para.trim_matches('\n');
        if para.trim().is_empty() {
            continue;
        }
        // An image alone on a line is its own block; anything else is
        // a paragraph of spans, images inside it lifted out in order.
        let mut spans: Vec<FeedSpan> = Vec::new();
        for (li, line) in para.split('\n').enumerate() {
            let t = line.trim();
            if let Some((alt, path)) = parse_image_line(t) {
                if !spans.is_empty() {
                    out.push(FeedBlock::Paragraph { spans: std::mem::take(&mut spans) });
                }
                out.push(FeedBlock::Image { path, alt });
                continue;
            }
            if li > 0 && !spans.is_empty() {
                spans.push(FeedSpan { text: "\n".into(), bold: false, italic: false, link: None });
            }
            inline_spans(line, &mut spans);
        }
        if !spans.is_empty() {
            out.push(FeedBlock::Paragraph { spans });
        }
    }
    out
}

fn parse_image_line(t: &str) -> Option<(String, String)> {
    let rest = t.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = &rest[..close];
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    if end + 1 != after.len() {
        return None;
    }
    Some((alt.to_string(), after[..end].to_string()))
}

fn inline_spans(line: &str, spans: &mut Vec<FeedSpan>) {
    let chars: Vec<char> = line.chars().collect();
    let mut bold = false;
    let mut italic = false;
    let mut buf = String::new();
    let mut i = 0;
    let flush = |buf: &mut String, spans: &mut Vec<FeedSpan>, bold: bool, italic: bool| {
        if !buf.is_empty() {
            spans.push(FeedSpan { text: std::mem::take(buf), bold, italic, link: None });
        }
    };
    while i < chars.len() {
        // **bold** — an opener counts only when a closer follows on the
        // line; otherwise the stars are stars.
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if bold || bold_closes_after(&chars, i + 2) {
                flush(&mut buf, spans, bold, italic);
                bold = !bold;
                i += 2;
                continue;
            }
            buf.push_str("**");
            i += 2;
            continue;
        }
        // *italic* — a lone star followed by a non-space opens when a
        // closer follows, a lone star after a non-space closes; a star
        // with spaces both sides, or one without a partner, is a star.
        if chars[i] == '*' {
            let next_ok = chars.get(i + 1).map_or(false, |c| !c.is_whitespace());
            let prev_ok = i > 0 && !chars[i - 1].is_whitespace();
            if italic && prev_ok {
                flush(&mut buf, spans, bold, italic);
                italic = false;
                i += 1;
                continue;
            }
            if !italic && next_ok && italic_closes_after(&chars, i + 1) {
                flush(&mut buf, spans, bold, italic);
                italic = true;
                i += 1;
                continue;
            }
        }
        // [text](target)
        if chars[i] == '[' {
            if let Some((text, target, used)) = parse_link(&chars[i..]) {
                flush(&mut buf, spans, bold, italic);
                spans.push(FeedSpan { text, bold, italic, link: Some(target) });
                i += used;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut buf, spans, bold, italic);
}

fn bold_closes_after(c: &[char], from: usize) -> bool {
    let mut j = from;
    while j + 1 < c.len() {
        if c[j] == '*' && c[j + 1] == '*' {
            return true;
        }
        j += 1;
    }
    false
}

fn italic_closes_after(c: &[char], from: usize) -> bool {
    let mut j = from;
    while j < c.len() {
        if c[j] == '*' && !(j + 1 < c.len() && c[j + 1] == '*') && !(j > 0 && c[j - 1] == '*') && j > 0 && !c[j - 1].is_whitespace() {
            return true;
        }
        j += 1;
    }
    false
}

fn parse_link(c: &[char]) -> Option<(String, String, usize)> {
    // c[0] == '['
    let close = c.iter().position(|&x| x == ']')?;
    if c.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = c[close + 2..].iter().position(|&x| x == ')')? + close + 2;
    let text: String = c[1..close].iter().collect();
    let target: String = c[close + 2..end].iter().collect();
    if text.is_empty() || target.is_empty() || target.contains(char::is_whitespace) {
        return None;
    }
    Some((text, target, end + 1))
}

// ----- pages for the room ------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn blocks_html(blocks: &[FeedBlock], base: &str) -> String {
    let mut h = String::new();
    for b in blocks {
        match b {
            FeedBlock::Image { path, alt } => {
                if bundle_path_ok(path) {
                    h.push_str(&format!("<p><img src=\"{}{}\" alt=\"{}\"></p>\n", base, esc(path.trim_start_matches('/')), esc(alt)));
                }
            }
            FeedBlock::Paragraph { spans } => {
                h.push_str("<p>");
                for s in spans {
                    let mut t = esc(&s.text).replace('\n', "<br>");
                    if s.bold {
                        t = format!("<strong>{t}</strong>");
                    }
                    if s.italic {
                        t = format!("<em>{t}</em>");
                    }
                    match &s.link {
                        Some(l) if target_ok(l) => {
                            let href = if l.starts_with("ducat:") { l.clone() } else { format!("{}{}", base, l.trim_start_matches('/')) };
                            h.push_str(&format!("<a href=\"{}\">{}</a>", esc(&href), t));
                        }
                        _ => h.push_str(&t),
                    }
                }
                h.push_str("</p>\n");
            }
        }
    }
    h
}

fn when(at: u64) -> String {
    // Days since the epoch → a civil date, no clock library needed.
    let days = (at / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let secs = at % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, (secs % 3600) / 60)
}

const PAGE_CSS: &str = "body{font-family:sans-serif;max-width:42rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#4c4f69;background:#eff1f5}a{color:#8839ef}img{max-width:100%;height:auto;border-radius:8px}.post{padding:1rem 0;border-bottom:1px solid #ccd0da}.when{color:#6c6f85;font-size:.9rem}.files a{display:inline-block;margin-right:1rem}";

/// One post as a page of the home: `posts/<id>.html`, referring to the
/// bundle's files by root-relative paths.
#[uniffi::export]
pub fn feed_post_html(name: String, post: FeedPost) -> String {
    let blocks = text_blocks(&post.text);
    let mut h = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>\n<p><a href=\"/feed.html\">&larr; {}</a></p>\n<div class=\"post\"><p class=\"when\">{}{}</p>\n",
        esc(&name),
        PAGE_CSS,
        esc(&name),
        when(post.at),
        post.edited.map(|e| format!(" · edited {}", when(e))).unwrap_or_default()
    );
    h.push_str(&blocks_html(&blocks, "/"));
    for m in &post.media {
        if bundle_path_ok(&m.path) {
            h.push_str(&format!("<p><img src=\"/{}\" alt=\"{}\" width=\"{}\" height=\"{}\"></p>\n", esc(m.path.trim_start_matches('/')), esc(&m.alt), m.w, m.h));
            if let Some(f) = &m.full {
                if share_addr_ok(f) {
                    h.push_str(&format!("<p class=\"files\"><a href=\"{}\">full size</a></p>\n", esc(f)));
                }
            }
        }
    }
    if !post.files.is_empty() {
        h.push_str("<p class=\"files\">");
        for f in &post.files {
            if share_addr_ok(&f.addr) {
                h.push_str(&format!("<a href=\"{}\">{}</a> ", esc(&f.addr), esc(&f.name)));
            }
        }
        h.push_str("</p>\n");
    }
    h.push_str("</div>\n</body></html>\n");
    h
}

/// The feed as a page: `feed.html`, every post's first lines and a link
/// to its page.
#[uniffi::export]
pub fn feed_index_html(doc: FeedDoc) -> String {
    let mut h = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>\n<h1>{}</h1>\n",
        esc(&doc.name),
        PAGE_CSS,
        esc(&doc.name)
    );
    for p in &doc.posts {
        h.push_str(&format!("<div class=\"post\"><p class=\"when\"><a href=\"/posts/{}.html\">{}</a></p>\n", esc(&p.id), when(p.at)));
        h.push_str(&blocks_html(&text_blocks(&p.text), "/"));
        for m in p.media.iter().take(1) {
            if bundle_path_ok(&m.path) {
                h.push_str(&format!("<p><img src=\"/{}\" alt=\"{}\"></p>\n", esc(m.path.trim_start_matches('/')), esc(&m.alt)));
            }
        }
        h.push_str("</div>\n");
    }
    if let Some(o) = &doc.older {
        if bundle_path_ok(o) {
            h.push_str(&format!("<p><a href=\"/{}\">older</a></p>\n", esc(o.trim_start_matches('/'))));
        }
    }
    h.push_str("</body></html>\n");
    h
}

/// A fresh post id: 16 hex characters that no clock made.
#[uniffi::export]
pub fn feed_new_id() -> String {
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; 8];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona() -> String {
        "ab".repeat(32)
    }

    fn doc(posts: Vec<FeedPost>) -> FeedDoc {
        FeedDoc { v: 1, persona: persona(), name: "Kara".into(), updated: 1_700_000_000, posts, older: None }
    }

    fn post(id: &str, at: u64, text: &str) -> FeedPost {
        FeedPost { id: id.into(), at, edited: None, text: text.into(), media: vec![], files: vec![], re: None }
    }

    #[test]
    fn a_feed_round_trips_newest_first() {
        let d = doc(vec![post("0000000000000001", 10, "first"), post("0000000000000002", 20, "second")]);
        let json = feed_encode(d).unwrap();
        let back = feed_parse(json).unwrap();
        assert_eq!(back.posts[0].id, "0000000000000002");
        assert_eq!(back.posts[1].id, "0000000000000001");
    }

    #[test]
    fn the_reader_is_strict() {
        let good = feed_encode(doc(vec![post("0000000000000001", 10, "hi")])).unwrap();
        assert!(feed_parse(good.replace("\"v\": 1", "\"v\": 2")).is_err(), "version pinned");
        assert!(feed_parse(good.replace("\"name\"", "\"nickname\"")).is_err(), "unknown key refused");
        assert!(feed_parse(good.replace("\"updated\": 1700000000", "\"updated\": 1700000000, \"likes\": 3")).is_err(), "extra key refused");
        let mut d = doc(vec![post("0000000000000001", 10, "hi")]);
        d.persona = "not a key".into();
        assert!(feed_check(&d).is_err());
        let d = doc(vec![post("0000000000000001", 10, "older"), post("0000000000000002", 20, "newer")]);
        assert!(feed_check(&d).is_err(), "oldest first is refused");
        let d = doc(vec![post("0000000000000001", 10, "a"), post("0000000000000001", 5, "b")]);
        assert!(feed_check(&d).is_err(), "duplicate ids");
        let d = doc(vec![post("xyz", 10, "a")]);
        assert!(feed_check(&d).is_err(), "id shape");
    }

    #[test]
    fn nothing_reaches_past_the_room() {
        let d = doc(vec![post("0000000000000001", 10, "see [this](https://example.org/)")]);
        assert!(feed_check(&d).is_err());
        let d = doc(vec![post("0000000000000001", 10, "![x](//cdn.example/x.png)")]);
        assert!(feed_check(&d).is_err());
        let d = doc(vec![post("0000000000000001", 10, "![x](../secret.png)")]);
        assert!(feed_check(&d).is_err());
        let mut p = post("0000000000000001", 10, "a picture");
        p.media.push(FeedMedia { path: "feed/1/1.jpg".into(), full: Some("https://x/y.jpg".into()), mime: "image/jpeg".into(), bytes: 1, w: 1, h: 1, alt: String::new() });
        assert!(feed_check(&doc(vec![p])).is_err(), "full must be a share");
        let mut p = post("0000000000000001", 10, "a file");
        p.files.push(FeedFile { name: "notes.pdf".into(), addr: "ducat:file/VLD0:abc:def".into(), mime: "application/pdf".into(), bytes: 9 });
        assert!(feed_check(&doc(vec![p])).is_ok());
        let d = doc(vec![post("0000000000000001", 10, "inside [here](posts/x.html) and [there](ducat:site/VLD0:k) and ![pic](feed/1/1.jpg)")]);
        assert!(feed_check(&d).is_ok());
    }

    #[test]
    fn the_subset_and_nothing_else() {
        let b = text_blocks("Hello **world**, *soft* and [a link](ducat:card/x).\n\n![alt text](feed/1/1.jpg)\n\nplain *not italic * here");
        assert_eq!(b.len(), 3);
        match &b[0] {
            FeedBlock::Paragraph { spans } => {
                assert_eq!(spans[0].text, "Hello ");
                assert!(spans[1].bold && spans[1].text == "world");
                assert!(spans[3].italic && spans[3].text == "soft");
                assert_eq!(spans[5].link.as_deref(), Some("ducat:card/x"));
                assert_eq!(spans[5].text, "a link");
            }
            _ => panic!("paragraph"),
        }
        assert_eq!(b[1], FeedBlock::Image { path: "feed/1/1.jpg".into(), alt: "alt text".into() });
        match &b[2] {
            FeedBlock::Paragraph { spans } => assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), "plain *not italic * here"),
            _ => panic!("paragraph"),
        }
        // A line break inside a paragraph is kept as one.
        let b = text_blocks("line one\nline two");
        match &b[0] {
            FeedBlock::Paragraph { spans } => assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), "line one\nline two"),
            _ => panic!("paragraph"),
        }
        // Markup-looking text that is not markup stays text.
        let b = text_blocks("a [bracket] and (paren) and ** alone");
        match &b[0] {
            FeedBlock::Paragraph { spans } => assert!(spans.iter().all(|s| s.link.is_none())),
            _ => panic!("paragraph"),
        }
    }

    #[test]
    fn the_timeline_orders_and_breaks_ties_the_same_way_everywhere() {
        let a = FeedDoc { v: 1, persona: "aa".repeat(32), name: "A".into(), updated: 0, posts: vec![post("0000000000000001", 30, "a30"), post("0000000000000002", 10, "a10")], older: None };
        let b = FeedDoc { v: 1, persona: "bb".repeat(32), name: "B".into(), updated: 0, posts: vec![post("0000000000000003", 30, "b30"), post("0000000000000004", 20, "b20")], older: None };
        let t = feed_merge(vec![b, a], 10);
        let texts: Vec<&str> = t.iter().map(|e| e.post.text.as_str()).collect();
        assert_eq!(texts, ["a30", "b30", "b20", "a10"]);
        assert_eq!(feed_merge(vec![], 5).len(), 0);
        let t = feed_merge(vec![doc(vec![post("0000000000000001", 1, "x"), post("0000000000000000", 0, "y")])], 1);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn pages_escape_and_stay_inside() {
        let mut p = post("0000000000000001", 1_700_000_000, "<script>alert(1)</script> **bold** [go](posts/other.html) ![p](feed/1/1.jpg)");
        p.media.push(FeedMedia { path: "feed/1/1.jpg".into(), full: Some("ducat:file/VLD0:a:b".into()), mime: "image/jpeg".into(), bytes: 5, w: 10, h: 10, alt: "a \"pic\"".into() });
        let h = feed_post_html("Kara <3".into(), p.clone());
        assert!(h.contains("&lt;script&gt;"), "text is escaped");
        assert!(!h.contains("<script>"));
        assert!(h.contains("<strong>bold</strong>"));
        assert!(h.contains("href=\"/posts/other.html\""));
        assert!(h.contains("src=\"/feed/1/1.jpg\""));
        assert!(h.contains("alt=\"a &quot;pic&quot;\""));
        assert!(h.contains("href=\"ducat:file/VLD0:a:b\""));
        assert!(h.contains("2023-11-14 22:13 UTC"));
        assert!(!h.contains("http"), "nothing reaches the network: {h}");
        let idx = feed_index_html(doc(vec![p]));
        assert!(idx.contains("/posts/0000000000000001.html"));
        assert!(!idx.contains("http"));
    }

    #[test]
    fn ids_are_hex_and_unique_enough() {
        let a = feed_new_id();
        let b = feed_new_id();
        assert_eq!(a.len(), 16);
        assert!(is_lower_hex(&a));
        assert_ne!(a, b);
    }
}
