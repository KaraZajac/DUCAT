//! Contact cards, prekeys and sealed messages, across the bridge (§16.9–§16.11).
//!
//! Kotlin gets bytes and structs; every protocol decision stays in `core`, which
//! is the same rule the rest of this bridge follows. A display name is checked
//! against §16.9's bound *here* rather than in Compose, because a bound enforced
//! in the UI is a bound a second UI forgets.

use ducat_core::cbor::decode;
use ducat_core::contact::{
    MessageKind,
    card_from_uri, card_to_uri, check_message, subkey_for as ring_subkey, still_in_ring,
    thread_aad as pair_aad, ContactCard, ContactDetails, LineItem, LogHead, Message, Pronouns,
    MAX_DISPLAY_NAME_CHARS, MAX_MESSAGE_CHARS, MAX_RECORD_KEY_CHARS,
};
use ducat_core::hpke::{self, PreKey, PreKeyBundle, PreKeyStore, SealedMessage};
use ducat_core::sig::{ObjectType, PublicKey, SecretKey, SignedBytes, Suite};
use ducat_core::wire::{open as open_env, seal as seal_env};

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum ContactError {
    #[error("{0}")]
    Refused(String),
}

fn refuse(e: impl std::fmt::Debug) -> ContactError {
    ContactError::Refused(format!("{e:?}"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The writer a contact inbox admits.
///
/// Any valid Ed25519 pair works: VLD0 signs with Ed25519, so Veilid will accept
/// one we generated as an SMPL member. Generated here rather than in Kotlin so
/// the secret is produced by the same CSPRNG as every other key in this bridge.
#[derive(uniffi::Record, Clone)]
pub struct WriterKeys {
    pub public: Vec<u8>,
    pub secret: Vec<u8>,
}

#[uniffi::export]
pub fn generate_writer_keys() -> WriterKeys {
    // The seed is kept rather than recovered: a VLD0 secret key *is* the
    // 32-byte Ed25519 seed, so this is the form Veilid will accept back as a
    // writer, and deriving the public key from it keeps the two in step.
    let seed = random32();
    let sk = SecretKey::ed25519_from_bytes(&seed);
    WriterKeys {
        public: sk.public().to_bytes().to_vec(),
        secret: seed.to_vec(),
    }
}

/// A card ready to hand over, in every form the UI needs at once.
#[derive(uniffi::Record, Clone)]
pub struct IssuedCard {
    /// The signed envelope. What NFC transfers verbatim.
    pub bytes: Vec<u8>,
    /// The same bytes as a `ducat:` URI (§18.7), with the writer secret beside
    /// them — what a QR encodes and what pastes into a message to a friend.
    pub uri: String,
    pub expiry: u64,
}

/// Mint a contact card naming an inbox that already exists.
///
/// The record is created by the caller (`node_dht_create_shared`) because that
/// needs the node, and this module deliberately holds no node state. What
/// happens here is only signing and encoding.
#[uniffi::export]
pub fn create_contact_card(
    persona_secret: Vec<u8>,
    inbox_key: String,
    writer_public: Vec<u8>,
    display_name: Option<String>,
    writer_secret: Vec<u8>,
    valid_secs: u64,
) -> Result<IssuedCard, ContactError> {
    if let Some(n) = &display_name {
        // Checked here so both the character bound and §16.10's "no empty text"
        // rule are enforced once, for every caller.
        if n.is_empty() || n.chars().count() > MAX_DISPLAY_NAME_CHARS {
            return Err(ContactError::Refused(format!(
                "a name must be 1 to {MAX_DISPLAY_NAME_CHARS} characters"
            )));
        }
    }
    if inbox_key.is_empty() || inbox_key.chars().count() > MAX_RECORD_KEY_CHARS {
        return Err(ContactError::Refused("inbox key is not a record key".into()));
    }
    let sk = persona_key(&persona_secret)?;
    let expiry = now() + valid_secs;
    let card = ContactCard {
        version: 1,
        suite: 1,
        persona: sk.public().to_bytes().to_vec(),
        inbox_key,
        writer_public,
        display_name,
        expiry,
    };
    let bytes = seal_env(
        &SignedBytes::from_received(card.to_value().encode()).map_err(refuse)?,
        ObjectType::ContactOffer,
        &sk,
    );
    let wsec: [u8; 32] = writer_secret
        .try_into()
        .map_err(|_| ContactError::Refused("writer secret is not 32 bytes".into()))?;
    Ok(IssuedCard {
        uri: card_to_uri(&bytes, &wsec),
        bytes,
        expiry,
    })
}

/// What the UI shows before someone decides to add a contact.
#[derive(uniffi::Record, Clone)]
pub struct ScannedCard {
    pub persona: Vec<u8>,
    pub inbox_key: String,
    pub writer_public: Vec<u8>,
    /// The capability. Whoever holds this can write the inbox's reply subkey,
    /// and **Veilid enforces that** — it is not a check this code performs.
    pub writer_secret: Vec<u8>,
    /// Self-asserted. §16.9 requires this be shown as unverified, and the
    /// petname the user assigns is the name actually displayed later.
    pub asserted_name: Option<String>,
    pub expiry: u64,
    pub expired: bool,
}

/// Read a card that arrived by NFC, QR or a pasted `ducat:` URI.
///
/// The signature proves the persona key made this card. It does **not** prove
/// the person who sent it holds that key — §16.9 is explicit that the carrying
/// channel supplies that, and the UI must not imply otherwise.
#[uniffi::export]
pub fn read_contact_card(input: String) -> Result<ScannedCard, ContactError> {
    let (env, secret) = card_from_uri(&input).map_err(refuse)?;
    let card = verify_card(&env)?;
    Ok(ScannedCard {
        persona: card.persona.clone(),
        inbox_key: card.inbox_key.clone(),
        writer_public: card.writer_public.clone(),
        writer_secret: secret.to_vec(),
        asserted_name: card.display_name.clone(),
        expiry: card.expiry,
        expired: now() > card.expiry,
    })
}

/// Encode what goes into an inbox subkey: who I am, where to leave things, and
/// Everything optional a person may publish about themselves (§16.9).
///
/// One record so a caller passes a profile rather than eight arguments, and so
/// adding a field later does not change every call site.
///
/// **None of it is in the card.** The card is a QR code someone scans across a
/// counter, and a picture does not fit in one. Profile travels on the record
/// afterwards, which is also why it can change without reissuing anything.
#[derive(uniffi::Record, Clone, Default)]
pub struct Profile {
    pub avatar: Option<Vec<u8>>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub signal: Option<String>,
    /// 1 she/her, 2 she/they, 3 he/him, 4 he/they, 5 they/them, 6 any.
    pub pronouns: Option<u32>,
    /// The car (§15.12): what a rider looks for at the curb.
    pub car_model: Option<String>,
    pub car_color: Option<String>,
    pub plate: Option<String>,
}

impl Profile {
    fn pronouns_enum(&self) -> Result<Option<Pronouns>, ContactError> {
        Ok(match self.pronouns {
            None => None,
            Some(v) => Some(match v {
                1 => Pronouns::SheHer,
                2 => Pronouns::SheThey,
                3 => Pronouns::HeHim,
                4 => Pronouns::HeThey,
                5 => Pronouns::TheyThem,
                6 => Pronouns::Any,
                _ => return Err(ContactError::Refused("unknown pronouns code".into())),
            }),
        })
    }

    fn code_of(p: Option<Pronouns>) -> Option<u32> {
        p.map(|p| match p {
            Pronouns::SheHer => 1,
            Pronouns::SheThey => 2,
            Pronouns::HeHim => 3,
            Pronouns::HeThey => 4,
            Pronouns::TheyThem => 5,
            Pronouns::Any => 6,
        })
    }
}

/// The labels, in the order a picker should show them. Kept here so the app and
/// the protocol cannot drift on what a code means.
#[uniffi::export]
pub fn pronoun_options() -> Vec<String> {
    [
        Pronouns::SheHer,
        Pronouns::SheThey,
        Pronouns::HeHim,
        Pronouns::HeThey,
        Pronouns::TheyThem,
        Pronouns::Any,
    ]
    .iter()
    .map(|p| p.label().to_string())
    .collect()
}

/// the keys to seal with.
#[uniffi::export]
pub fn build_contact_details(
    persona_secret: Vec<u8>,
    outbox_key: String,
    prekey_bundle: Vec<u8>,
    display_name: Option<String>,
    // Optional (§16.12). Publishing lets contacts pay without asking, at the
    // cost of the address being reused — the caller decides, not this function.
    payto: Option<String>,
    profile: Profile,
    // What this handshake is for ("profile", "sale", "hail", …). It rides so
    // the party answering can scope their own reply to the moment; the caller
    // is expected to have already trimmed `profile` to match.
    purpose: Option<String>,
) -> Result<Vec<u8>, ContactError> {
    let sk = persona_key(&persona_secret)?;
    let pronouns = profile.pronouns_enum()?;
    // Round-tripped through the decoder before it goes out, so a malformed
    // profile is caught on the device that composed it rather than refused on
    // the device that receives it — where the person who could fix it is not.
    let encoded = ContactDetails {
        version: 1,
        suite: 1,
        persona: sk.public().to_bytes().to_vec(),
        outbox_key,
        prekey_bundle,
        display_name,
        payto,
        avatar: profile.avatar,
        email: profile.email,
        phone: profile.phone,
        signal: profile.signal,
        pronouns,
        car_model: profile.car_model,
        car_color: profile.car_color,
        plate: profile.plate,
        purpose,
    }
    .to_value()
    .encode();
    ContactDetails::from_value(decode(&encoded).map_err(refuse)?).map_err(refuse)?;
    Ok(encoded)
}

/// The other side of that, for a subkey we just read.
#[derive(uniffi::Record, Clone)]
pub struct PeerDetails {
    pub persona: Vec<u8>,
    pub outbox_key: String,
    pub prekey_bundle: Vec<u8>,
    pub asserted_name: Option<String>,
    /// Where they can be paid without asking, if they chose to publish it.
    pub payto: Option<String>,
    pub profile: Profile,
    /// What the issuer said this handshake is for (§16.9) — "profile" for a
    /// standing contact code, "sale"/"hail"/… for a transaction. The claimant
    /// reads it to decide how much of their own profile to send back. None on
    /// an older record that predates the field.
    pub purpose: Option<String>,
}

#[uniffi::export]
pub fn parse_contact_details(bytes: Vec<u8>) -> Result<PeerDetails, ContactError> {
    let d = ContactDetails::from_value(decode(&bytes).map_err(refuse)?).map_err(refuse)?;
    Ok(PeerDetails {
        persona: d.persona,
        outbox_key: d.outbox_key,
        prekey_bundle: d.prekey_bundle,
        asserted_name: d.display_name,
        payto: d.payto,
        profile: Profile {
            avatar: d.avatar,
            email: d.email,
            phone: d.phone,
            signal: d.signal,
            pronouns: Profile::code_of(d.pronouns),
            car_model: d.car_model,
            car_color: d.car_color,
            plate: d.plate,
        },
        purpose: d.purpose,
    })
}

// --- the outbox ring (§16.12) ---------------------------------------------

/// Encode a head, republishing our current prekeys with it.
///
/// The bundle rides along because the head is read on every poll, so a
/// refreshed supply reaches every reader for no extra round trip. Without it a
/// pair that exhausts its one-time keys stays on the signed prekey forever —
/// forward secrecy quietly gone, and no path back.
#[uniffi::export]
pub fn build_log_head(
    next_seq: u64,
    prekey_bundle: Option<Vec<u8>>,
    // §16.16's watermark: how far into the peer's log this user has read.
    // None means receipts are off, which is the default.
    read_up_to: Option<u64>,
    // §16.12: the ring size, when this log uses other than the default eight.
    ring: Option<u32>,
) -> Vec<u8> {
    LogHead { version: 1, suite: 1, next_seq, prekey_bundle, read_up_to, ring }
        .to_value()
        .encode()
}

/// A head, decoded.
#[derive(uniffi::Record, Clone)]
pub struct HeadInfo {
    pub next_seq: u64,
    /// The peer's read watermark into *our* log, if they publish one (§16.16).
    pub read_up_to: Option<u64>,
    /// The ring size this log uses; readers MUST honour it (§16.12).
    pub ring: Option<u32>,
    /// Present when the publisher included refreshed keys. A reader that sees
    /// one should replace its cached copy: keeping a stale bundle means sealing
    /// to keys that were consumed long ago.
    pub prekey_bundle: Option<Vec<u8>>,
}

#[uniffi::export]
pub fn parse_log_head(bytes: Vec<u8>) -> Result<HeadInfo, ContactError> {
    let h = LogHead::from_value(decode(&bytes).map_err(refuse)?).map_err(refuse)?;
    Ok(HeadInfo { next_seq: h.next_seq, read_up_to: h.read_up_to, ring: h.ring, prekey_bundle: h.prekey_bundle })
}

/// Which subkey a sequence number occupies. Subkey 0 is the head, so an
/// off-by-one here overwrites it and loses the whole log rather than one entry.
#[uniffi::export]
pub fn log_subkey(seq: u64, subkey_count: u32) -> u32 {
    ring_subkey(seq, subkey_count)
}

/// Whether a reader can still fetch `seq`, or the ring has passed it by. A
/// reader that was away too long has genuinely lost messages and must be able
/// to tell, rather than render a thread with a hole in it.
#[uniffi::export]
pub fn log_still_readable(seq: u64, next_seq: u64, subkey_count: u32) -> bool {
    still_in_ring(seq, next_seq, subkey_count)
}

/// The AAD binding a ciphertext to one conversation, symmetric by construction.
#[uniffi::export]
pub fn thread_aad(mine_hex: String, theirs_hex: String) -> Vec<u8> {
    pair_aad(&mine_hex, &theirs_hex)
}

// --- §16.11 ---------------------------------------------------------------

/// A freshly generated set of prekeys: what to publish, and what to keep.
#[derive(uniffi::Record, Clone)]
pub struct PrekeyMaterial {
    /// Published to the rendezvous so people can write to us.
    pub bundle: Vec<u8>,
    /// The signed prekey's secret. Rotated on a schedule, never consumed.
    pub signed_secret: Vec<u8>,
    /// One-time secrets, in the same order as the ids below. **Deleting these
    /// after use is the entire forward-secrecy property** — a backup that keeps
    /// them keeps every message they ever opened.
    pub one_time_secrets: Vec<Vec<u8>>,
    pub one_time_ids: Vec<u32>,
}

#[uniffi::export]
pub fn generate_prekeys(
    count: u32,
    valid_secs: u64,
    // Where the ids begin. They were always 1..=count, which made every
    // caller's ids collide with every other caller's: a second card, or a
    // top-up, silently reused ids that peers' cached bundles still pointed at,
    // and the secrets behind those ids were gone. Ids are cheap; uniqueness is
    // the entire point of having them.
    start_id: u32,
    // An existing signed-prekey secret to keep, rather than rotating it as a
    // side effect. Rotation is a real operation with a real cost — everything
    // sealed to the old key stops opening — and it must never happen because a
    // caller wanted more one-time keys.
    reuse_signed_secret: Option<Vec<u8>>,
) -> PrekeyMaterial {
    let (signed_secret, signed_public) = match reuse_signed_secret
        .as_deref()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
    {
        Some(sk) => (sk, hpke::public_of(&sk)),
        None => hpke::derive_keypair(&random32()),
    };
    let mut one_time = Vec::new();
    let mut secrets = Vec::new();
    let mut ids = Vec::new();
    let from = start_id.max(1);
    for i in from..from.saturating_add(count) {
        let (sk, pk) = hpke::derive_keypair(&random32());
        one_time.push(PreKey { id: i, public: pk });
        secrets.push(sk.to_vec());
        ids.push(i);
    }
    let bundle = PreKeyBundle {
        version: 1,
        suite: 1,
        signed_prekey: signed_public,
        one_time,
        expiry: now() + valid_secs,
    };
    PrekeyMaterial {
        bundle: bundle.to_value().encode(),
        signed_secret: signed_secret.to_vec(),
        one_time_secrets: secrets,
        one_time_ids: ids,
    }
}

/// A message sealed to a contact, plus which key it used.
#[derive(uniffi::Record, Clone)]
pub struct SealedOut {
    pub bytes: Vec<u8>,
    pub prekey_id: u32,
    /// False means the one-time supply was exhausted and this message is only
    /// forward-secret until the signed prekey rotates. §16.11 requires the
    /// caller be able to see this rather than treating both as success.
    pub forward_secret: bool,
    /// The link the *next* message in this thread must carry (§16.10).
    ///
    /// Returned here because it is computed over the **plaintext** message, and
    /// after sealing the caller no longer holds that. A caller that derived a
    /// link from the ciphertext instead would produce a chain that verifies
    /// against nothing.
    pub next_link: Vec<u8>,
}

/// An attachment reference, across the bridge (§16.15).
#[derive(uniffi::Record, Clone)]
pub struct AttachmentRef {
    /// Exactly one transport (§16.15): the record road for small blobs,
    /// or the swarm road (key + digest together) for what a record
    /// cannot hold.
    pub record_key: Option<String>,
    pub swarm_key: Option<String>,
    pub swarm_digest: Option<Vec<u8>>,
    pub key: Vec<u8>,
    pub nonce: Vec<u8>,
    pub len: u64,
    pub ct_hash: Vec<u8>,
    pub mime: String,
    pub name: Option<String>,
}

/// One live-position update as it crosses the bridge (§15.12).
#[derive(uniffi::Record)]
pub struct PositionFrameIo {
    pub counter: u64,
    pub lat_e7: i64,
    pub lon_e7: i64,
    /// Whole degrees 0..=359, or absent.
    pub heading: Option<u16>,
    pub captured: u64,
}

/// Seal one position frame into the value written to the stream's record
/// subkey (§15.12). `nonce` is fresh per write, drawn by the caller; the
/// record key is bound in as associated data, so the value cannot be lifted
/// into another record. Returns a constant length whatever the fields.
#[uniffi::export]
pub fn position_seal(
    stream_key: Vec<u8>,
    record_key: String,
    nonce: Vec<u8>,
    frame: PositionFrameIo,
) -> Result<Vec<u8>, ContactError> {
    let sk: [u8; 32] = stream_key
        .try_into()
        .map_err(|_| ContactError::Refused("a stream key is 32 bytes".into()))?;
    let n: [u8; ducat_core::position::NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| ContactError::Refused("a position nonce is 24 bytes".into()))?;
    let f = ducat_core::position::PositionFrame {
        counter: frame.counter,
        lat_e7: frame.lat_e7,
        lon_e7: frame.lon_e7,
        heading: frame.heading,
        captured: frame.captured,
    };
    Ok(ducat_core::position::seal(&sk, &record_key, &n, &f))
}

/// Open a value read from a live-position record (§15.12). The record key MUST
/// be the one the value was written under — it is the associated data — so a
/// mismatch fails to authenticate rather than returning the wrong ride's
/// position. The caller enforces counter monotonicity across calls.
#[uniffi::export]
pub fn position_open(
    stream_key: Vec<u8>,
    record_key: String,
    value: Vec<u8>,
) -> Result<PositionFrameIo, ContactError> {
    let sk: [u8; 32] = stream_key
        .try_into()
        .map_err(|_| ContactError::Refused("a stream key is 32 bytes".into()))?;
    let f = ducat_core::position::open(&sk, &record_key, &value).map_err(refuse)?;
    Ok(PositionFrameIo {
        counter: f.counter,
        lat_e7: f.lat_e7,
        lon_e7: f.lon_e7,
        heading: f.heading,
        captured: f.captured,
    })
}

/// A fresh publication master secret (§16.20 track). One per publication;
/// every period's content key derives from it, so this is the only key a
/// publisher stores or backs up.
#[uniffi::export]
pub fn publication_master_create() -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k.to_vec()
}

/// One period's content key, derived — deterministic over (master, id), so
/// a restored device and a paying member both arrive at the same key.
#[uniffi::export]
pub fn publication_period_key(master: Vec<u8>, period_id: String) -> Result<Vec<u8>, ContactError> {
    let m: [u8; 32] = master
        .try_into()
        .map_err(|_| ContactError::Refused("a publication master is 32 bytes".into()))?;
    Ok(ducat_core::publish::period_key(&m, &period_id)
        .map_err(refuse)?
        .to_vec())
}

/// Seal one publication chunk for one (record, subkey) landing site.
#[uniffi::export]
pub fn publication_seal_chunk(
    key: Vec<u8>,
    record_key: String,
    subkey: u32,
    nonce: Vec<u8>,
    plaintext: Vec<u8>,
) -> Result<Vec<u8>, ContactError> {
    let k: [u8; 32] = key
        .try_into()
        .map_err(|_| ContactError::Refused("a period key is 32 bytes".into()))?;
    let n: [u8; ducat_core::publish::NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| ContactError::Refused("a publication nonce is 24 bytes".into()))?;
    Ok(ducat_core::publish::seal_chunk(&k, &record_key, subkey, &n, &plaintext))
}

/// Open a chunk read from a record's slot; the landing site is the AAD, so
/// pass where it was actually read from.
#[uniffi::export]
pub fn publication_open_chunk(
    key: Vec<u8>,
    record_key: String,
    subkey: u32,
    value: Vec<u8>,
) -> Result<Vec<u8>, ContactError> {
    let k: [u8; 32] = key
        .try_into()
        .map_err(|_| ContactError::Refused("a period key is 32 bytes".into()))?;
    ducat_core::publish::open_chunk(&k, &record_key, subkey, &value).map_err(refuse)
}

/// Seal attachment bytes; returns the ciphertext to park in a record.
/// The key and nonce are the caller's to generate fresh — never reuse either.
#[uniffi::export]
pub fn attachment_seal(key: Vec<u8>, nonce: Vec<u8>, plaintext: Vec<u8>) -> Result<Vec<u8>, ContactError> {
    let k: [u8; 32] = key.try_into().map_err(|_| ContactError::Refused("key is 32 bytes".into()))?;
    let n: [u8; 24] = nonce.try_into().map_err(|_| ContactError::Refused("nonce is 24 bytes".into()))?;
    Ok(ducat_core::contact::attachment_seal(&k, &n, &plaintext))
}

/// Open fetched attachment bytes. Verify the hash before calling (§16.15).
#[uniffi::export]
pub fn attachment_open(key: Vec<u8>, nonce: Vec<u8>, ciphertext: Vec<u8>) -> Result<Vec<u8>, ContactError> {
    let k: [u8; 32] = key.try_into().map_err(|_| ContactError::Refused("key is 32 bytes".into()))?;
    let n: [u8; 24] = nonce.try_into().map_err(|_| ContactError::Refused("nonce is 24 bytes".into()))?;
    ducat_core::contact::attachment_open(&k, &n, &ciphertext).map_err(refuse)
}

/// One line on a bill, across the bridge (§16.13).
#[derive(uniffi::Record, Clone)]
pub struct BillLine {
    pub description: String,
    pub amount_pxmr: u64,
}

/// Seal one message in a thread.
#[uniffi::export]
pub fn seal_message(
    bundle_bytes: Vec<u8>,
    seq: u64,
    prev_link: Vec<u8>,
    body: String,
    thread_aad: Vec<u8>,
    // 0 text, 1 request, 2 notice (§16.13). A request carries no authority —
    // the payer still decides at §15.5's confirm screen.
    kind: u8,
    amount_pxmr: Option<u64>,
    txid: Option<Vec<u8>>,
    // Only a request may name one (§16.13). Where to pay travels with the ask
    // so the payer needs nothing from a record that may be stale.
    payto: Option<String>,
    // What the money is for. Empty means not itemised; the items plus tax MUST
    // add up to `amount_pxmr`, and core refuses the message if they do not.
    items: Vec<BillLine>,
    tax_pxmr: Option<u64>,
    // §16.14: the message a reaction is about, in the recipient's log unless
    // `re_own`.
    re_seq: Option<u64>,
    re_own: bool,
    // §16.15: a sealed blob parked in its own record.
    attachment: Option<AttachmentRef>,
    // §15.12: a ride offer's courtesy figure; refused on any other kind.
    eta_secs: Option<u64>,
    // §17.9 ceremony: opaque threshold bytes, round tag, per-escrow context.
    payload: Option<Vec<u8>>,
    round: Option<u64>,
    ceremony_id: Option<Vec<u8>>,
    // §15.12: a live-position stream reference on a kind-11 message. Both or
    // neither — core refuses a half-reference, and a reference on any other
    // kind.
    position_record: Option<String>,
    position_stream_key: Option<Vec<u8>>,
    // §16.19: which group this rides in, the sender's own counter there, and
    // the group reference for replies/reactions — the pairwise re_seq cannot
    // name a fanned-out message, so groups target by (sender, counter).
    group_id: Option<Vec<u8>>,
    group_seq: Option<u64>,
    group_re_sender: Option<Vec<u8>>,
    group_re_seq: Option<u64>,
    // §16.20: a publication period's key on a kind-13 message. The period
    // pair together or not at all; the shelf pair likewise; core refuses
    // every other arrangement.
    pub_period_id: Option<String>,
    pub_period_key: Option<Vec<u8>>,
    pub_record: Option<String>,
    pub_head_key: Option<Vec<u8>>,
    // §16.20's shipment: the swarm pair, together or not at all.
    pub_swarm_key: Option<String>,
    pub_swarm_digest: Option<Vec<u8>>,
    // §16.21's door: the call route and id, together or not at all.
    call_route: Option<Vec<u8>>,
    call_id: Option<Vec<u8>>,
    // §16.20's ask: the period a kind-16 wants sold to it. A label and no
    // authority — naming one obliges the publisher to nothing.
    wanted_period: Option<String>,
) -> Result<SealedOut, ContactError> {
    if body.is_empty() || body.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ContactError::Refused(format!(
            "a message must be 1 to {MAX_MESSAGE_CHARS} characters"
        )));
    }
    let bundle =
        PreKeyBundle::from_value(decode(&bundle_bytes).map_err(refuse)?).map_err(refuse)?;
    if now() > bundle.expiry {
        return Err(ContactError::Refused(
            "their published keys have expired; ask them to come online".into(),
        ));
    }
    let prev: [u8; 32] = prev_link
        .try_into()
        .map_err(|_| ContactError::Refused("previous link is not 32 bytes".into()))?;
    // Both or neither, checked before we build: a lone record or a lone key is
    // a caller bug, refused here rather than shipped as a half-reference.
    let position = match (position_record, position_stream_key) {
        (Some(record_key), Some(k)) => {
            let stream_key: [u8; 32] = k.try_into().map_err(|_| {
                ContactError::Refused("a position stream key is 32 bytes".into())
            })?;
            Some(ducat_core::contact::PositionRef { record_key, stream_key })
        }
        (None, None) => None,
        _ => {
            return Err(ContactError::Refused(
                "a position reference carries its record and its key together".into(),
            ))
        }
    };
    let call = match (call_route, call_id) {
        (Some(route), Some(id)) => {
            let id: [u8; 8] = id
                .try_into()
                .map_err(|_| ContactError::Refused("a call id is 8 bytes".into()))?;
            if route.is_empty() || route.len() > ducat_core::contact::MAX_CALL_ROUTE {
                return Err(ContactError::Refused("a call route is 1 to 4096 bytes".into()));
            }
            Some(ducat_core::contact::CallRef { route, id })
        }
        (None, None) => None,
        _ => {
            return Err(ContactError::Refused(
                "a call carries its route and its id together".into(),
            ))
        }
    };
    let publication = match (pub_period_id, pub_period_key) {
        (Some(period_id), Some(k)) => {
            let period_key: [u8; 32] = k.try_into().map_err(|_| {
                ContactError::Refused("a period key is 32 bytes".into())
            })?;
            let head_key = match (&pub_record, pub_head_key) {
                (Some(_), Some(h)) => Some(h.try_into().map_err(|_| {
                    ContactError::Refused("a head key is 32 bytes".into())
                })?),
                (None, None) => None,
                _ => {
                    return Err(ContactError::Refused(
                        "a publication shelf carries its record and its head key together".into(),
                    ))
                }
            };
            let swarm_digest = match (&pub_swarm_key, pub_swarm_digest) {
                (Some(_), Some(d)) => Some(d.try_into().map_err(|_| {
                    ContactError::Refused("an index digest is 32 bytes".into())
                })?),
                (None, None) => None,
                _ => {
                    return Err(ContactError::Refused(
                        "a swarm share carries its key and its index digest together".into(),
                    ))
                }
            };
            Some(ducat_core::contact::PublicationKey {
                period_id,
                period_key,
                record_key: pub_record,
                head_key,
                swarm_key: pub_swarm_key,
                swarm_digest,
            })
        }
        (None, None) => None,
        _ => {
            return Err(ContactError::Refused(
                "a publication key carries its period id and its key together".into(),
            ))
        }
    };
    let msg = Message {
        version: 1, suite: 1, seq, prev, body, timestamp: now(),
        // §16.20's ask travels as a bare label on kind 16 and nothing else.
        wanted_period: if kind == 16 { wanted_period } else { None },
        kind: match kind {
            1 => MessageKind::PaymentRequest,
            2 => MessageKind::PaymentSent,
            3 => MessageKind::Receipt,
            4 => MessageKind::Reaction,
            5 => MessageKind::Retract,
            6 => MessageKind::RideOffer,
            7 => MessageKind::RideAccept,
            8 => MessageKind::DkgRound,
            9 => MessageKind::FrostRound,
            10 => MessageKind::CeremonyAbort,
            11 => MessageKind::PositionRef,
            12 => MessageKind::GroupRoster,
            13 => MessageKind::PublicationKey,
            14 => MessageKind::CallOffer,
            15 => MessageKind::CallAnswer,
            16 => MessageKind::PublicationWanted,
            _ => MessageKind::Text,
        },
        amount_pxmr,
        txid,
        payto,
        items: items
            .into_iter()
            .map(|i| LineItem { description: i.description, amount_pxmr: i.amount_pxmr })
            .collect(),
        tax_pxmr,
        re_seq,
        re_own,
        eta_secs,
        payload,
        round,
        ceremony_id: match ceremony_id {
            Some(c) if c.len() == 32 => Some(c.try_into().unwrap()),
            Some(_) => return Err(ContactError::Refused("ceremony id is 32 bytes".into())),
            None => None,
        },
        attachment: attachment.map(|a| ducat_core::contact::Attachment {
            record_key: a.record_key,
            swarm_key: a.swarm_key,
            swarm_digest: a
                .swarm_digest
                .map(|d| d.try_into().unwrap_or([0u8; 32])),
            key: a.key.try_into().unwrap_or([0u8; 32]),
            nonce: a.nonce.try_into().unwrap_or([0u8; 24]),
            len: a.len,
            ct_hash: a.ct_hash.try_into().unwrap_or([0u8; 32]),
            mime: a.mime,
            name: a.name,
        }),
        position,
        publication,
        group_id,
        group_seq,
        group_re_sender,
        group_re_seq,
        call,
    };
    // A message this encoder produces must be one its own decoder accepts —
    // otherwise the malformation ships sealed, and it is the *recipient's*
    // queue that pays, wedged on bytes only the sender could have refused.
    Message::from_value(
        decode(&msg.to_value().encode()).map_err(refuse)?
    ).map_err(|e| ContactError::Refused(format!("message would be refused: {e:?}")))?;
    let next_link = msg.link().to_vec();
    let (chosen, forward_secret) = bundle.select();
    let mut rng = SystemRng;
    let (enc, ct) = hpke::seal(
        &mut rng,
        &chosen.public,
        &hpke::message_info(1),
        &thread_aad,
        &msg.to_value().encode(),
    )
    .map_err(refuse)?;
    let sealed = SealedMessage { version: 1, suite: 1, prekey_id: chosen.id, enc, ciphertext: ct };
    Ok(SealedOut {
        bytes: sealed.to_value().encode(),
        prekey_id: chosen.id,
        forward_secret,
        next_link,
    })
}

