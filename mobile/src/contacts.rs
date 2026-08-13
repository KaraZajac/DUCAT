//! Contact cards, prekeys and sealed messages, across the bridge (§16.9–§16.11).
//!
//! Kotlin gets bytes and structs; every protocol decision stays in `core`, which
//! is the same rule the rest of this bridge follows. A display name is checked
//! against §16.9's bound *here* rather than in Compose, because a bound enforced
//! in the UI is a bound a second UI forgets.

use ducat_core::cbor::decode;
use ducat_core::contact::{
    check_claim, check_message, ContactClaim, ContactInvite, Message, MAX_DISPLAY_NAME_CHARS,
    MAX_MESSAGE_CHARS,
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

/// A card ready to hand over, in every form the UI needs at once.
#[derive(uniffi::Record, Clone)]
pub struct IssuedCard {
    /// The signed envelope. What NFC transfers verbatim.
    pub bytes: Vec<u8>,
    /// The same bytes as a `ducat:` URI (§18.7) — what a QR encodes and what
    /// pastes into a message to a friend.
    pub uri: String,
    /// Held by the issuer to recognise the claim. Never leaves the device.
    pub claim_secret: Vec<u8>,
    /// Stored so a second claim can be refused (§16.9). Single use is a property
    /// of a store that outlives the call, not of a function.
    pub claim_commit: Vec<u8>,
    pub expiry: u64,
}

/// Mint a contact card for this persona.
///
/// `valid_secs` is the caller's, because how long a card should live depends on
/// how it is being handed over: seconds across a table, hours if it is going
/// into a message someone reads tomorrow.
#[uniffi::export]
pub fn create_contact_card(
    persona_secret: Vec<u8>,
    rendezvous: Vec<u8>,
    display_name: Option<String>,
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
    let sk = persona_key(&persona_secret)?;
    let secret = random32();
    let expiry = now() + valid_secs;
    let invite = ContactInvite {
        version: 1,
        suite: 1,
        persona: sk.public().to_bytes().to_vec(),
        rendezvous,
        display_name,
        claim_commit: hpke_commit(&secret),
        expiry,
    };
    let bytes = seal_env(
        &SignedBytes::from_received(invite.to_value().encode()).map_err(refuse)?,
        ObjectType::ContactOffer,
        &sk,
    );
    Ok(IssuedCard {
        uri: ducat_core::contact::card_to_uri(&bytes, &secret),
        bytes,
        claim_secret: secret.to_vec(),
        claim_commit: invite.claim_commit.to_vec(),
        expiry,
    })
}

/// What the UI shows before someone decides to add a contact.
#[derive(uniffi::Record, Clone)]
pub struct ScannedCard {
    pub persona: Vec<u8>,
    pub rendezvous: Vec<u8>,
    /// Self-asserted. §16.9 requires this be shown as unverified, and the
    /// petname the user assigns is the name that is actually displayed later.
    pub asserted_name: Option<String>,
    pub claim_secret: Vec<u8>,
    pub expiry: u64,
    pub expired: bool,
}

/// Read a card that arrived by NFC, QR or a pasted `ducat:` URI.
///
/// The signature check proves the persona key made this card. It does **not**
/// prove the person who sent it holds that key — §16.9 is explicit that the
/// carrying channel supplies that, and the UI must not imply otherwise.
#[uniffi::export]
pub fn read_contact_card(input: String) -> Result<ScannedCard, ContactError> {
    let (env, secret) = ducat_core::contact::card_from_uri(&input).map_err(refuse)?;
    let invite = verify_card(&env)?;
    Ok(ScannedCard {
        persona: invite.persona.clone(),
        rendezvous: invite.rendezvous.clone(),
        asserted_name: invite.display_name.clone(),
        claim_secret: secret.to_vec(),
        expiry: invite.expiry,
        expired: now() > invite.expiry,
    })
}

/// Read a card that arrived as raw bytes (NFC), where there is no URI to carry
/// the claim secret, so it is supplied alongside.
#[uniffi::export]
pub fn read_contact_card_bytes(
    envelope: Vec<u8>,
    claim_secret: Vec<u8>,
) -> Result<ScannedCard, ContactError> {
    let invite = verify_card(&envelope)?;
    Ok(ScannedCard {
        persona: invite.persona.clone(),
        rendezvous: invite.rendezvous.clone(),
        asserted_name: invite.display_name.clone(),
        claim_secret,
        expiry: invite.expiry,
        expired: now() > invite.expiry,
    })
}

/// Build the claim to send back over the card's rendezvous.
#[uniffi::export]
pub fn build_claim(
    persona_secret: Vec<u8>,
    rendezvous: Vec<u8>,
    display_name: Option<String>,
    claim_secret: Vec<u8>,
) -> Result<Vec<u8>, ContactError> {
    let sk = persona_key(&persona_secret)?;
    let secret: [u8; 32] = claim_secret
        .try_into()
        .map_err(|_| ContactError::Refused("claim secret is not 32 bytes".into()))?;
    Ok(ContactClaim {
        version: 1,
        suite: 1,
        persona: sk.public().to_bytes().to_vec(),
        rendezvous,
        display_name,
        claim_secret: secret,
        timestamp: now(),
    }
    .to_value()
    .encode())
}

/// Decide whether an inbound claim on one of our cards may be honoured.
#[uniffi::export]
pub fn check_inbound_claim(
    card_bytes: Vec<u8>,
    claim_bytes: Vec<u8>,
    already_claimed: bool,
) -> Result<ScannedCard, ContactError> {
    let invite = verify_card(&card_bytes)?;
    let claim = ContactClaim::from_value(decode(&claim_bytes).map_err(refuse)?).map_err(refuse)?;
    check_claim(&invite, &claim, now(), already_claimed).map_err(refuse)?;
    Ok(ScannedCard {
        persona: claim.persona,
        rendezvous: claim.rendezvous,
        asserted_name: claim.display_name,
        claim_secret: Vec::new(),
        expiry: invite.expiry,
        expired: false,
    })
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
pub fn generate_prekeys(count: u32, valid_secs: u64) -> PrekeyMaterial {
    let (signed_secret, signed_public) = hpke::derive_keypair(&random32());
    let mut one_time = Vec::new();
    let mut secrets = Vec::new();
    let mut ids = Vec::new();
    for i in 1..=count {
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

/// Seal one message in a thread.
#[uniffi::export]
pub fn seal_message(
    bundle_bytes: Vec<u8>,
    seq: u64,
    prev_link: Vec<u8>,
    body: String,
    thread_aad: Vec<u8>,
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
    let msg = Message { version: 1, suite: 1, seq, prev, body, timestamp: now() };
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

fn verify_card(envelope: &[u8]) -> Result<ContactInvite, ContactError> {
    // The persona is inside the object the signature covers, so the key is read
    // from the payload and then checked against it. A card that verifies under
    // a key other than the one it names is a card claiming someone else's
    // identity, which is the whole point of checking.
    let peek = decode_invite_unverified(envelope)?;
    let pk = PublicKey::from_bytes(Suite::Ed25519X25519, &peek.persona).map_err(refuse)?;
    let (ty, body) = open_env(envelope, &pk).map_err(refuse)?;
    if ty != ObjectType::ContactOffer {
        return Err(ContactError::Refused("not a contact card".into()));
    }
    ContactInvite::from_value(decode(body.bytes()).map_err(refuse)?).map_err(refuse)
}

/// Read the payload without checking the signature, only to learn which key to
/// check it with. Never returned to a caller.
fn decode_invite_unverified(envelope: &[u8]) -> Result<ContactInvite, ContactError> {
    let body = ducat_core::wire::peek_body(envelope).map_err(refuse)?;
    ContactInvite::from_value(decode(&body).map_err(refuse)?).map_err(refuse)
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

fn hpke_commit(secret: &[u8; 32]) -> [u8; 32] {
    ducat_core::contact::claim_commitment(secret)
}
