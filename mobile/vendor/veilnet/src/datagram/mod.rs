//! Datagram-style communication over Veilid private routes.
//!
//! This module provides abstractions for UDP-like communication patterns using Veilid's
//! private routing system. Messages are sent over encrypted private routes through the
//! Veilid network, providing anonymity and censorship resistance.
//!
//! # Components
//!
//! - [`crate::datagram::listener::Listener`] - Binds to a DHT address to receive incoming messages
//! - [`crate::datagram::dialer::Dialer`] - Connects to DHT addresses to send outgoing messages
//!
//! # Example
//!
//! ```no_run
//! use veilnet::{
//!     connection::Veilid,
//!     datagram::{Listener, Dialer},
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Set up a listener
//! let conn = Veilid::new().await?;
//! let listener = Listener::new(conn, None, 8080).await?;
//! let addr = listener.addr().clone();
//!
//! // Set up a dialer
//! let conn2 = Veilid::new().await?;
//! let mut dialer = Dialer::new(conn2).await?;
//!
//! // Send a message
//! let (route_id, _) = dialer.resolve(&addr).await?;
//! dialer.send_to(route_id, b"Hello, world!".to_vec()).await?;
//! # Ok(())
//! # }
//! ```

/// Client for sending messages to DHT addresses.
pub mod dialer;
/// Server for receiving messages at DHT addresses.
pub mod listener;

pub mod socket;

pub use dialer::Dialer;
pub use listener::Listener;
