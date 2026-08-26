//! DUCAT protocol core.
//!
//! Wire format and contract logic, with no I/O and no platform dependencies, so
//! that it is testable in isolation and reusable by any client. Corresponds to
//! Part V (§18) of the protocol document.

pub mod backup;
pub mod bond;
pub mod burning;
pub mod cbor;
pub mod contact;
pub mod geo;
pub mod hpke;
pub mod custody;
pub mod escrow;
pub mod float;
pub mod commit;
pub mod negotiate;
pub mod position;
pub mod reject;
pub mod sig;
pub mod state;
pub mod board;
pub mod wire;
pub mod transport;
pub mod verify;
