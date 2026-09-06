//! The listing bundle's document (§16.18.3): `listing.json` at the root of
//! the share a notice names, beside the pictures. A closed set like the
//! feed's; refused whole on anything it does not name, so a bad document
//! never hides good pictures — the reader shows those alone.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::feed::{bundle_path_ok, mime_ok, target_ok, text_blocks, FeedBlock};

pub const LISTING_VERSION: u64 = 1;
pub const LISTING_FILE: &str = "listing.json";
pub const MAX_DESCRIPTION: usize = 8000;
pub const MAX_TITLE: usize = 120;
pub const MAX_PICTURES: usize = 24;
pub const MAX_FILES: usize = 8;
pub const MAX_SPECS: usize = 32;
pub const MAX_CAPTION: usize = 300;
pub const MAX_SPEC_CHARS: usize = 120;
pub const MAX_NAME: usize = 200;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ListingDocError {
    #[error("listing: {0}")]
    Refused(String),
}

fn refuse<T>(msg: impl Into<String>) -> Result<T, ListingDocError> {
    Err(ListingDocError::Refused(msg.into()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct ListingPicture {
    pub path: String,
    pub mime: String,
    pub bytes: u64,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub caption: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct ListingFile {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct ListingDoc {
    pub v: u64,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub updated: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<ListingPicture>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ListingFile>,
    /// A map for the bridge's sake; written sorted, so two writes of the
    /// same document are the same bytes.
    #[serde(default, skip_serializing_if = "HashMap::is_empty", serialize_with = "sorted")]
    pub specs: HashMap<String, String>,
}

fn sorted<S: serde::Serializer>(m: &HashMap<String, String>, ser: S) -> Result<S::Ok, S::Error> {
    let ordered: std::collections::BTreeMap<&String, &String> = m.iter().collect();
    serde::Serialize::serialize(&ordered, ser)
}

fn id_ok(id: &str) -> bool {
    // A listing id is what its client minted: the desk's `<hex>-<hex>-<n>`
    // or the phone's hex. Printable, short, no path characters.
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn listing_doc_check(doc: &ListingDoc) -> Result<(), ListingDocError> {
    if doc.v != LISTING_VERSION {
        return refuse(format!("version {} is not {}", doc.v, LISTING_VERSION));
    }
    if !id_ok(&doc.id) {
        return refuse("id is not a listing id");
    }
    if doc.title.trim().is_empty() || doc.title.chars().count() > MAX_TITLE {
        return refuse("title is empty or too long");
    }
    if doc.description.chars().count() > MAX_DESCRIPTION {
        return refuse(format!("description longer than {MAX_DESCRIPTION} characters"));
    }
    if doc.pictures.len() > MAX_PICTURES {
        return refuse(format!("more than {MAX_PICTURES} pictures"));
    }
    if doc.files.len() > MAX_FILES {
        return refuse(format!("more than {MAX_FILES} files"));
    }
    if doc.specs.len() > MAX_SPECS {
        return refuse(format!("more than {MAX_SPECS} specs"));
    }
    let mut seen = std::collections::HashSet::new();
    for p in &doc.pictures {
        if !bundle_path_ok(&p.path) {
            return refuse(format!("picture path {:?} is not inside the bundle", p.path));
        }
        if !seen.insert(p.path.clone()) {
            return refuse(format!("picture path {:?} appears twice", p.path));
        }
        if !p.mime.starts_with("image/") || !mime_ok(&p.mime) {
            return refuse(format!("picture mime {:?}", p.mime));
        }
        if p.caption.chars().count() > MAX_CAPTION {
            return refuse("caption too long");
        }
    }
    for f in &doc.files {
        if !bundle_path_ok(&f.path) {
            return refuse(format!("file path {:?} is not inside the bundle", f.path));
        }
        if !seen.insert(f.path.clone()) {
            return refuse(format!("path {:?} appears twice", f.path));
        }
        if f.name.is_empty() || f.name.chars().count() > MAX_NAME || f.name.contains('/') || f.name.contains('\\') {
            return refuse(format!("file name {:?}", f.name));
        }
        if !mime_ok(&f.mime) {
            return refuse(format!("file mime {:?}", f.mime));
        }
    }
    for (k, v) in &doc.specs {
        if k.trim().is_empty() || k.chars().count() > MAX_SPEC_CHARS || v.chars().count() > MAX_SPEC_CHARS {
            return refuse("a spec key or value is empty or too long");
        }
    }
    // Every target in the description, by the same rule as the fields.
    for b in text_blocks(&doc.description) {
        match b {
            FeedBlock::Image { path, .. } => {
                if !bundle_path_ok(&path) {
                    return refuse(format!("description image {:?} is not inside the bundle", path));
                }
            }
            FeedBlock::Paragraph { spans } => {
                for s in spans {
                    if let Some(l) = &s.link {
                        if !target_ok(l) {
                            return refuse(format!("description links past the room: {:?}", l));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[uniffi::export]
pub fn listing_doc_parse(json: String) -> Result<ListingDoc, ListingDocError> {
    if json.len() > 1024 * 1024 {
        return refuse("listing document larger than 1 MiB");
    }
    let doc: ListingDoc =
        serde_json::from_str(&json).map_err(|e| ListingDocError::Refused(format!("not a listing document: {e}")))?;
    listing_doc_check(&doc)?;
    Ok(doc)
}

#[uniffi::export]
pub fn listing_doc_encode(doc: ListingDoc) -> Result<String, ListingDocError> {
    listing_doc_check(&doc)?;
    serde_json::to_string_pretty(&doc).map_err(|e| ListingDocError::Refused(e.to_string()))
}

/// The description as blocks for a screen: §16.23's text subset, the
/// same parser the feed uses, so a listing reads like a post.
#[uniffi::export]
pub fn listing_doc_blocks(description: String) -> Vec<FeedBlock> {
    text_blocks(&description)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> ListingDoc {
        ListingDoc {
            v: 1,
            id: "1a077560cf4-1ab8c8-2".into(),
            title: "Brass desk lamp".into(),
            description: "A **brass** lamp. See ![the base](pictures/00.jpg).".into(),
            updated: 1_800_000_000,
            pictures: vec![ListingPicture { path: "pictures/00.jpg".into(), mime: "image/jpeg".into(), bytes: 36108, w: 1400, h: 1000, caption: "".into() }],
            files: vec![ListingFile { path: "files/manual.pdf".into(), name: "manual.pdf".into(), mime: "application/pdf".into(), bytes: 100 }],
            specs: [("make".to_string(), "Anglepoise".to_string())].into_iter().collect(),
        }
    }

    #[test]
    fn round_trips() {
        let json = listing_doc_encode(doc()).unwrap();
        assert_eq!(listing_doc_parse(json).unwrap(), doc());
    }

    #[test]
    fn a_key_nobody_named_is_refused_whole() {
        let mut json = listing_doc_encode(doc()).unwrap();
        json = json.replacen("\"v\": 1", "\"v\": 1, \"price\": 3", 1);
        assert!(listing_doc_parse(json).is_err());
    }

    #[test]
    fn paths_stay_inside_the_bundle_and_links_inside_the_room() {
        let mut d = doc();
        d.pictures[0].path = "../secret.jpg".into();
        assert!(listing_doc_check(&d).is_err());
        let mut d = doc();
        d.description = "See [this](https://example.com)".into();
        assert!(listing_doc_check(&d).is_err());
        let mut d = doc();
        d.description = "See [our home](ducat:site/VLD0:abc)".into();
        assert!(listing_doc_check(&d).is_ok());
    }

    #[test]
    fn limits_hold() {
        let mut d = doc();
        d.description = "x".repeat(MAX_DESCRIPTION + 1);
        assert!(listing_doc_check(&d).is_err());
        let mut d = doc();
        d.pictures = (0..MAX_PICTURES + 1).map(|i| ListingPicture { path: format!("pictures/{i:02}.jpg"), mime: "image/jpeg".into(), bytes: 1, w: 1, h: 1, caption: "".into() }).collect();
        assert!(listing_doc_check(&d).is_err());
        let mut d = doc();
        d.files[0].mime = "image/jpeg".into();
        assert!(listing_doc_check(&d).is_ok());
        d.pictures[0].mime = "application/pdf".into();
        assert!(listing_doc_check(&d).is_err());
    }
}
