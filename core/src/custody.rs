//! Where the spend key lives, and what that costs (§4.4).
//!
//! Two very different things get called "hardware keys" and they behave
//! oppositely:
//!
//! - A **secure element** (Secure Enclave, StrongBox) holds a key that cannot be
//!   extracted, and therefore cannot be backed up. It dies with the phone. This
//!   is what made §4.1's original persona rule unrecoverable.
//! - An **external hardware wallet** (Ledger, Trezor) is a device the user owns
//!   separately. It survives the phone, and it carries its own seed backup — so
//!   it does not have a backup problem at all. It *is* the backup.
//!
//! Supporting the second is worth doing, and it changes something the backup
//! module could not fix on its own: §4.3.4 has to warn that a backup file is a
//! complete spending credential. Behind a hardware reserve, the bundle is only
//! worth the float.
//!
//! # What a Monero hardware wallet does not do
//!
//! **It cannot hold the persona key.** A Ledger or Trezor running the Monero app
//! signs Monero transactions; it does not produce Ed25519 or P-256 signatures
//! over DUCAT's domain-separated objects (§18.3). So no custody mode moves the
//! persona off the phone, and §4.3's export/import remains the only answer for
//! identity in every mode. The choice here is about money, not about who you are.
//!
//! **It cannot do multisig.** Monero multisig on hardware is roadmap, not
//! shipped. Escrow (§8.2) and bonds (§17.2) are multisig, so a device-held spend
//! key cannot enter either — which is why this module refuses those combinations
//! up front rather than letting a user discover it at a counter.

use crate::reject::{Reject, RejectCode};
use crate::state::{Role, SettleMode};

/// Where the user's spendable Monero key lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Custody {
    /// Everything on the phone, protected by §4.3's backup and §15.5.1's
    /// verification tiers. The only mode with no external dependency, and the
    /// only one where the backup file is worth the whole balance.
    Software,
    /// A software hot wallet holding the float (§17.2), topped up from a
    /// reserve on an external device.
    ///
    /// The recommended shape, and the reason is not defence in depth for its own
    /// sake: it makes the amount at risk a number the user chose. A stolen phone,
    /// a leaked backup, and a compromised client all cost the float rather than
    /// the balance, and the reserve's own seed phrase — which DUCAT never sees —
    /// covers the rest.
    HardwareReserve,
    /// The spend key lives only on the external device.
    ///
    /// Maximum protection of funds, and it gives up two things that matter:
    /// multisig, so no escrow and no bond, and tap latency, since a USB-OTG
    /// round trip plus an on-device confirmation does not fit §15's budget.
    HardwareOnly,
}

impl Custody {
    /// Whether this mode can hold a Monero multisig share.
    ///
    /// False for `HardwareOnly` for an external reason, not a design choice:
    /// Monero multisig is not implemented on Ledger or Trezor. If that ships,
    /// this becomes true and nothing else here changes.
    pub fn can_multisig(&self) -> bool {
        !matches!(self, Custody::HardwareOnly)
    }

    /// Whether a tap can complete inside §15's budget.
    ///
    /// A hardware wallet needs a physical connection and a button press. That is
    /// not a slow tap, it is a different interaction, and offering it at a
    /// counter produces the worst outcome available — a queue, a stalled
    /// terminal, and a payment that fails after the customer has committed to
    /// it. Better to know before presenting.
    pub fn tap_capable(&self) -> bool {
        !matches!(self, Custody::HardwareOnly)
    }

    /// Whether this mode can take `role` in a transaction settling under `mode`.
    ///
    /// Role matters, and only for `Fast`: §17's bond is posted by the
    /// *provider*, so a payer under `fast/1` holds no multisig share and a
    /// hardware-only consumer can pay a bonded merchant perfectly well. Refusing
    /// them both would be simpler and wrong.
    pub fn can_settle(&self, mode: SettleMode, role: Role) -> Result<(), Reject> {
        let needs_multisig = match mode {
            SettleMode::Direct => false,
            SettleMode::Escrow => true,
            SettleMode::Fast => role == Role::Payee,
        };
        if needs_multisig && !self.can_multisig() {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "a device-held spend key cannot hold a multisig share: \
                 Monero multisig is not implemented on hardware wallets",
            ));
        }
        Ok(())
    }

    /// Whether this mode can post a bond (§17.2). Bonds are 2-of-3 multisig.
    pub fn can_post_bond(&self) -> Result<(), Reject> {
        if !self.can_multisig() {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "a bond is a multisig deposit and cannot be posted from a hardware wallet",
            ));
        }
        Ok(())
    }

    /// What a leaked backup file is worth in this mode.
    ///
    /// Not decoration. §4.3.4 must tell the user that a backup is a complete
    /// spending credential, and under `HardwareReserve` that sentence is simply
    /// false — the bundle holds the hot wallet's seed and the reserve is behind
    /// a device seed DUCAT never sees. A client that shows the same warning in
    /// every mode is lying in one of them.
    pub fn backup_exposes_reserve(&self) -> bool {
        matches!(self, Custody::Software)
    }
}
