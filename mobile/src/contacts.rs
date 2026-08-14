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
        },
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
    pub record_key: String,
    pub key: Vec<u8>,
    pub nonce: Vec<u8>,
    pub len: u64,
    pub ct_hash: Vec<u8>,
    pub mime: String,
    pub name: Option<String>,
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
    let msg = Message {
        version: 1, suite: 1, seq, prev, body, timestamp: now(),
        kind: match kind {
            1 => MessageKind::PaymentRequest,
            2 => MessageKind::PaymentSent,
            3 => MessageKind::Receipt,
            4 => MessageKind::Reaction,
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
        attachment: attachment.map(|a| ducat_core::contact::Attachment {
            record_key: a.record_key,
            key: a.key.try_into().unwrap_or([0u8; 32]),
            nonce: a.nonce.try_into().unwrap_or([0u8; 24]),
            len: a.len,
            ct_hash: a.ct_hash.try_into().unwrap_or([0u8; 32]),
            mime: a.mime,
            name: a.name,
        }),
    };
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
        attachment: msg.attachment.as_ref().map(|a| AttachmentRef {
            record_key: a.record_key.clone(),
            key: a.key.to_vec(),
            nonce: a.nonce.to_vec(),
            len: a.len,
            ct_hash: a.ct_hash.to_vec(),
            mime: a.mime.clone(),
            name: a.name.clone(),
        }),
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
    let want = prev.unwrap_or([0u8; 32]);
    if msg.prev != want {
        return Err(ContactError::Refused(
            "this message does not follow the one before it".into(),
        ));
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

