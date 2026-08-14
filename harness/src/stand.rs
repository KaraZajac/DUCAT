//! Rendezvous by convention: two strangers, one place name, no exchange.
//!
//! Everything in DUCAT so far connects people who *met* — a card crossed a
//! table, a QR crossed a screen. Dispatch (§15.11's taxi grown into a hail)
//! needs the one thing that model refuses: a rider and a driver who have
//! never met converging on the same DHT record with nothing in common but
//! *where they are*.
//!
//! Veilid makes this possible without a directory, and the mechanism is worth
//! stating precisely because it looks like magic until it doesn't:
//! `get_dht_record_key(schema, owner_key, _)` computes a record key **locally**
//! from an owner public key. So a keypair derived deterministically from a
//! public string — `seed = SHA-256("DUCAT-STAND-v0" ‖ cell)` — gives everyone
//! who knows the string the same record key, and the DHT becomes a map from
//! *names* to *bulletin boards*. A geocell string is such a name. So is
//! "the taxi rank at the airport".
//!
//! The cost is stated with the trick: the seed is public, therefore the
//! *secret* is public, therefore **anyone can write or vandalise the board**.
//! This is a bulletin board in the honest sense — pinned in a public square,
//! erasable by anyone with hands. What keeps it useful is what keeps a real
//! one useful: notices are small (a card URI and a coarse area), stale ones
//! expire, everything of value moves immediately into a claimed card's sealed
//! thread, and a wiped board is re-pinned by the next person who needs it.
//! What it must never carry: precise locations, identities, anything worth
//! scraping. The board is a place to say "someone here wants a ride", and
//! nothing else.
//!
//!   ducat-harness --stand-post <cell> <text>    derive, create/open, write
//!   ducat-harness --stand-read <cell>           derive, compute key, read
//!
//! The proof is that `--stand-read` is handed *only the cell string* — the
//! record key it reads from is computed, not communicated.

use sha2::{Digest, Sha256};
use veilid_core::*;

/// The convention. Version pinned in the string so a future scheme change
/// lands on different records rather than fighting over these.
fn cell_keypair(cell: &str) -> (BarePublicKey, BareSecretKey) {
    let seed: [u8; 32] = Sha256::new()
        .chain_update(b"DUCAT-STAND-v0")
        .chain_update([0u8])
        .chain_update(cell.as_bytes())
        .finalize()
        .into();
    let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
    let pk = sk.verifying_key();
    (
        BarePublicKey::new(pk.as_bytes()),
        BareSecretKey::new(&seed),
    )
}

/// The other half of the convention, learned from the source: veilid 0.5
/// encrypts every record's values, and the key rides in the RecordKey
/// *handle*, never on the network. `create` always draws a random one — so a
/// public board derives its encryption key from the cell name too, both
/// sides construct the same full key locally, and the create-time key is
/// simply never used. (The values are then "encrypted" under a public
/// secret, which is to say: public, which is the point of a board.)
fn cell_encryption(cell: &str) -> BareSharedSecret {
    let k: [u8; 32] = Sha256::new()
        .chain_update(b"DUCAT-STAND-v0-ENC")
        .chain_update([0u8])
        .chain_update(cell.as_bytes())
        .finalize()
        .into();
    BareSharedSecret::new(&k)
}

fn schema() -> DHTSchema {
    // Eight notice slots, exactly as §15.12 pins it. The schema is part of
    // the record-key derivation, so this is not a tunable: a board created
    // with a different slot count is a *different record*, and two parties
    // disagreeing on it would each stare at their own empty board.
    DHTSchema::dflt(8).expect("static schema")
}

