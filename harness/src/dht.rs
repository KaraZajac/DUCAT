//! Does a DHT record written by one node read back on another? (§16.12)
//!
//! Everything the mailbox rewrite depends on rests on this one property, and it
//! is worth proving on its own before any protocol is layered over it. The
//! failure we are replacing — routes that die with the process — was invisible
//! until two real nodes tried to use it, so this is checked the same way.
//!
//!   ducat-harness --dht-write            creates a record, writes, prints the key
//!   ducat-harness --dht-read <key>       a different node reads it back

use std::time::Instant;

use veilid_core::*;

pub async fn write() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — DHT record write\x1b[0m\n");
    let (api, _calls) = crate::veilid::start("dht-w").await?;
    let rc = api.routing_context()?;

    let t0 = Instant::now();
    let schema = DHTSchema::dflt(8)?;
    let desc = rc.create_dht_record(CRYPTO_KIND_VLD0, schema, None).await?;
    println!("  created  in {} ms", t0.elapsed().as_millis());
    println!("  key      {}", desc.key());

    // Subkey 0 is the head in the real design; here it just carries a payload
    // so the reader has something to compare.
    let payload = b"ducat dht mailbox proof";
    let t1 = Instant::now();
    rc.set_dht_value(desc.key().clone(), 0, payload.to_vec(), None).await?;
    println!("  wrote    {} B in {} ms", payload.len(), t1.elapsed().as_millis());

    // Written and then *left*: the whole point is that the reader does not need
    // this node present. Closing the record before exiting is what a real
    // client does when a conversation goes idle.
    rc.close_dht_record(desc.key().clone()).await?;
    println!("\n  read it back with:\n    cargo run -q -p ducat-harness -- --dht-read {}\n", desc.key());

    api.shutdown().await;
    Ok(())
}

pub async fn read(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — DHT record read (different node)\x1b[0m\n");
    let rk = RecordKey::from_str(key).map_err(|e| format!("bad key: {e}"))?;

    let (api, _calls) = crate::veilid::start("dht-r").await?;
    let rc = api.routing_context()?;

    let t0 = Instant::now();
    rc.open_dht_record(rk.clone(), None).await?;
    println!("  opened   in {} ms", t0.elapsed().as_millis());

    // force_refresh, because the point is to reach the network rather than a
    // local copy this node has no reason to hold.
    let t1 = Instant::now();
    let got = rc.get_dht_value(rk.clone(), 0, true).await?;
    match got {
        Some(v) => {
            println!("  read     {} B in {} ms", v.data().len(), t1.elapsed().as_millis());
            println!("  content  {:?}", String::from_utf8_lossy(v.data()));
            println!("\n  \x1b[32mthe record survived the writer leaving\x1b[0m");
        }
        None => {
            println!("  \x1b[31mno value at subkey 0\x1b[0m");
            return Err("record read returned nothing".into());
        }
    }
    rc.close_dht_record(rk).await?;
    api.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// The contact-request inbox (§16.12), end to end across three processes.
// ---------------------------------------------------------------------------
//
// This mirrors `node_dht_create_shared` in the bridge exactly. That function
// compiles and has never run: an SMPL schema is validated by veilid at create
// time and a writer's permission is enforced at set time, so neither the type
// checker nor a single-node test can tell us it is right. The card rewrite
// rests on this handshake, and finding out on a phone costs a round trip per
// attempt.
//
//   --inbox-create                            issuer: make the inbox, write subkey 0
//   --inbox-reply <key> <wpub> <wsec>         claimant: read 0, write 1
//   --inbox-collect <key>                     issuer: read the reply

pub async fn inbox_create() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n\x1b[1mDUCAT — contact inbox: create\x1b[0m\n");
    let (api, _c) = crate::veilid::start("inbox-c").await?;
    let rc = api.routing_context()?;

    // The writer keypair travels in the card. Whoever holds the card can answer
    // in subkey 1 and nobody else can, which is what makes a claim single-use
    // without needing either party online at the same moment.
    let all_crypto = api.crypto()?;
    let crypto = all_crypto.get(CRYPTO_KIND_VLD0).ok_or("no VLD0")?;
    let writer = crypto.generate_keypair();
    let member = BareMemberId::new(&writer.value().key().bytes());

    let schema = DHTSchema::smpl(1, vec![DHTSchemaSMPLMember { m_key: member, m_cnt: 1 }])?;
    let desc = rc.create_dht_record(CRYPTO_KIND_VLD0, schema, None).await?;
    println!("  key      {}", desc.key());
    println!("  subkeys  0 = ours (owner), 1 = theirs (writer)");

    rc.set_dht_value(desc.key().clone(), 0, b"issuer: persona + outbox key".to_vec(), None)
        .await?;
    println!("  wrote    subkey 0");

    let wk = hex::encode(writer.value().key().bytes());
    let ws = hex::encode(writer.value().secret().bytes());
    println!("\n  claimant runs:\n    cargo run -q -p ducat-harness -- --inbox-reply {} {} {}", desc.key(), wk, ws);
    println!("  then:\n    cargo run -q -p ducat-harness -- --inbox-collect {}\n", desc.key());

    rc.close_dht_record(desc.key().clone()).await?;
    api.shutdown().await;
    Ok(())
}

pub async fn inbox_reply(key: &str, wpub: &str, wsec: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — contact inbox: reply (different node)\x1b[0m\n");
    let rk = RecordKey::from_str(key).map_err(|e| format!("bad key: {e}"))?;
    let kp = KeyPair::new(
        CRYPTO_KIND_VLD0,
        BareKeyPair::new(
            BarePublicKey::new(&hex::decode(wpub)?),
            BareSecretKey::new(&hex::decode(wsec)?),
        ),
    );

    let (api, _c) = crate::veilid::start("inbox-r").await?;
    let rc = api.routing_context()?;
    rc.open_dht_record(rk.clone(), Some(kp)).await?;
    println!("  opened as writer");

    match rc.get_dht_value(rk.clone(), 0, true).await? {
        Some(v) => println!("  read 0   {:?}", String::from_utf8_lossy(v.data())),
        None => return Err("subkey 0 was empty — the issuer never wrote".into()),
    }

    rc.set_dht_value(rk.clone(), 1, b"claimant: persona + outbox key".to_vec(), None)
        .await?;
    println!("  wrote    subkey 1");
    println!("\n  \x1b[32mthe card holder answered in place, with the issuer offline\x1b[0m");

    rc.close_dht_record(rk).await?;
    api.shutdown().await;
    Ok(())
}

pub async fn inbox_collect(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::str::FromStr;
    println!("\n\x1b[1mDUCAT — contact inbox: collect\x1b[0m\n");
    let rk = RecordKey::from_str(key).map_err(|e| format!("bad key: {e}"))?;
    let (api, _c) = crate::veilid::start("inbox-k").await?;
    let rc = api.routing_context()?;
    rc.open_dht_record(rk.clone(), None).await?;
    match rc.get_dht_value(rk.clone(), 1, true).await? {
        Some(v) => {
            println!("  read 1   {:?}", String::from_utf8_lossy(v.data()));
            println!("\n  \x1b[32mthe handshake completed without the two ever being online together\x1b[0m");
        }
        None => {
            println!("  \x1b[31msubkey 1 is empty\x1b[0m");
            return Err("no reply".into());
        }
    }
    rc.close_dht_record(rk).await?;
    api.shutdown().await;
    Ok(())
}
