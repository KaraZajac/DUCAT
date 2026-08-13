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