pub async fn post(cell: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — stand post (rendezvous by convention)\x1b[0m\n");
    let (pk, sk) = cell_keypair(cell);
    println!("  cell     {cell:?}");
    println!("  owner    {pk} (derived from the cell name, not generated)");

    let (api, _c) = crate::veilid::start("stand-p").await?;
    let rc = api.routing_context()?;
    let kp = KeyPair::new(CRYPTO_KIND_VLD0, BareKeyPair::new(pk, sk));
    let enc = SharedSecret::new(CRYPTO_KIND_VLD0, cell_encryption(cell));

    // The conventional key: opaque part from the owner key, encryption part
    // from the cell name. Both computed locally; nothing exchanged.
    let key = api
        .get_dht_record_key(schema(), kp.key(), Some(enc))
        .await?;

    // Create publishes the descriptor (schema + owner). Its handle carries a
    // random encryption key we deliberately never use: close, and reopen
    // under the conventional key, so what we write decrypts for anyone who
    // can derive it. If the board already exists, create fails and the open
    // is all that was needed anyway.
    if let Ok(desc) = rc
        .create_dht_record(CRYPTO_KIND_VLD0, schema(), Some(kp.clone()))
        .await
    {
        rc.close_dht_record(desc.key().clone()).await?;
    }
    rc.open_dht_record(key.clone(), Some(kp)).await?;
    println!("  record   {key}");

    rc.set_dht_value(key.clone(), 0, text.as_bytes().to_vec(), None)
        .await?;
    println!("  posted   {} B", text.len());
    println!(
        "\n  now, from anyone who knows only the cell name:\n    \
         cargo run -q -p ducat-harness -- --stand-read {cell:?}\n"
    );
    rc.close_dht_record(key).await?;
    api.shutdown().await;
    Ok(())
}

pub async fn read(cell: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — stand read (key computed, not communicated)\x1b[0m\n");
    let (pk, _) = cell_keypair(cell);
    println!("  cell     {cell:?}");

    let (api, _c) = crate::veilid::start("stand-r").await?;
    let rc = api.routing_context()?;

    let enc = SharedSecret::new(CRYPTO_KIND_VLD0, cell_encryption(cell));
    let key = api
        .get_dht_record_key(schema(), PublicKey::new(CRYPTO_KIND_VLD0, pk), Some(enc))
        .await?;
    println!("  record   {key}  ← computed locally from the cell name");

    rc.open_dht_record(key.clone(), None).await?;
    let mut found = 0;
    for subkey in 0..8u32 {
        if let Ok(Some(v)) = rc.get_dht_value(key.clone(), subkey, true).await {
            if v.data().is_empty() { continue }
            found += 1;
            println!("  [{subkey}] {} B: {:?}", v.data().len(),
                String::from_utf8_lossy(&v.data()[..v.data().len().min(60)]));
        }
    }
    if found == 0 {
        println!("  \x1b[31mboard is empty\x1b[0m");
    } else {
        println!("\n  \x1b[32m{found} notice(s) on the board\x1b[0m");
    }
    rc.close_dht_record(key).await?;
    api.shutdown().await;
    Ok(())
}

/// The driver's side of a §15.12 hail, from the desk: watch a stand, print
/// what appears, and claim the first live notice — which drops the two of us
/// into an ordinary thread where `--say` and the rest already work.
pub async fn hail_watch(cell: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::cbor::decode;
    use ducat_core::contact::HailNotice;

    println!("\n\x1b[1mDUCAT — driving at {cell:?}\x1b[0m\n");
    let (pk, sk) = cell_keypair(cell);
    let enc = SharedSecret::new(CRYPTO_KIND_VLD0, cell_encryption(cell));
    let (api, _c) = crate::veilid::start("drive").await?;
    let rc = api.routing_context()?;
    let key = api
        .get_dht_record_key(schema(), PublicKey::new(CRYPTO_KIND_VLD0, pk.clone()), Some(enc))
        .await?;
    println!("  board    {key}");
    // First to the corner pins the board (learned live: a driver arriving
    // before any rider got KeyNotFound and went home).
    if rc.open_dht_record(key.clone(), None).await.is_err() {
        let kp = KeyPair::new(CRYPTO_KIND_VLD0, BareKeyPair::new(pk, sk));
        if let Ok(desc) = rc.create_dht_record(CRYPTO_KIND_VLD0, schema(), Some(kp)).await {
            let _ = rc.close_dht_record(desc.key().clone()).await;
        }
        rc.open_dht_record(key.clone(), None).await?;
        println!("  (board was unpinned — pinned it)");
    }

    let now = || std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    loop {
        for subkey in 0..8u32 {
            let Ok(Some(v)) = rc.get_dht_value(key.clone(), subkey, true).await else {
                continue;
            };
            if v.data().is_empty() {
                continue;
            }
            let Ok(val) = decode(v.data()) else { continue };
            let Ok(n) = HailNotice::from_value(val) else {
                println!("  [{subkey}] undecodable notice — ignored");
                continue;
            };
            if n.expiry <= now() {
                continue;
            }
            println!("  [{subkey}] to {:?}, {}, stands {}s",
                n.dest,
                n.fare_pxmr.map(|f| format!("offers {} pXMR", f))
                    .unwrap_or_else(|| "quote me".into()),
                n.expiry - now());
            println!("\n  taking it — claiming the card…");
            rc.close_dht_record(key.clone()).await?;
            api.shutdown().await;
            // The claim is the existing card machinery, unchanged.
            crate::mailbox::claim(&n.card).await?;
            println!("\n  \x1b[32mhail taken\x1b[0m — talk with --say");
            return Ok(());
        }
        print!(".");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Which prekey id each of our sent messages was sealed to — the envelope is
/// plaintext CBOR, so our own outbox answers without any secrets.
pub async fn peek_seals() -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::cbor::decode;
    use ducat_core::hpke::SealedMessage;
    use std::str::FromStr;
    let st = std::fs::read_to_string(crate::mailbox::state_path_pub("claimant"))?;
    let my_log = st.lines().nth(2).ok_or("no outbox in state")?.to_string();
    let (api, _c) = crate::veilid::start("peek").await?;
    let rc = api.routing_context()?;
    let log = RecordKey::from_str(&my_log)?;
    rc.open_dht_record(log.clone(), None).await?;
    for subkey in 1..8u32 {
        if let Ok(Some(v)) = rc.get_dht_value(log.clone(), subkey, true).await {
            if let Ok(sealed) = SealedMessage::from_value(decode(v.data()).map_err(|e| format!("{e:?}"))?) {
                println!("  slot {subkey}: sealed to prekey id {}", sealed.prekey_id);
            }
        }
    }
    rc.close_dht_record(log).await?;
    api.shutdown().await;
    Ok(())
}
