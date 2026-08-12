//! Transport identifiers (§18.7).
//!
//! Behaviour is portable; identifiers are not. These are constants rather than
//! configuration because two clients that disagree on them cannot discover each
//! other at all — there is no negotiation layer below the layer these establish.

/// ISO 7816 application identifier for the NFC binding.
///
/// `0xF0` ‖ `"DUCAT"`. The leading `0xF` is not arbitrary and not a placeholder:
/// ISO/IEC 7816-5 assigns the first nibble by category — `'A'` internationally
/// registered, `'D'` nationally registered — and reserves the range where bits
/// 8–5 of the first byte are all `1` for **proprietary identifiers that require
/// no registration**. Drafts before 0.48 called this "pending real RID
/// registration"; there was nothing to wait for.
///
/// The cost of that freedom is that nothing guarantees uniqueness, so the name
/// is spelled out in full. AIDs may run to 16 bytes; the earlier four-character
/// `"DCAT"` was fitted to a 5-byte *minimum* that was never a maximum.
///
/// **Treat as immutable.** iOS readers declare selectable AIDs at build time in
/// `com.apple.developer.nfc.readersession.iso7816.select-identifiers`, so this
/// cannot be discovered at runtime and changing it is a simultaneous update of
/// every iOS client in existence.
pub const NFC_AID: [u8; 6] = [0xF0, b'D', b'U', b'C', b'A', b'T'];

/// BLE GATT service.
pub const BLE_SERVICE_UUID: &str = "30910001-5923-472e-860f-56eaed5db906";
/// Reader → presenter: the bootstrap blob (§15.3).
pub const BLE_BOOTSTRAP_WRITE_UUID: &str = "30910002-5923-472e-860f-56eaed5db906";
/// Presenter → reader: session traffic.
pub const BLE_SESSION_NOTIFY_UUID: &str = "30910003-5923-472e-860f-56eaed5db906";
/// Where the presenter publishes its L2CAP PSM.
///
/// The PSM itself is deliberately **not** a constant. LE Connection-Oriented
/// Channels take dynamic PSMs in `0x0080–0x00FF`, allocated by the local stack
/// at listen time, so a specification that pinned one would be pinning a value
/// it does not control.
pub const BLE_PSM_DISCOVERY_UUID: &str = "30910004-5923-472e-860f-56eaed5db906";

/// Dynamic PSM range for LE CoC. A presenter's advertised PSM must fall here.
pub const LE_PSM_RANGE: std::ops::RangeInclusive<u16> = 0x0080..=0x00FF;

/// Magic prefix for QR `inline` mode (§18.7). Raw binary in byte mode.
pub const QR_INLINE_MAGIC: &[u8; 4] = b"DCAT";
/// URI scheme for QR `token` mode.
pub const QR_TOKEN_SCHEME: &str = "ducat:";

/// Whether an AID sits in ISO 7816-5's registration-free proprietary range.
///
/// The rule is on bits 8–5 of the first byte, all set. Checking the whole nibble
/// rather than comparing against `0xF0` keeps the *reason* in the code: a future
/// AID of `0xF1…` would be equally valid, and an `0xA…` or `0xD…` one would be
/// claiming a registration nobody holds.
pub fn is_proprietary_aid(aid: &[u8]) -> bool {
    // 5 is the minimum AID length and 16 the maximum.
    (5..=16).contains(&aid.len()) && (aid[0] & 0xF0) == 0xF0
}