/// This persona's public key, hex, as contacts are keyed by it.
#[uniffi::export]
pub fn persona_public_hex(persona_secret: Vec<u8>) -> Result<String, ContactError> {
    Ok(persona_key(&persona_secret)?
        .public()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Remove a consumed one-time prekey from a published bundle.
///
/// Burning a secret without pruning the bundle leaves the bundle advertising a
/// key that can no longer decrypt anything. Senders pick the first one-time
/// entry, so the *first* consumed id is offered forever and every message after
/// the first is refused — permanently, and identically after a re-fetch, since
/// the stale bundle is what gets re-served.
///
/// §16.11 says a one-time key is used once and deleted. Deleting half of it —
/// the secret but not the advertisement — is worse than not deleting at all,
/// because it fails closed on every subsequent message.
#[uniffi::export]
pub fn prune_prekey(bundle_bytes: Vec<u8>, id: u32) -> Result<Vec<u8>, ContactError> {
    let mut b =
        PreKeyBundle::from_value(decode(&bundle_bytes).map_err(refuse)?).map_err(refuse)?;
    b.one_time.retain(|k| k.id != id);
    Ok(b.to_value().encode())
}

/// Which one-time ids a bundle advertises.
///
/// Needed to *repair* a bundle, not just to count it: a store that burned
/// secrets without pruning has a bundle advertising keys it can no longer use,
/// and no amount of correct behaviour from here on fixes the entries already
/// written. Reconciling needs to know which ones they are.
#[uniffi::export]
pub fn bundle_one_time_ids(bundle_bytes: Vec<u8>) -> Result<Vec<u32>, ContactError> {
    Ok(
        PreKeyBundle::from_value(decode(&bundle_bytes).map_err(refuse)?)
            .map_err(refuse)?
            .one_time
            .iter()
            .map(|k| k.id)
            .collect(),
    )
}

/// How many one-time keys a bundle still advertises.
#[uniffi::export]
pub fn bundle_one_time_count(bundle_bytes: Vec<u8>) -> Result<u32, ContactError> {
    Ok(
        PreKeyBundle::from_value(decode(&bundle_bytes).map_err(refuse)?)
            .map_err(refuse)?
            .one_time
            .len() as u32,
    )
}

/// Which prekey a sealed message names, without opening it.
///
/// The receiver must look up a secret *before* it can decrypt, and looking one
/// up requires knowing which. Reading this costs nothing and reveals nothing
/// the sender did not already put in the clear.
#[uniffi::export]
pub fn sealed_prekey_id(sealed_bytes: Vec<u8>) -> Result<u32, ContactError> {
    Ok(
        SealedMessage::from_value(decode(&sealed_bytes).map_err(refuse)?)
            .map_err(refuse)?
            .prekey_id,
    )
}

/// A message that arrived, after decryption and chain checking.
#[derive(uniffi::Record, Clone)]
pub struct OpenedMessage {
    pub seq: u64,
    pub body: String,
    pub timestamp: u64,
    pub link: Vec<u8>,
    /// True if a one-time key was consumed. The caller MUST delete that secret;
    /// keeping it is keeping the ability to decrypt this message forever.
    pub consumed_one_time: bool,
    pub prekey_id: u32,
    /// 0 text, 1 request, 2 notice (§16.13).
    pub kind: u8,
    pub amount_pxmr: Option<u64>,
    pub txid: Option<Vec<u8>>,
    /// Where a request asks to be paid. Shown on the confirm screen, never
    /// acted on without it.
    pub payto: Option<String>,
    /// What the money is for, if the sender said. Already checked to add up to
    /// the amount — core refuses the message otherwise, so a caller rendering
    /// this does not have to re-derive the total to know it is honest.
    pub items: Vec<BillLine>,
    pub tax_pxmr: Option<u64>,
    pub re_seq: Option<u64>,
    pub re_own: bool,
    pub attachment: Option<AttachmentRef>,
    /// §15.12: a ride offer's distance-in-time, seconds.
    pub eta_secs: Option<u64>,
    /// §17.9 ceremony fields, opaque to DUCAT.
    pub payload: Option<Vec<u8>>,
    pub round: Option<u64>,
    pub ceremony_id: Option<Vec<u8>>,
    /// §15.12: a live-position stream reference. Present only on kind 11.
    pub position: Option<PositionRefOut>,
    /// §16.20: a publication period's key. Present only on kind 13.
    pub publication: Option<PublicationKeyOut>,
    /// §16.20: the period a reader asks to be sold. Present only on kind 16.
    /// A label and nothing else — the ask hands over no capability.
    pub wanted_period: Option<String>,
    /// §16.21: a live call's door. Present only on kinds 14–15.
    pub call_route: Option<Vec<u8>>,
    pub call_id: Option<Vec<u8>>,
    /// §16.19: the group this message belongs to, and its name there.
    pub group_id: Option<Vec<u8>>,
    pub group_seq: Option<u64>,
    pub group_re_sender: Option<Vec<u8>>,
    pub group_re_seq: Option<u64>,
}

/// A live-position stream reference as it crosses the bridge (§15.12).
#[derive(uniffi::Record, Clone)]
pub struct PositionRefOut {
    pub record_key: String,
    pub stream_key: Vec<u8>,
}

/// A publication key as it crosses the bridge (§16.20).
#[derive(uniffi::Record, Clone)]
pub struct PublicationKeyOut {
    pub period_id: String,
    pub period_key: Vec<u8>,
    pub record_key: Option<String>,
    pub head_key: Option<Vec<u8>>,
    pub swarm_key: Option<String>,
    pub swarm_digest: Option<Vec<u8>>,
}

/// A group roster as it crosses the bridge (§16.19).
#[derive(uniffi::Record, Clone)]
pub struct GroupRosterOut {
    pub name: String,
    /// Every member's persona key, 32 bytes each. Grow-only: a reader merges
    /// by union and never removes.
    pub members: Vec<Vec<u8>>,
}

/// Encode a roster payload (§16.19): canonical CBOR, one shape both sides of
/// the wire produce byte-for-byte, which is what lets a future vector pin it.
/// Field 1 the name, field 2 the members.
#[uniffi::export]
pub fn group_roster_encode(
    name: String,
    members: Vec<Vec<u8>>,
) -> Result<Vec<u8>, ContactError> {
    use ducat_core::cbor::Value;
    if members.is_empty() {
        return Err(ContactError::Refused("a roster with nobody in it is not one".into()));
    }
    for m in &members {
        if m.len() != 32 {
            return Err(ContactError::Refused("a member is a 32-byte persona key".into()));
        }
    }
    let mut map = std::collections::BTreeMap::new();
    map.insert(1u64, Value::Text(name));
    map.insert(
        2u64,
        Value::Array(members.into_iter().map(Value::Bytes).collect()),
    );
    Ok(Value::Map(map).encode())
}

/// Decode a roster payload. Strict on shape, tolerant of nothing: a roster
/// that does not parse is a roster nobody should act on.
#[uniffi::export]
pub fn group_roster_decode(bytes: Vec<u8>) -> Result<GroupRosterOut, ContactError> {
    use ducat_core::cbor::Value;
    let v = decode(&bytes).map_err(refuse)?;
    let Value::Map(m) = v else {
        return Err(ContactError::Refused("a roster is a map".into()));
    };
    let name = match m.get(&1) {
        Some(Value::Text(t)) => t.clone(),
        _ => return Err(ContactError::Refused("a roster names its group".into())),
    };
    let members = match m.get(&2) {
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| match e {
                Value::Bytes(b) if b.len() == 32 => Ok(b.clone()),
                _ => Err(ContactError::Refused("a member is a 32-byte persona key".into())),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ContactError::Refused("a roster carries its member list".into())),
    };
    if members.is_empty() {
        return Err(ContactError::Refused("a roster with nobody in it is not one".into()));
    }
    Ok(GroupRosterOut { name, members })
}

/// Open an inbound sealed message and check it follows the thread.
///
/// The caller supplies only the secret for the prekey the message names, so a
/// compromised call cannot walk the whole key store.
#[uniffi::export]
pub fn open_message(
    sealed_bytes: Vec<u8>,
    prekey_secret: Vec<u8>,
    is_one_time: bool,
    expected_seq: u64,
    prev_link: Option<Vec<u8>>,
    thread_aad: Vec<u8>,
) -> Result<OpenedMessage, ContactError> {
    let sealed =
        SealedMessage::from_value(decode(&sealed_bytes).map_err(refuse)?).map_err(refuse)?;
    let secret: [u8; 32] = prekey_secret
        .try_into()
        .map_err(|_| ContactError::Refused("prekey secret is not 32 bytes".into()))?;

    // Reconstructed per call rather than held: this bridge is stateless, and the
    // durable "has this key been used" record belongs in the app's database
    // where it survives a process death. `consumed_one_time` is the instruction.
    if is_one_time && sealed.prekey_id == ducat_core::hpke::SIGNED_PREKEY_ID {
        return Err(ContactError::Refused(
            "id 0 is the signed prekey and is never one-time".into(),
        ));
    }
    let mut store = PreKeyStore::new(secret);
    let id = if is_one_time {
        store.insert_one_time(sealed.prekey_id, secret);
        sealed.prekey_id
    } else {
        0
    };
    let probe = SealedMessage { prekey_id: id, ..sealed.clone() };
    let (plain, consumed) = store
        .open_and_consume(&probe, &hpke::message_info(1), &thread_aad)
        .map_err(refuse)?;

    let msg = Message::from_value(decode(&plain).map_err(refuse)?).map_err(refuse)?;
    let previous = match &prev_link {
        None => None,
        Some(l) => {
            let l: [u8; 32] = l
                .clone()
                .try_into()
                .map_err(|_| ContactError::Refused("previous link is not 32 bytes".into()))?;
            // check_message compares against the *previous message's* link, so
            // reconstruct the minimum that reproduces it.
            Some(l)
        }
    };
    verify_chain(&msg, expected_seq, previous)?;
    Ok(OpenedMessage {
        seq: msg.seq,
        body: msg.body.clone(),
        timestamp: msg.timestamp,
        link: msg.link().to_vec(),
        consumed_one_time: consumed,
        prekey_id: sealed.prekey_id,
        kind: msg.kind as u8,
        amount_pxmr: msg.amount_pxmr,
        txid: msg.txid.clone(),
        payto: msg.payto.clone(),
        items: msg
            .items
            .iter()
            .map(|i| BillLine { description: i.description.clone(), amount_pxmr: i.amount_pxmr })
            .collect(),
        tax_pxmr: msg.tax_pxmr,
        re_seq: msg.re_seq,
        re_own: msg.re_own,
        eta_secs: msg.eta_secs,
        payload: msg.payload.clone(),
        round: msg.round,
        ceremony_id: msg.ceremony_id.map(|c| c.to_vec()),
        attachment: msg.attachment.as_ref().map(|a| AttachmentRef {
            record_key: a.record_key.clone(),
            swarm_key: a.swarm_key.clone(),
            swarm_digest: a.swarm_digest.map(|d| d.to_vec()),
            key: a.key.to_vec(),
            nonce: a.nonce.to_vec(),
            len: a.len,
            ct_hash: a.ct_hash.to_vec(),
            mime: a.mime.clone(),
            name: a.name.clone(),
        }),
        group_id: msg.group_id.clone(),
        group_seq: msg.group_seq,
        group_re_sender: msg.group_re_sender.clone(),
        group_re_seq: msg.group_re_seq,
        position: msg.position.as_ref().map(|p| PositionRefOut {
            record_key: p.record_key.clone(),
            stream_key: p.stream_key.to_vec(),
        }),
        publication: msg.publication.as_ref().map(|p| PublicationKeyOut {
            period_id: p.period_id.clone(),
            period_key: p.period_key.to_vec(),
            record_key: p.record_key.clone(),
            head_key: p.head_key.map(|h| h.to_vec()),
            swarm_key: p.swarm_key.clone(),
            swarm_digest: p.swarm_digest.map(|d| d.to_vec()),
        }),
        wanted_period: msg.wanted_period.clone(),
        call_route: msg.call.as_ref().map(|c| c.route.clone()),
        call_id: msg.call.as_ref().map(|c| c.id.to_vec()),
    })
}

/// §16.10's chain rule, against a stored link rather than a stored message.
fn verify_chain(msg: &Message, expected_seq: u64, prev: Option<[u8; 32]>) -> Result<(), ContactError> {
    if msg.seq != expected_seq {
        return Err(ContactError::Refused(format!(
            "expected message {expected_seq}, got {}",
            msg.seq
        )));
    }
    match prev {
        Some(want) => {
            if msg.prev != want {
                return Err(ContactError::Refused(
                    "this message does not follow the one before it".into(),
                ));
            }
        }
        // No link mid-thread means a recorded gap — a lost or unreadable
        // message the reader stepped past. §16.11: the chain restarts at the
        // next message, prev unverifiable across the gap. Treating "unknown"
        // as "must be the thread start" froze a thread every two minutes for
        // fifteen hours after one honest hole; continuity across a gap is
        // unverifiable *by definition*, and refusing everything after it
        // converts one lost message into a dead thread.
        None if expected_seq > 0 => {}
        None => {
            if msg.prev != [0u8; 32] {
                return Err(ContactError::Refused(
                    "a first message must not claim a predecessor".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Local-only: check a thread we already hold is internally consistent.
#[uniffi::export]
pub fn verify_thread(messages: Vec<Vec<u8>>) -> Result<u64, ContactError> {
    let mut prev: Option<Message> = None;
    for (i, raw) in messages.iter().enumerate() {
        let m = Message::from_value(decode(raw).map_err(refuse)?).map_err(refuse)?;
        check_message(&m, i as u64, prev.as_ref()).map_err(refuse)?;
        prev = Some(m);
    }
    Ok(messages.len() as u64)
}

// --- helpers --------------------------------------------------------------

fn persona_key(secret: &[u8]) -> Result<SecretKey, ContactError> {
    let b: [u8; 32] = secret
        .try_into()
        .map_err(|_| ContactError::Refused("persona secret is not 32 bytes".into()))?;
    Ok(SecretKey::ed25519_from_bytes(&b))
}

fn verify_card(envelope: &[u8]) -> Result<ContactCard, ContactError> {
    // The persona is inside the object the signature covers, so the key is read
    // from the payload and then checked against it. A card that verifies under
    // a key other than the one it names is a card claiming someone else's
    // identity, which is the whole point of checking.
    let peek = decode_card_unverified(envelope)?;
    let pk = PublicKey::from_bytes(Suite::Ed25519X25519, &peek.persona).map_err(refuse)?;
    let (ty, body) = open_env(envelope, &pk).map_err(refuse)?;
    if ty != ObjectType::ContactOffer {
        return Err(ContactError::Refused("not a contact card".into()));
    }
    ContactCard::from_value(decode(body.bytes()).map_err(refuse)?).map_err(refuse)
}

/// Read the payload without checking the signature, only to learn which key to
/// check it with. Never returned to a caller.
fn decode_card_unverified(envelope: &[u8]) -> Result<ContactCard, ContactError> {
    let body = ducat_core::wire::peek_body(envelope).map_err(refuse)?;
    ContactCard::from_value(decode(&body).map_err(refuse)?).map_err(refuse)
}

/// The OS CSPRNG, presented through the trait version `hpke` expects.
///
/// `core` holds no randomness by design (§16.11), so the bridge is where the
/// entropy source is named — and it is the same `OsRng` the rest of this module
/// already uses for keys and salts, rather than a second source nobody audits.
struct SystemRng;

impl hpke::rand_core::TryRng for SystemRng {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        use rand_core::{OsRng, RngCore};
        OsRng.fill_bytes(dst);
        Ok(())
    }
}
impl hpke::rand_core::TryCryptoRng for SystemRng {}

fn random32() -> [u8; 32] {
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b
}


/// A hail notice (§16.17), for the app's rider and driver screens.
#[derive(uniffi::Record)]
pub struct HailInfo {
    /// Who wrote this notice, hex, as verified on the way in.
    ///
    /// Not the poster's persona — a per-listing key (see `board::listing_seed`),
    /// so it is the same across that listing's refreshes and says nothing about
    /// who they are anywhere else. Empty on the way *out*: the encoder derives
    /// it, and a caller setting it would be claiming something.
    pub poster: String,
    pub card: String,
    pub dest: String,
    pub fare_pxmr: Option<u64>,
    pub expiry: u64,
    pub origin_cell: Option<String>,
    pub dest_cell: Option<String>,
    /// The block this notice was stamped against (§16.18.1), read back out so
    /// a caller with a chain view can check that height really has that hash.
    /// Passing the height alone is the cheap test and it is not the whole one:
    /// the height is signed, but the hash beside it is only bytes until
    /// somebody compares it to a block.
    ///
    /// Defaulted, and ignored on the way *out*: the beacon is a parameter of
    /// the encoder, not a field a caller fills in, and every screen that
    /// builds one of these would otherwise have to name two values it has no
    /// opinion about.
    #[uniffi(default = 0)]
    pub beacon_height: u64,
    #[uniffi(default = "")]
    pub beacon_hash: String,
}

/// A listing, as the app hands it over (§16.18).
///
/// Only the searchable half. What the renter needs in order to *arrive* —
/// the address, the plate, the door code — never comes near this struct,
/// because everything in it goes on a board a stranger can read.
#[derive(uniffi::Record)]
pub struct RentalInfo {
    /// Who wrote this notice, hex, as verified on the way in.
    ///
    /// Not the poster's persona — a per-listing key (see `board::listing_seed`),
    /// so it is the same across that listing's refreshes and says nothing about
    /// who they are anywhere else. Empty on the way *out*: the encoder derives
    /// it, and a caller setting it would be claiming something.
    pub poster: String,
    /// The block this notice was stamped against (§16.18.1), read back out so
    /// a caller with a chain view can check that height really has that hash.
    /// Passing the height alone is the cheap test and it is not the whole one:
    /// the height is signed, but the hash beside it is only bytes until
    /// somebody compares it to a block.
    ///
    /// Defaulted, and ignored on the way *out*: the beacon is a parameter of
    /// the encoder, not a field a caller fills in, and every screen that
    /// builds one of these would otherwise have to name two values it has no
    /// opinion about.
    #[uniffi(default = 0)]
    pub beacon_height: u64,
    #[uniffi(default = "")]
    pub beacon_hash: String,
    pub card: String,
    /// 1 = a place to stay, 2 = a vehicle.
    pub kind: u64,
    pub title: String,
    pub area: String,
    pub cell: Option<String>,
    pub price_pxmr: u64,
    pub deposit_pxmr: u64,
    pub expiry: u64,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u64>,
    pub gearbox: Option<u64>,
    pub fuel: Option<u64>,
    pub seats: Option<u64>,
    pub color: Option<String>,
    pub trim: Option<String>,
    pub rooms: Option<u64>,
    pub sleeps: Option<u64>,
    pub size_m2: Option<u64>,
    pub subtype: Option<u64>,
    pub features: Vec<String>,
    /// How many the poster has. One unless they said otherwise.
    pub quantity: u64,
}

fn rental_from_core(n: ducat_core::contact::RentalNotice) -> RentalInfo {
    RentalInfo {
        // Filled by rental_decode once it has verified one; the mapping from
        // core knows nothing about who signed, or about which block it was
        // stamped against.
        poster: String::new(),
        beacon_height: 0,
        beacon_hash: String::new(),
        card: n.card, kind: n.kind, title: n.title, area: n.area, cell: n.cell,
        price_pxmr: n.price_pxmr, deposit_pxmr: n.deposit_pxmr, expiry: n.expiry,
        make: n.make, model: n.model, year: n.year, gearbox: n.gearbox,
        fuel: n.fuel, seats: n.seats, color: n.color, trim: n.trim,
        rooms: n.rooms, sleeps: n.sleeps, size_m2: n.size_m2,
        subtype: n.subtype, features: n.features, quantity: n.quantity,
    }
}

/// Encode a listing, signed for one slot and with the work done for it.
///
/// `listing_id` is the poster's own local id for the listing, and it is what
/// makes the signing key stable across that listing's refreshes without tying
/// it to the persona — see `board::listing_seed`. `board` and `subkey` are the
/// slot the notice is going into, and they are inside the signature, so this
/// has to be called once the slot is chosen rather than once per listing.
///
/// Blocking for a second or so: that is the proof of work, and it is the
/// point. Call it off the main thread.
#[uniffi::export]
pub fn rental_encode(
    info: RentalInfo,
    persona_secret: Vec<u8>,
    listing_id: String,
    board: String,
    subkey: u32,
    // §16.18.1: the Monero block this stamp is mined against, so that next
    // year's boards cannot be mined this afternoon. A caller with no chain
    // view cannot post — there is nothing honest to put here, and a beacon
    // the poster invents is the precomputation this exists to stop.
    beacon_height: u64,
    beacon_hash_hex: String,
) -> Result<Vec<u8>, ContactError> {
    let n = ducat_core::contact::RentalNotice {
        version: 2,
        card: info.card, kind: info.kind, title: info.title, area: info.area,
        cell: info.cell, price_pxmr: info.price_pxmr,
        deposit_pxmr: info.deposit_pxmr, expiry: info.expiry,
        make: info.make, model: info.model, year: info.year,
        gearbox: info.gearbox, fuel: info.fuel, seats: info.seats,
        color: info.color, trim: info.trim, rooms: info.rooms,
        sleeps: info.sleeps, size_m2: info.size_m2,
        subtype: info.subtype, features: info.features,
        // Zero would be a listing of nothing, and the UI has no way to mean
        // it; a caller that leaves it unset means one.
        quantity: info.quantity.max(1),
    };
    let ducat_core::cbor::Value::Map(m) = n.to_value() else { unreachable!() };
    let seed = ducat_core::board::listing_seed(&persona_secret, &listing_id);
    let beacon = beacon_from(beacon_height, &beacon_hash_hex)?;
    let sealed =
        ducat_core::board::seal(m, ducat_core::board::RENTAL, &seed, &board, subkey, &beacon);
    let bytes = sealed.encode();
    // Encode-then-decode, as the hail does: what goes onto a public board is
    // only ever bytes this implementation would itself accept — which now
    // includes the signature and the work verifying against this very slot.
    rental_decode(bytes.clone(), board, subkey, 0)?;
    Ok(bytes)
}

/// A beacon out of the two values a caller can actually hold.
fn beacon_from(height: u64, hash_hex: &str) -> Result<ducat_core::board::Beacon, ContactError> {
    let raw = crate::hex_to_bytes(hash_hex)
        .ok_or_else(|| ContactError::Refused("that is not a block hash".into()))?;
    let hash: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| ContactError::Refused("a block hash is 32 bytes".into()))?;
    Ok(ducat_core::board::Beacon { height, hash })
}

/// Read a listing off a board, refusing anything unsigned, mis-signed, signed
/// for a different slot, unpaid for, or stamped against a block too old to be
/// the one it claims.
///
/// The slot has to be passed in because it is inside the signature: a board's
/// write key is public, so without that binding a valid notice could be lifted
/// onto every other slot in the cell.
///
/// **`tip_height` is what this device believes the chain height to be, and
/// zero means it does not know.** Reading a board has never needed a Monero
/// node and this does not make it need one: with no chain view the freshness
/// test is skipped and the notice is judged on its signature and its work
/// alone, which is what it was judged on before the beacon existed. A
/// marketplace that goes dark because a daemon is unreachable would be a worse
/// answer than the spam it was avoiding, and an attacker cannot choose which
/// readers have a node.
///
/// What passes here is the *cheap* half. The height is inside the signature so
/// it cannot be moved, but the hash beside it is only bytes until somebody
/// compares them to a real block — see `board::beacon_in_window`. The returned
/// `beacon_height`/`beacon_hash` are for the caller that does.
#[uniffi::export]
pub fn rental_decode(
    bytes: Vec<u8>,
    board: String,
    subkey: u32,
    tip_height: u64,
) -> Result<RentalInfo, ContactError> {
    let o = ducat_core::board::open(
        decode(&bytes).map_err(refuse)?,
        ducat_core::board::RENTAL,
        &board,
        subkey,
    )
    .map_err(refuse)?;
    if tip_height > 0 && !ducat_core::board::beacon_in_window(&o.beacon, tip_height) {
        return Err(ContactError::Refused(
            "this notice was stamped against a block that is not recent".into(),
        ));
    }
    let n = ducat_core::contact::RentalNotice::from_value(o.notice).map_err(refuse)?;
    let mut info = rental_from_core(n);
    info.poster = o.poster.iter().map(|b| format!("{b:02x}")).collect::<String>();
    info.beacon_height = o.beacon.height;
    info.beacon_hash = o.beacon.hash.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(info)
}

/// §16.18.2 over the bridge: what a publication listing says, in the open.
#[derive(uniffi::Record)]
pub struct PubListingInfo {
    pub card: String,
    pub title: String,
    pub blurb: Option<String>,
    /// Piconero a period; `None` is free, and the only spelling of it.
    pub price_pxmr: Option<u64>,
    pub expiry: u64,
    /// Filled by decode: hex of the listing's own verifying key.
    pub poster: String,
    pub beacon_height: u64,
    pub beacon_hash: String,
}

/// §16.22: a site head across the bridge.
#[derive(uniffi::Record, Clone)]
pub struct SiteHeadIo {
    pub title: String,
    pub share: String,
    pub digest_hex: String,
    pub updated: u64,
}

/// Encode a site head for the record's subkey 0.
#[uniffi::export]
pub fn site_head_encode(head: SiteHeadIo) -> Result<Vec<u8>, ContactError> {
    let digest: [u8; 32] = crate::hex_to_bytes(&head.digest_hex)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ContactError::Refused("digest is 64 hex chars".into()))?;
    let h = ducat_core::contact::SiteHead {
        version: 1,
        title: head.title,
        share: head.share,
        digest,
        updated: head.updated,
    };
    // Encode-then-decode self-check, like every writer here: what we
    // publish must be what a strict reader accepts.
    let bytes = h.to_value().encode();
    ducat_core::contact::SiteHead::from_value(
        ducat_core::cbor::decode(&bytes)
            .map_err(|e| ContactError::Refused(format!("self-check: {e:?}")))?,
    )
    .map_err(|e| ContactError::Refused(format!("self-check: {e:?}")))?;
    Ok(bytes)
}

/// Decode a site head read from a record. Strict: whatever a stranger
/// wrote is checked at the door.
#[uniffi::export]
pub fn site_head_decode(bytes: Vec<u8>) -> Result<SiteHeadIo, ContactError> {
    let v = ducat_core::cbor::decode(&bytes)
        .map_err(|e| ContactError::Refused(format!("{e:?}")))?;
    let h = ducat_core::contact::SiteHead::from_value(v)
        .map_err(|e| ContactError::Refused(format!("{e:?}")))?;
    Ok(SiteHeadIo {
        title: h.title,
        share: h.share,
        digest_hex: h.digest.iter().map(|b| format!("{b:02x}")).collect(),
        updated: h.updated,
    })
}

/// Seal a publication listing for one slot — same stamp, same price, same
/// rules as a rental's, in this family's own field namespace. The board
/// name carries the category (topic:) or the cell (local:), and it is
/// inside the signature, so the same bytes cannot appear on another topic.
#[uniffi::export]
pub fn pub_listing_encode(
    info: PubListingInfo,
    persona_secret: Vec<u8>,
    listing_id: String,
    board: String,
    subkey: u32,
    beacon_height: u64,
    beacon_hash_hex: String,
) -> Result<Vec<u8>, ContactError> {
    let n = ducat_core::contact::PubNotice {
        version: 1,
        card: info.card,
        title: info.title,
        blurb: info.blurb,
        price_pxmr: info.price_pxmr,
        expiry: info.expiry,
    };
    let ducat_core::cbor::Value::Map(m) = n.to_value() else { unreachable!() };
    let seed = ducat_core::board::listing_seed(&persona_secret, &listing_id);
    let beacon = beacon_from(beacon_height, &beacon_hash_hex)?;
    let sealed =
        ducat_core::board::seal(m, ducat_core::board::PUB, &seed, &board, subkey, &beacon);
    let bytes = sealed.encode();
    pub_listing_decode(bytes.clone(), board, subkey, 0)?;
    Ok(bytes)
}

/// Read a publication listing off a board — same refusals as a rental's.
#[uniffi::export]
pub fn pub_listing_decode(
    bytes: Vec<u8>,
    board: String,
    subkey: u32,
    tip_height: u64,
) -> Result<PubListingInfo, ContactError> {
    let o = ducat_core::board::open(
        decode(&bytes).map_err(refuse)?,
        ducat_core::board::PUB,
        &board,
        subkey,
    )
    .map_err(refuse)?;
    if tip_height > 0 && !ducat_core::board::beacon_in_window(&o.beacon, tip_height) {
        return Err(ContactError::Refused(
            "this notice was stamped against a block that is not recent".into(),
        ));
    }
    let n = ducat_core::contact::PubNotice::from_value(o.notice).map_err(refuse)?;
    Ok(PubListingInfo {
        card: n.card,
        title: n.title,
        blurb: n.blurb,
        price_pxmr: n.price_pxmr,
        expiry: n.expiry,
        poster: o.poster.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        beacon_height: o.beacon.height,
        beacon_hash: o.beacon.hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    })
}

/// The hail's half of the same thing — see [`rental_encode`].
#[uniffi::export]
pub fn hail_encode(
    info: HailInfo,
    persona_secret: Vec<u8>,
    hail_id: String,
    board: String,
    subkey: u32,
    beacon_height: u64,
    beacon_hash_hex: String,
) -> Result<Vec<u8>, ContactError> {
    let n = ducat_core::contact::HailNotice {
        version: 2,
        card: info.card,
        dest: info.dest,
        fare_pxmr: info.fare_pxmr,
        expiry: info.expiry,
        origin_cell: info.origin_cell,
        dest_cell: info.dest_cell,
    };
    let ducat_core::cbor::Value::Map(m) = n.to_value() else { unreachable!() };
    let seed = ducat_core::board::listing_seed(&persona_secret, &hail_id);
    let beacon = beacon_from(beacon_height, &beacon_hash_hex)?;
    let sealed =
        ducat_core::board::seal(m, ducat_core::board::HAIL, &seed, &board, subkey, &beacon);
    let bytes = sealed.encode();
    // Encode-then-decode: what goes onto a public board is only ever bytes
    // this implementation would itself accept.
    hail_decode(bytes.clone(), board, subkey, 0)?;
    Ok(bytes)
}

/// The hail's half of the same thing — see [`rental_decode`].
#[uniffi::export]
pub fn hail_decode(
    bytes: Vec<u8>,
    board: String,
    subkey: u32,
    tip_height: u64,
) -> Result<HailInfo, ContactError> {
    let o = ducat_core::board::open(
        decode(&bytes).map_err(refuse)?,
        ducat_core::board::HAIL,
        &board,
        subkey,
    )
    .map_err(refuse)?;
    if tip_height > 0 && !ducat_core::board::beacon_in_window(&o.beacon, tip_height) {
        return Err(ContactError::Refused(
            "this notice was stamped against a block that is not recent".into(),
        ));
    }
    let n = ducat_core::contact::HailNotice::from_value(o.notice).map_err(refuse)?;
    Ok(HailInfo {
        poster: o.poster.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        beacon_height: o.beacon.height,
        beacon_hash: o.beacon.hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        card: n.card,
        dest: n.dest,
        fare_pxmr: n.fare_pxmr,
        expiry: n.expiry,
        origin_cell: n.origin_cell,
        dest_cell: n.dest_cell,
    })
}

/// §15.12's geocells, straight from core so the phone and the vectors agree
/// on every boundary.
#[uniffi::export]
pub fn geohashEncode(lat_e7: i64, lon_e7: i64, precision: u32) -> Result<String, ContactError> {
    ducat_core::geo::geohash_encode(lat_e7, lon_e7, precision).map_err(refuse)
}

#[uniffi::export]
pub fn geohashNeighbors(cell: String) -> Result<Vec<String>, ContactError> {
    ducat_core::geo::geohash_neighbors(&cell).map_err(refuse)
}

/// §15.12's overflow ladder: the board name for one shard of a stand.
#[uniffi::export]
pub fn standShardName(base: String, shard: u32) -> Result<String, ContactError> {
    ducat_core::geo::stand_shard_name(&base, shard).map_err(refuse)
}

/// The longest a board notice may claim to be good for.
///
/// A reader's question, not a decoder's: decoding must not depend on the
/// clock, or the conformance vectors would start failing on their own one day.
/// Every sweep already drops a notice whose expiry has passed; this is the
/// other end of the same test, and without it one payment of proof-of-work
/// buys a slot for ever. See `MAX_NOTICE_TTL_SECS`.
#[uniffi::export]
pub fn maxNoticeTtlSecs() -> u64 {
    ducat_core::contact::MAX_NOTICE_TTL_SECS
}

/// How tall a ladder may grow — readers sweep until an empty shard or this.
#[uniffi::export]
pub fn maxStandShards() -> u32 {
    ducat_core::geo::MAX_STAND_SHARDS
}

/// §15.12's generation: which board a cell's notices live on right now.
///
/// The caller supplies the clock, because the epoch decides a *name* and a
/// name that moved under a decoder would make every vector time-dependent.
#[uniffi::export]
pub fn standEpoch(nowSecs: u64) -> u64 {
    ducat_core::geo::stand_epoch(nowSecs)
}

/// The board name for one generation of a stand — `<base>@<epoch>`.
///
/// Applied before the shard suffix, so a full name reads `geo:u4pruy@3021-3`.
#[uniffi::export]
pub fn standEpochName(base: String, epoch: u64) -> Result<String, ContactError> {
    ducat_core::geo::stand_epoch_name(&base, epoch).map_err(refuse)
}

/// How long one generation lasts, so a client can tell how stale a board it
/// last wrote to has become without recomputing the rule.
#[uniffi::export]
pub fn standEpochSecs() -> u64 {
    ducat_core::geo::STAND_EPOCH_SECS
}

#[uniffi::export]
pub fn geohashCenter(cell: String) -> Result<Vec<i64>, ContactError> {
    let (lat, lon) = ducat_core::geo::geohash_center(&cell).map_err(refuse)?;
    Ok(vec![lat, lon])
}

#[uniffi::export]
pub fn geohashBounds(cell: String) -> Result<Vec<i64>, ContactError> {
    let (a, b, c, d) = ducat_core::geo::geohash_bounds(&cell).map_err(refuse)?;
    Ok(vec![a, b, c, d])
}

#[uniffi::export]
pub fn haversineM(lat1_e7: i64, lon1_e7: i64, lat2_e7: i64, lon2_e7: i64) -> u64 {
    ducat_core::geo::haversine_m(lat1_e7, lon1_e7, lat2_e7, lon2_e7)
}

/// Strip what the wire will refuse, so a sender never publishes something
/// every reader silently drops.
///
/// Exposed rather than reimplemented on the Kotlin side: the stripper and the
/// wire's own check have to agree on exactly which characters they are about,
/// and two copies of that list drift. Miss one and the message leaves the
/// phone and vanishes at the far end after the slot is already spent; take one
/// the wire allows and honest Arabic and Hebrew quietly lose their typography.
#[uniffi::export]
pub fn clean_display_text(text: String) -> String {
    ducat_core::wire::without_display_hazards(&text)
}
