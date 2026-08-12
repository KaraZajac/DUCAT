//! DUCAT protocol core.
//!
//! Wire format and contract logic, with no I/O and no platform dependencies, so
//! that it is testable in isolation and reusable by any client. Corresponds to
//! Part V (§18) of the protocol document.

pub mod backup;
pub mod cbor;
pub mod custody;
pub mod commit;
pub mod negotiate;
pub mod reject;
pub mod sig;
pub mod state;
pub mod wire;
pub mod verify;
