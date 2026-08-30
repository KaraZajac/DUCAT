//! Networking abstractions built on Veilid API primitives.
//!
//! veilnet provides high-level networking capabilities using the Veilid distributed hash table
//! and private routing system. It enables applications to communicate over the Veilid network
//! without dealing directly with low-level Veilid APIs.
//!
//! # Examples
//!
//! Creating a connection and setting up datagram communication:
//!
//! ```no_run
//! use veilnet::{
//!     connection::Veilid,
//!     datagram::{Listener, Dialer}
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let conn = Veilid::new().await?;
//! let listener = Listener::new(conn, None, 8080).await?;
//! let addr = listener.addr().clone();
//!
//! let conn2 = Veilid::new().await?;
//! let mut dialer = Dialer::new(conn2).await?;
//! let (route_id, _) = dialer.resolve(&addr).await?;
//! dialer.send_to(route_id, b"Hello, world!".to_vec()).await?;
//! # Ok(())
//! # }
//! ```

#![recursion_limit = "256"]

/// Connection management and Veilid network interface.
pub mod connection;
/// Datagram-style communication over Veilid private routes.
pub mod datagram;
pub mod proto;
mod types;

/// A connection to the Veilid network.
pub use connection::Connection;
/// An address for DHT-based communication in the Veilid network.
pub use types::DHTAddr;
