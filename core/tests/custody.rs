//! §4.4 — what each custody mode can and cannot do.

use ducat_core::custody::Custody::*;
use ducat_core::reject::RejectCode;
use ducat_core::state::{Role, SettleMode};

/// The whole point of refusing here rather than at FUND: a user who picked
/// hardware-only must find out before a counter, not after a customer has
/// committed to a payment that can never be funded.
#[test]
fn hardware_only_cannot_enter_escrow_from_either_side() {
    for role in [Role::Payer, Role::Payee] {
        assert_eq!(
            HardwareOnly.can_settle(SettleMode::Escrow, role).unwrap_err().code,
            RejectCode::PolicyRefused,
            "escrow needs a multisig share from both sides"
        );
    }
    for role in [Role::Payer, Role::Payee] {
        assert!(Software.can_settle(SettleMode::Escrow, role).is_ok());
        assert!(HardwareReserve.can_settle(SettleMode::Escrow, role).is_ok());
    }
}

/// Role matters for exactly one mode, and getting it wrong the simple way —
/// refusing both sides — would lock hardware users out of the flow they are most
/// suited to: paying a bonded merchant while holding no multisig share at all.
#[test]
fn a_hardware_only_payer_can_still_pay_a_bonded_merchant() {
    assert!(
        HardwareOnly.can_settle(SettleMode::Fast, Role::Payer).is_ok(),
        "under fast/1 the bond is the provider's; the payer holds no share"
    );
    assert_eq!(
        HardwareOnly.can_settle(SettleMode::Fast, Role::Payee).unwrap_err().code,
        RejectCode::PolicyRefused,
        "the provider side of fast/1 is bonded, and a bond is multisig"
    );
}

#[test]
fn direct_settlement_works_in_every_mode() {
    for c in [Software, HardwareReserve, HardwareOnly] {
        for role in [Role::Payer, Role::Payee] {
            assert!(
                c.can_settle(SettleMode::Direct, role).is_ok(),
                "{c:?} must be able to settle directly — it is the only mode with no \
                 external dependency and it must never be unavailable"
            );
        }
    }
}

#[test]
fn only_hardware_only_is_barred_from_bonding() {
    assert!(Software.can_post_bond().is_ok());
    assert!(HardwareReserve.can_post_bond().is_ok());
    assert_eq!(
        HardwareOnly.can_post_bond().unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

/// A USB-OTG round trip and a button press is not a slow tap, it is a different
/// interaction. Offering it at a counter produces the worst outcome available.
#[test]
fn a_hardware_only_wallet_does_not_offer_taps() {
    assert!(Software.tap_capable());
    assert!(HardwareReserve.tap_capable());
    assert!(!HardwareOnly.tap_capable());
}

/// §4.3.4 warns that a backup file is a complete spending credential. Behind a
/// reserve that sentence is false, and a client showing the same warning in
/// every mode is lying in one of them.
#[test]
fn a_reserve_bounds_what_a_leaked_backup_is_worth() {
    assert!(Software.backup_exposes_reserve());
    assert!(!HardwareReserve.backup_exposes_reserve());
    assert!(!HardwareOnly.backup_exposes_reserve());
}

/// The constraint is external and dated. If Monero ships multisig on hardware
/// this flips and every rule above follows automatically — which is why they are
/// all expressed through `can_multisig` rather than matched on the mode.
#[test]
fn every_refusal_traces_to_the_multisig_limit() {
    assert!(Software.can_multisig());
    assert!(HardwareReserve.can_multisig());
    assert!(!HardwareOnly.can_multisig());
}
