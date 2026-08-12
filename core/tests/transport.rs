//! §18.7 transport identifiers.

use ducat_core::transport::*;

/// The property that made registration unnecessary. If someone "improves" the
/// AID to something that looks more official, this catches it — an `0xA…` or
/// `0xD…` prefix claims a registration nobody holds.
#[test]
fn the_aid_sits_in_the_registration_free_range() {
    assert!(is_proprietary_aid(&NFC_AID));
    assert_eq!(NFC_AID[0] & 0xF0, 0xF0, "bits 8-5 of the first byte must all be set");
    assert_eq!(&NFC_AID[1..], b"DUCAT");
    // Registered ranges are not ours to use.
    assert!(!is_proprietary_aid(&[0xA0, 0x00, 0x00, 0x00, 0x03])); // Visa's RID shape
    assert!(!is_proprietary_aid(&[0xD2, 0x76, 0x00, 0x01, 0x24])); // a national RID shape
}

#[test]
fn aid_length_is_within_iso_7816_bounds() {
    assert!((5..=16).contains(&NFC_AID.len()));
    assert!(!is_proprietary_aid(&[0xF0, 0x44, 0x55, 0x43])); // 4 bytes, below minimum
    assert!(!is_proprietary_aid(&[0xF0; 17]));               // 17 bytes, above maximum
}

/// Four distinct UUIDs sharing a base, differing only in the 16-bit slot. A
/// copy-paste that left two identical would make the service unusable in a way
/// that is tedious to debug over the air.
#[test]
fn the_ble_uuids_are_distinct_and_share_a_base() {
    let all = [
        BLE_SERVICE_UUID,
        BLE_BOOTSTRAP_WRITE_UUID,
        BLE_SESSION_NOTIFY_UUID,
        BLE_PSM_DISCOVERY_UUID,
    ];
    for (i, a) in all.iter().enumerate() {
        assert_eq!(a.len(), 36, "canonical UUID form");
        for b in all.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
        // Same base: everything after the first 8 hex digits.
        assert_eq!(&a[8..], &BLE_SERVICE_UUID[8..]);
        assert_eq!(&a[..4], &BLE_SERVICE_UUID[..4]);
    }
}

/// The PSM is read from the peer, never assumed. Pinning one would pin a value
/// the specification does not control.
#[test]
fn the_psm_range_is_the_le_dynamic_range() {
    assert_eq!(*LE_PSM_RANGE.start(), 0x0080);
    assert_eq!(*LE_PSM_RANGE.end(), 0x00FF);
    assert!(LE_PSM_RANGE.contains(&0x0080));
    assert!(!LE_PSM_RANGE.contains(&0x007F), "SIG-assigned range is not ours");
}

#[test]
fn qr_modes_are_distinguishable_from_each_other() {
    assert_eq!(QR_INLINE_MAGIC, b"DCAT");
    assert!(QR_TOKEN_SCHEME.ends_with(':'));
    // A token URI must not be mistaken for an inline payload.
    assert!(!QR_TOKEN_SCHEME.as_bytes().starts_with(QR_INLINE_MAGIC));
}
