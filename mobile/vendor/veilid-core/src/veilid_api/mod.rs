/// Android-specific JNI setup for registering the host app's JavaVM and Context.
#[cfg(target_os = "android")]
pub mod android;
mod api;
#[cfg(feature = "debug-api")]
mod debug;
#[cfg(not(feature = "debug-api"))]
mod debug_stub;
mod dht_transaction;
mod error;
mod routing_context;
mod serialize_helpers;
mod types;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_veilid_api;

pub use api::*;
#[cfg(feature = "unstable-blockstore")]
pub use block_store::*;
pub use crypto::*;
pub use dht_transaction::*;
pub use error::*;
pub use protected_store::*;
pub use routing_context::*;
pub use serialize_helpers::*;
pub use table_store::{TableDB, TableDBTransaction, TableStore};
pub use types::*;

use crate::*;

use core_context::{api_shutdown, VeilidCoreContext};
use routing_table::{AllocateRouteParams, DirectionSet, RouteIdAndKeys, RouteSpecStore};
#[cfg(feature = "debug-api")]
use rpc_processor::*;

/////////////////////////////////////////////////////////////////////////////////////////////////////
