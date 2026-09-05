//! DUCAT's application logic, in Rust, for every client.
//!
//! The protocol lives in `ducat-core`; the node, the swarm and the wallet in
//! `ducat-mobile`. What sits between those and a screen — the stores, the
//! publishing flows, the mailbox's send and poll, the lap that keeps a node
//! honest — has lived in Kotlin, shared with the desk by compiling the
//! phone's files against a shim. This crate is that layer moving down a
//! level so a desk that is not a JVM can have it too, and so there is one
//! of it rather than two.
//!
//! Everything here takes an [`App`], which is a data directory and nothing
//! more: where the stores live, where bundles are cached, where the node
//! keeps its keys. Two desks on one machine are two directories.

pub mod attachments;
pub mod backup;
pub mod boards;
pub mod catalogue;
pub mod contacts;
pub mod donations;
pub mod groups;
pub mod identity;
pub mod lap;
pub mod ledger;
pub mod listings;
pub mod log;
pub mod mailbox;
pub mod opinion;
pub mod paths;
pub mod pay;
pub mod publications;
pub mod recurring;
pub mod releases;
pub mod sites;
pub mod store;
pub mod tabs;
pub mod thumbs;
pub mod wallet;

use std::path::{Path, PathBuf};

/// One identity's home on disk.
#[derive(Clone, Debug)]
pub struct App {
    root: PathBuf,
}

impl App {
    /// Open (creating if needed) the app rooted at `root`.
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<App> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("files"))?;
        std::fs::create_dir_all(root.join("prefs"))?;
        log::init(&root);
        Ok(App { root })
    }

    /// The default location for this user, honouring the same environment
    /// the Compose desk did so `DUCAT_DESK_STATE` still names an identity.
    pub fn open_default() -> std::io::Result<App> {
        let root = paths::data_dir();
        // The previous desk kept its state one directory over. A fresh
        // start here with that directory present adopts it — copied, so
        // the old desk keeps working — and the identity, contacts and
        // wallet carry across instead of being minted twice.
        if !root.join("prefs").exists() {
            if let Some(old) = paths::previous_desk_dir().filter(|d| d.join("prefs").exists()) {
                if let Err(e) = copy_tree(&old, &root) {
                    log::warn("App", format!("could not adopt {}: {e}", old.display()));
                } else {
                    log::init(&root);
                    log::info("App", format!("adopted the previous desk's state from {}", old.display()));
                }
            }
        }
        App::open(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where bundles, releases and every other cached file lives —
    /// `files/`, matching the phone's `filesDir`, so paths documented for
    /// one hold for the other.
    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    /// The node's own directory: keyring, table store, the `full_node`
    /// marker.
    pub fn node_dir(&self) -> PathBuf {
        self.root.join("veilid")
    }

    /// A store by name, backed by `prefs/<name>.json`.
    pub fn store(&self, name: &str) -> store::Store {
        store::Store::new(self.root.join("prefs").join(format!("{name}.json")))
    }

    /// Start the node under this app's directory, if it is not running.
    /// Returns once the call is made; readiness is a status poll away.
    pub fn start_node(&self) -> Result<(), Error> {
        ducat_mobile::node::node_start(self.node_dir().to_string_lossy().into_owned(), true)
            .map_err(|e| Error::Node(format!("{e:?}")))
    }

    pub fn node_status(&self) -> ducat_mobile::node::NodeStatus {
        ducat_mobile::node::node_status()
    }

    /// Unix seconds now.
    pub fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// What the app layer can refuse or fail at.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The caller asked for something the rules do not allow — a page
    /// that reaches the clearnet, an address that is not one.
    #[error("{0}")]
    Refused(String),
    /// The node said no.
    #[error("node: {0}")]
    Node(String),
    /// The swarm said no.
    #[error("swarm: {0}")]
    Swarm(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("store: {0}")]
    Store(#[from] serde_json::Error),
    /// A card that cannot be claimed, typed so a screen can say why in
    /// the reader's language instead of repeating an English sentence.
    #[error("card: {0:?}")]
    Card(CardProblem),
}

/// Why a card was not claimed. Each is a different screen: a spent card
/// wants a fresh one, an own card is the listing being yours, a card whose
/// details are not up yet wants a second scan in a minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CardProblem {
    /// Its one reply slot is already written (§7.5's claim-once).
    AlreadyUsed,
    /// The card is this desk's own.
    Own,
    /// The record exists but subkey 0 has not arrived yet.
    NotPublished,
    /// Past its expiry.
    Expired,
}

impl From<ducat_mobile::node::NodeError> for Error {
    fn from(e: ducat_mobile::node::NodeError) -> Self {
        Error::Node(format!("{e:?}"))
    }
}

impl From<ducat_mobile::swarm::SwarmError> for Error {
    fn from(e: ducat_mobile::swarm::SwarmError) -> Self {
        Error::Swarm(format!("{e:?}"))
    }
}

impl From<ducat_mobile::contacts::ContactError> for Error {
    fn from(e: ducat_mobile::contacts::ContactError) -> Self {
        Error::Refused(format!("{e:?}"))
    }
}

/// Total bytes of every regular file under `dir`, recursively.
pub(crate) fn dir_bytes(dir: &Path) -> u64 {
    fn walk(p: &Path, acc: &mut u64) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, acc);
                } else if let Ok(m) = path.metadata() {
                    *acc += m.len();
                }
            }
        }
    }
    let mut n = 0;
    walk(dir, &mut n);
    n
}

/// Does `dir` hold at least one regular file, anywhere below it?
pub(crate) fn has_any_file(dir: &Path) -> bool {
    fn walk(p: &Path) -> bool {
        match std::fs::read_dir(p) {
            Ok(rd) => rd.flatten().any(|e| {
                let path = e.path();
                if path.is_dir() { walk(&path) } else { path.is_file() }
            }),
            Err(_) => false,
        }
    }
    dir.is_dir() && walk(dir)
}

/// Copy a directory tree. `dst` is created; existing files are replaced.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
