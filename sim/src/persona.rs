//! A simulated DUCAT participant.
//!
//! Each persona holds real keys and drives the real state machine from
//! `ducat-core`. Nothing here fakes protocol behaviour: messages are signed,
//! verified over bytes as received, and the transcript each transaction
//! produces is the same object a client would hold.

use ducat_core::sig::{PublicKey, SecretKey};
use ducat_core::state::{Role, SettleMode, State};

/// What a participant does in the market. This is simulation scaffolding, not
/// a protocol concept — the protocol knows only payer and payee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Consumer,
    Taxi,
    Coffee,
    Shopkeeper,
}

impl Kind {
    /// Profile id this participant presents under (§7).
    pub fn profile(self) -> u64 {
        match self {
            Kind::Consumer => 1,   // xfer/1
            Kind::Taxi => 3,       // ride/1
            Kind::Coffee => 2,     // pos/1
            Kind::Shopkeeper => 2, // pos/1
        }
    }
}

pub struct Persona {
    pub name: String,
    pub kind: Kind,
    /// Long-lived identity that accrues reputation (§4).
    pub persona_key: SecretKey,
    /// Monero address funds are received at. Fresh subaddresses per tap are
    /// mandatory in production (§15.10); the simulator uses one address per
    /// participant and says so rather than pretending otherwise.
    pub payto: Vec<u8>,
    /// Transaction state, per §18.4.
    pub state: State,
    /// Transcripts of completed transactions — the only record they produce.
    pub receipts: Vec<CompletedTransaction>,
    pub balance_pxmr: u64,
}

#[derive(Debug, Clone)]
pub struct CompletedTransaction {
    pub counterparty: String,
    pub profile: u64,
    pub amount_pxmr: u64,
    pub paid: bool,
    /// Canonical bytes of the four objects, in order. A client would persist
    /// exactly this (§7.4) — the receipt alone proves nothing.
    pub transcript: Vec<Vec<u8>>,
}

impl Persona {
    pub fn new(name: &str, kind: Kind, seed: u8, payto: Vec<u8>) -> Self {
        Persona {
            name: name.to_string(),
            kind,
            persona_key: SecretKey::ed25519_from_bytes(&[seed; 32]),
            payto,
            state: State::Idle,
            receipts: Vec::new(),
            balance_pxmr: 0,
        }
    }

    pub fn public(&self) -> PublicKey {
        self.persona_key.public()
    }

    /// Advance this participant's state machine, surfacing refusals rather
    /// than swallowing them — §18.4 requires an unexpected message be rejected,
    /// and a simulator that ignores that is not simulating the protocol.
    pub fn step(
        &mut self,
        role: Role,
        mode: SettleMode,
        event: &ducat_core::state::Event,
    ) -> Result<(), String> {
        match ducat_core::state::transition(self.state, role, mode, event) {
            Ok(t) => {
                self.state = t.next;
                Ok(())
            }
            Err(e) => Err(format!("{} refused {:?}: {:?}", self.name, event, e.code)),
        }
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}
