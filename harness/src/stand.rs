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

/// Open a board by name, pinning it first if nobody has (the convention
/// means any reader holds the keypair to fix KeyNotFound itself).
async fn open_board(
    api: &VeilidAPI,
    rc: &RoutingContext,
    name: &str,
) -> Result<RecordKey, Box<dyn std::error::Error>> {
    let (pk, sk) = cell_keypair(name);
    let enc = SharedSecret::new(CRYPTO_KIND_VLD0, cell_encryption(name));
    let key = api
        .get_dht_record_key(schema(), PublicKey::new(CRYPTO_KIND_VLD0, pk.clone()), Some(enc))
        .await?;
    if rc.open_dht_record(key.clone(), None).await.is_err() {
        let kp = KeyPair::new(CRYPTO_KIND_VLD0, BareKeyPair::new(pk, sk));
        if let Ok(desc) = rc.create_dht_record(CRYPTO_KIND_VLD0, schema(), Some(kp)).await {
            let _ = rc.close_dht_record(desc.key().clone()).await;
        }
        rc.open_dht_record(key.clone(), None).await?;
    }
    Ok(key)
}

/// Is this slot free to write? Empty, or holding debris — anything that is
/// not a live hail notice. Reading it first is also what primes the local
/// value_seq, without which the overwrite is silently refused (§16.12's
/// read-before-write, learned the hard way on the mailbox ring).
fn slot_is_free(data: &[u8], now: u64) -> bool {
    use ducat_core::cbor::decode;
    use ducat_core::contact::HailNotice;
    if data.is_empty() {
        return true;
    }
    match decode(data).ok().and_then(|v| HailNotice::from_value(v).ok()) {
        Some(n) => n.expiry <= now,
        None => true,
    }
}

/// Post a *real* notice — §16.17 bytes, not text. The ladder only respects
/// hail-shaped tenants (text is debris that holds no place), so testing the
/// overflow at all needs the genuine article.
pub async fn post_hail(cell: &str, dest: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::contact::HailNotice;
    let n = HailNotice {
        version: 1,
        card: format!("ducat:card/ladder-test-{}", std::process::id()),
        dest: dest.to_string(),
        fare_pxmr: None,
        expiry: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
            + 600,
        origin_cell: None,
        dest_cell: None,
    };
    post_bytes(cell, &n.to_value().encode()).await
}

pub async fn post(cell: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    post_bytes(cell, text.as_bytes()).await
}

async fn post_bytes(cell: &str, body: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::geo::{stand_shard_name, MAX_STAND_SHARDS};
    println!("\n\x1b[1mDUCAT — stand post (rendezvous by convention)\x1b[0m\n");
    let (api, _c) = crate::veilid::start("stand-p").await?;
    let rc = api.routing_context()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    // §15.12's overflow ladder: the lowest shard with a free slot takes the
    // notice. Backfilling low keeps the ladder compact, which is what keeps
    // a reader's sweep short.
    for shard in 0..MAX_STAND_SHARDS {
        let name = stand_shard_name(cell, shard).map_err(|e| format!("{e:?}"))?;
        let key = open_board(&api, &rc, &name).await?;
        for subkey in 0..8u32 {
            let occupant = rc.get_dht_value(key.clone(), subkey, true).await?;
            if slot_is_free(occupant.as_ref().map(|v| v.data()).unwrap_or(&[]), now) {
                // The board is written with the derived owner key, so reopen
                // as owner for the set.
                let (pk, sk) = cell_keypair(&name);
                let kp = KeyPair::new(CRYPTO_KIND_VLD0, BareKeyPair::new(pk, sk));
                rc.close_dht_record(key.clone()).await?;
                rc.open_dht_record(key.clone(), Some(kp)).await?;
                rc.set_dht_value(key.clone(), subkey, body.to_vec(), None)
                    .await?;
                println!("  posted   {} B at {name:?} slot {subkey}", body.len());
                rc.close_dht_record(key).await?;
                api.shutdown().await;
                return Ok(());
            }
        }
        println!("  {name:?} is full — climbing the ladder");
        rc.close_dht_record(key).await?;
    }
    api.shutdown().await;
    Err("every shard is full — this cell has outgrown itself; use a finer geohash".into())
}

pub async fn read(cell: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::geo::{stand_shard_name, MAX_STAND_SHARDS};
    println!("\n\x1b[1mDUCAT — stand read (key computed, not communicated)\x1b[0m\n");
    let (api, _c) = crate::veilid::start("stand-r").await?;
    let rc = api.routing_context()?;

    let mut found = 0;
    let mut quiet = 0;
    for shard in 0..MAX_STAND_SHARDS {
        let name = stand_shard_name(cell, shard).map_err(|e| format!("{e:?}"))?;
        let key = open_board(&api, &rc, &name).await?;
        if shard == 0 {
            println!("  record   {key}  ← computed locally from the cell name");
        }
        let mut here = 0;
        for subkey in 0..8u32 {
            if let Ok(Some(v)) = rc.get_dht_value(key.clone(), subkey, true).await {
                if v.data().is_empty() { continue }
                here += 1;
                println!("  [{name:?} {subkey}] {} B: {:?}", v.data().len(),
                    String::from_utf8_lossy(&v.data()[..v.data().len().min(60)]));
            }
        }
        rc.close_dht_record(key).await?;
        found += here;
        // Claims and expiry empty the low shards first, so one hole is not
        // the end of the ladder: keep going past a single quiet shard, stop
        // after two in a row. Costs a quiet cell one extra read.
        if here == 0 {
            quiet += 1;
            if quiet >= 2 { break }
        } else {
            quiet = 0;
        }
    }
    if found == 0 {
        println!("  \x1b[31mboard is empty\x1b[0m");
    } else {
        println!("\n  \x1b[32m{found} notice(s) on the ladder\x1b[0m");
    }
    api.shutdown().await;
    Ok(())
}

