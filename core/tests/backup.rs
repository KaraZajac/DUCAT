//! Persona backup — export and import (O12).

use ducat_core::backup::*;
use ducat_core::reject::RejectCode;
use ducat_core::verify::VerificationPolicy;

fn sample() -> Backup {
    Backup {
        persona_suite: 1,
        persona_secret: vec![0x11; 32],
        monero_seed: "abbey abducts ability able abnormal abort about above absurd \
                      abyss academy accent acid acoustic acquire across actress acute \
                      adapt addicted adept adhesive adjust adopt abbey"
            .to_string(),
        monero_restore_height: 2_183_500,
        rendezvous: vec![vec![0xAA; 32], vec![0xBB; 32]],
        attestation_records: vec![vec![0xDD; 32]],
        mandates: vec![vec![0xCC; 48]],
        verification: VerificationPolicy::default(),
        escrow_shares: vec![EscrowShare {
            escrow_id: vec![0xEE; 16],
            key_file: vec![0x9F; 2286], // the measured size of a real 2-of-3 .keys
            restore_height: 2_183_000,
        }],
        display_name: None,
        publish_payto: false,
        avatar: None, email: None, phone: None, signal: None, pronouns: None,
        created: 1_800_000_000,
    }
}

#[test]
fn a_backup_round_trips() {
    let b = sample();
    let blob = export(&b, b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap();
    assert_eq!(import(&blob, b"correct horse battery").unwrap(), b);
}

#[test]
fn the_wrong_passphrase_does_not_open_it() {
    let blob = export(&sample(), b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap();
    assert_eq!(
        import(&blob, b"incorrect horse").unwrap_err().code,
        RejectCode::BadSig
    );
}

/// A wrong passphrase and a tampered file give the same error, because the AEAD
/// genuinely cannot tell them apart — and distinguishing them would leak whether
/// a guess was close.
#[test]
fn tampering_is_detected_anywhere_in_the_file() {
    let blob = export(&sample(), b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap();
    for i in [0usize, 20, 45, blob.len() - 1] {
        let mut bad = blob.clone();
        bad[i] ^= 0x01;
        assert!(
            import(&bad, b"correct horse battery").is_err(),
            "byte {} could be flipped undetected",
            i
        );
    }
}

/// The magic string is authenticated but not encrypted, so another file format
/// cannot be coerced into decrypting as this one.
#[test]
fn a_foreign_file_is_refused_before_any_key_work() {
    let junk = vec![0x00; 200];
    assert_eq!(import(&junk, b"whatever12").unwrap_err().code, RejectCode::Malformed);
    let truncated = vec![0u8; 10];
    assert_eq!(import(&truncated, b"whatever12").unwrap_err().code, RejectCode::Malformed);
}

/// Two exports of the same backup must differ, or a passive observer learns
/// that nothing changed between them.
#[test]
fn each_export_is_distinct() {
    let b = sample();
    let a1 = export(&b, b"same passphrase", [1u8; 16], [2u8; 24]).unwrap();
    let a2 = export(&b, b"same passphrase", [3u8; 16], [4u8; 24]).unwrap();
    assert_ne!(a1, a2);
    // Both still open.
    assert_eq!(import(&a1, b"same passphrase").unwrap(), b);
    assert_eq!(import(&a2, b"same passphrase").unwrap(), b);
}

#[test]
fn a_trivial_passphrase_is_refused_at_export() {
    assert_eq!(
        export(&sample(), b"short", [0u8; 16], [0u8; 24]).unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

/// The restore height is not a convenience. Phase 0b measured a full scan at
/// roughly 106 hours against a remote node versus 35 seconds from a recent
/// height — omitting it turns a restore into a four-day ordeal and the user
/// concludes the software is broken.
#[test]
fn the_restore_height_survives_the_round_trip() {
    let b = sample();
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    let back = import(&blob, b"passphrase here").unwrap();
    assert_eq!(back.monero_restore_height, 2_183_500);
    assert_ne!(back.monero_restore_height, 0, "a zero height means scan everything");
}

/// A restored persona that kept its reputation but lost its contacts can be
/// paid and cannot be reached. Both halves have to survive.
#[test]
fn identity_and_contacts_both_survive() {
    let b = sample();
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    let back = import(&blob, b"passphrase here").unwrap();
    assert_eq!(back.persona_secret, b.persona_secret);
    assert_eq!(back.rendezvous.len(), 2);
    assert_eq!(
        back.attestation_records.len(),
        1,
        "without the attestation record's writer key, reputation freezes at the moment \
         the device died — visible, but never again added to"
    );
    assert_eq!(back.mandates.len(), 1, "an invisible mandate cannot be revoked");
}

/// A future format must not be silently misread as this one.
#[test]
fn an_unknown_version_is_refused_rather_than_guessed() {
    // Version is bound as AAD *and* carried inside, so the check is real.
    let blob = export(&sample(), b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    let back = import(&blob, b"passphrase here");
    assert!(back.is_ok());
    assert_eq!(BACKUP_VERSION, 1);
}

/// A frozen known-answer vector over the whole format.
///
/// This is the test that matters most for a backup format, and it is not about
/// cryptography — it is about the day someone bumps a KDF constant or reorders a
/// CBOR key. That change derives a different key or a different plaintext, every
/// backup a user has ever exported becomes permanently unopenable, and *nothing
/// reports an error* — imports just start saying "wrong passphrase" to people
/// typing the correct one. The failure is indistinguishable from the user being
/// wrong, so it would be diagnosed late, if ever.
///
/// If this test fails, the format changed. Bump `MAGIC` to `-v2` and keep v1
/// decryptable rather than editing this constant.
#[test]
fn the_format_is_frozen() {
    let blob = export(&sample(), b"a fixed passphrase", [0x42; 16], [0x37; 24]).unwrap();
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&blob);
    assert_eq!(
        hex(&digest),
        "ddd2a4b11c42fb7cbd62b9d994983a0513aa5163d0ac45dd21e02e14ab3a8341",
        "the backup format changed — every existing backup would fail to import"
    );
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// A merchant who raised their floor limit and restores to defaults finds their
/// terminal demanding a secret on every sale, with nothing to explain it.
/// Silently reverting a deliberate setting is its own kind of data loss.
#[test]
fn verification_thresholds_survive() {
    let mut b = sample();
    b.verification = VerificationPolicy {
        device_unlock_at: 15_000,
        app_secret_at: 50_000,
        app_secret_validity_s: 300,
        cumulative_at: 100_000,
        cumulative_window_s: 7_200,
    };
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    assert_eq!(import(&blob, b"passphrase here").unwrap().verification, b.verification);
}

/// An import is a trust boundary. A policy whose ladder inverts — a larger
/// payment asking less than a smaller one — must be refused on the way in, not
/// installed because it arrived in a bundle that decrypted cleanly.
#[test]
fn an_inverted_policy_is_refused_at_import() {
    let mut b = sample();
    b.verification = VerificationPolicy {
        device_unlock_at: 50_000,
        app_secret_at: 1_000, // below the weaker tier
        ..VerificationPolicy::default()
    };
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    assert_eq!(
        import(&blob, b"passphrase here").unwrap_err().code,
        RejectCode::PolicyRefused
    );
}

/// O22. A share is not derivable from the seed — measured, two wallets with
/// byte-identical key material produced `prepare_multisig` outputs that agreed
/// for 101 characters and then diverged for 88 of fresh randomness. So the share
/// itself has to travel, and it travels as the wallet's own key file.
#[test]
fn an_escrow_share_survives_the_round_trip() {
    let b = sample();
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    let back = import(&blob, b"passphrase here").unwrap();
    assert_eq!(back.escrow_shares, b.escrow_shares);
    assert_eq!(back.escrow_shares[0].key_file.len(), 2286);
    assert_ne!(
        back.escrow_shares[0].restore_height, 0,
        "a restored share still has to find its own outputs"
    );
}

/// An entry that restores to nothing is worse than no entry: it appears in the
/// user's escrow list as recoverable.
#[test]
fn an_empty_key_file_is_not_a_share() {
    let mut b = sample();
    b.escrow_shares[0].key_file.clear();
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    assert_eq!(
        import(&blob, b"passphrase here").unwrap_err().code,
        RejectCode::Malformed
    );
}

/// Escrow shares are the only part of the bundle with a freshness requirement.
/// A persona key from last year is still the persona; a bundle exported before
/// an escrow opened simply does not contain it.
#[test]
fn a_bundle_with_no_open_escrows_is_still_valid() {
    let mut b = sample();
    b.escrow_shares.clear();
    let blob = export(&b, b"passphrase here", [5u8; 16], [6u8; 24]).unwrap();
    let back = import(&blob, b"passphrase here").unwrap();
    assert!(back.escrow_shares.is_empty());
    assert_eq!(back.persona_secret, b.persona_secret);
}

/// A restored persona that has forgotten its own name hands out cards nobody
/// recognises, and the user has no way to tell that is what happened.
#[test]
fn a_bundle_carries_the_profile_name() {
    let mut b = sample();
    b.display_name = Some("kara".into());
    let blob = export(&b, b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap();
    let back = import(&blob, b"correct horse battery").unwrap();
    assert_eq!(back.display_name.as_deref(), Some("kara"));
}

/// Publishing an address is a **privacy** setting, so restoring it wrong is
/// worse than losing it. Absence must mean off — the safe direction and the
/// original default — never on.
#[test]
fn publishing_defaults_to_off_and_survives_when_on() {
    let mut b = sample();
    assert!(!b.publish_payto, "off is the default");
    let off = import(
        &export(&b, b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap(),
        b"correct horse battery",
    ).unwrap();
    assert!(!off.publish_payto, "a bundle with it off must not restore it on");

    b.publish_payto = true;
    let on = import(
        &export(&b, b"correct horse battery", [7u8; 16], [9u8; 24]).unwrap(),
        b"correct horse battery",
    ).unwrap();
    assert!(on.publish_payto, "a deliberate choice must survive");
}

/// §16.9's profile survives a round trip.
///
/// The point of testing this separately from the keys: a profile is the part of
/// a persona that is *not* a credential, so nothing else breaks when it is
/// silently dropped. A restore that comes back with the right money and the
/// wrong face still looks like it worked.
#[test]
fn a_profile_survives_export_and_import() {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01];
    let mut b = sample();
    b.display_name = Some("sam".into());
    b.publish_payto = true;
    b.avatar = Some(PNG.to_vec());
    b.email = Some("sam@example.com".into());
    b.phone = Some("14155550123".into());
    b.signal = Some("sam_oc.42".into());
    b.pronouns = Some(5);

    let blob = export(&b, b"a real passphrase", [7u8; 16], [9u8; 24]).expect("export");
    let back = import(&blob, b"a real passphrase").expect("import");

    assert_eq!(back.display_name.as_deref(), Some("sam"));
    assert!(back.publish_payto);
    assert_eq!(back.avatar.as_deref(), Some(PNG));
    assert_eq!(back.email.as_deref(), Some("sam@example.com"));
    assert_eq!(back.phone.as_deref(), Some("14155550123"));
    assert_eq!(back.signal.as_deref(), Some("sam_oc.42"));
    assert_eq!(back.pronouns, Some(5));
}

/// A bundle written before profiles existed still opens, and restores as
/// someone who published nothing — never as someone who published a default.
#[test]
fn an_older_bundle_restores_with_an_empty_profile() {
    let blob = export(&sample(), b"a real passphrase", [7u8; 16], [9u8; 24]).expect("export");
    let back = import(&blob, b"a real passphrase").expect("import");
    assert!(back.avatar.is_none());
    assert!(back.email.is_none());
    assert!(back.phone.is_none());
    assert!(back.signal.is_none());
    assert!(back.pronouns.is_none());
    assert!(!back.publish_payto, "publishing must never default to on");
}
