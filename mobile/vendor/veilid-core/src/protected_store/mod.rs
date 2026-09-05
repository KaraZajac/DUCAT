use super::*;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use wasm::*;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use native::*;

/// Protected store keys that Veilid creates and manages, removed together by `delete_all`.
pub static KNOWN_PROTECTED_STORE_KEYS: [&str; 2] = ["device_encryption_key", "_test_key"];

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_protected_store;