/// The driver's side of a §15.12 hail, from the desk: watch a stand, print
/// what appears, and claim the first live notice — which drops the two of us
/// into an ordinary thread where `--say` and the rest already work.
pub async fn hail_watch(cell: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::cbor::decode;
    use ducat_core::contact::HailNotice;

    // A geocell means the 3×3 (§15.12): the named cell and its 8 neighbours,
    // because a rider fifty metres over a border is otherwise invisible.
    let cells: Vec<String> = if let Some(gh) = cell.strip_prefix("geo:") {
        let mut v = vec![cell.to_string()];
        v.extend(ducat_core::geo::geohash_neighbors(gh)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .map(|n| format!("geo:{n}")));
        v
    } else {
        vec![cell.to_string()]
    };
    if cells.len() > 1 {
        println!("\n\x1b[1mDUCAT — driving the 3×3 around {cell:?}\x1b[0m\n");
        return hail_watch_cells(&cells).await;
    }
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

/// The 3×3 watch: nine boards, ensured pinned, polled round-robin.
async fn hail_watch_cells(cells: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use ducat_core::cbor::decode;
    use ducat_core::contact::HailNotice;
    use ducat_core::geo::{stand_shard_name, MAX_STAND_SHARDS};

    let (api, _c) = crate::veilid::start("drive").await?;
    let rc = api.routing_context()?;
    // Boards open lazily as the ladder is climbed: 9 cells × 16 shards eagerly
    // would be 144 records for what is almost always 9.
    let mut opened: std::collections::HashMap<String, RecordKey> = Default::default();

    let now = || std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    loop {
        for cell in cells {
            // §15.12's overflow ladder: sweep from shard 0. Claims and
            // expiry empty the low shards first, so one quiet shard is not
            // the end of the ladder — stop only after two in a row, which
            // costs a quiet cell one extra read.
            let mut quiet = 0u32;
            'ladder: for shard in 0..MAX_STAND_SHARDS {
                let name = stand_shard_name(cell, shard).map_err(|e| format!("{e:?}"))?;
                let key = match opened.get(&name) {
                    Some(k) => k.clone(),
                    None => {
                        let k = open_board(&api, &rc, &name).await?;
                        if shard == 0 {
                            println!("  board {} = {}", name, k);
                        }
                        opened.insert(name.clone(), k.clone());
                        k
                    }
                };
                // All eight slots at once: sequential force-refreshed reads
                // made a 3x3 two-shard pass 144 round trips — half an hour,
                // against notices that live fifteen minutes. Concurrency is
                // not an optimization here; it is what makes the sweep exist.
                let mut fetches = Vec::new();
                for subkey in 0..8u32 {
                    let rc2 = rc.clone();
                    let key2 = key.clone();
                    fetches.push(tokio::spawn(async move {
                        (subkey, rc2.get_dht_value(key2, subkey, true).await)
                    }));
                }
                let mut slots = Vec::new();
                for f in fetches {
                    if let Ok((subkey, Ok(Some(v)))) = f.await {
                        slots.push((subkey, v.data().to_vec()));
                    }
                }
                let mut live_here = 0u32;
                for (subkey, bytes) in slots {
                    if bytes.is_empty() { continue }
                    let Ok(val) = decode(&bytes) else { continue };
                    let Ok(n) = HailNotice::from_value(val) else { continue };
                    if n.expiry <= now() { continue }
                    live_here += 1;
                    println!(
                        "  [{name} slot {subkey}] to {:?} ({}), {}, stands {}s",
                        n.dest,
                        n.dest_cell.as_deref().unwrap_or("no cell"),
                        n.fare_pxmr.map(|f| format!("offers {f} pXMR"))
                            .unwrap_or_else(|| "quote me".into()),
                        n.expiry - now(),
                    );
                    println!("\n  taking it — claiming the card…");
                    for k in opened.values() { let _ = rc.close_dht_record(k.clone()).await; }
                    api.shutdown().await;
                    crate::mailbox::claim(&n.card).await?;
                    println!("\n  \x1b[32mhail taken\x1b[0m — talk with --say");
                    return Ok(());
                }
                if live_here == 0 {
                    quiet += 1;
                    if quiet >= 2 { break 'ladder }
                } else {
                    quiet = 0;
                }
            }
        }
        print!(".");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    }
}
