//! Where an identity lives on this machine.

use std::path::PathBuf;

/// `DUCAT_DESK_STATE` names the identity — two desks on one machine are two
/// directories, each a complete persona, wallet and set of contacts — and
/// otherwise the platform's data directory holds one under `ducat`.
///
/// `ducat`, not the Compose desk's `ducat-desk`: the stores here are not
/// that client's on-disk format, and sharing a directory would have each
/// process corrupting the other's tables. A migration is a deliberate step
/// with its own code, not an accident of a shared path.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("DUCAT_DESK_STATE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    platform_data_home().join("ducat")
}

fn platform_data_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = std::env::var_os("APPDATA") {
            return PathBuf::from(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Library/Application Support");
        }
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share")
}
