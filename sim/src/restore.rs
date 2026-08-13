//! Device loss, end to end, against stagenet (§4.3).
//!
//! Everything about backup is easy to get right on paper and easy to get wrong
//! in a way that only shows up the one time it matters — when the user's phone
//! is already at the bottom of a river. So this does not test the crypto, which
//! `core/tests/backup.rs` covers. It tests the *claim*: that a user holding one
//! encrypted file and a passphrase gets their money back on hardware that has
//! never seen their wallet.
//!
//! The wallet is restored into a fresh directory on a separate RPC port, from
//! the seed alone. Nothing is copied.

use std::process::Command;

use ducat_core::backup::{export, import, Backup};
use serde_json::json;

use crate::wallet::{Wallet, RELAYS};

const SOURCE_PORT: u16 = 28101;
const SOURCE_NAME: &str = "user_01";
/// A port nothing else in this project uses, so a stale process cannot be
/// mistaken for a successful restore — a mistake this project has made before.
const RESTORE_PORT: u16 = 28111;

pub fn restore_main() {
    println!("\n\x1b[1mDUCAT — device loss and restore, on stagenet\x1b[0m");
    println!("  §4.3: one encrypted file, one passphrase, hardware that has never seen the wallet\n");

    // ---- 1. The device that is about to be lost -------------------------
    let src = match Wallet::new(SOURCE_NAME, SOURCE_PORT) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("  source wallet unavailable: {e}");
            std::process::exit(1);
        }
    };
    let _ = src.refresh();
    let before = src.balance().expect("source balance");
    let seed = src
        .call("query_key", json!({"key_type": "mnemonic"}))
        .expect("mnemonic")["key"]
        .as_str()
        .expect("mnemonic string")
        .to_string();
    let height = src.call("get_height", json!({})).expect("height")["height"]
        .as_u64()
        .unwrap_or(0);

    println!("  original wallet");
    println!("    address  {}…", &src.address[..16]);
    println!("    balance  {} pXMR across {} unlocked outputs", before.unlocked, before.unlocked_outputs);
    println!("    height   {height}\n");

    // ---- 2. Export ------------------------------------------------------
    // A real client draws these from the OS CSPRNG. Fixed here so the run is
    // reproducible; §4.3.2 requires them fresh per export.
    let bundle = Backup {
        persona_suite: 1,
        persona_secret: vec![0xA5; 32],
        monero_seed: seed.clone(),
        // Backdated slightly: a restore height must be at or below the wallet's
        // first transaction or funds are missed, so clients err early.
        monero_restore_height: height.saturating_sub(500),
        rendezvous: vec![vec![0x01; 32]],
        attestation_records: vec![vec![0x02; 32]],
        mandates: vec![],
        verification: ducat_core::verify::VerificationPolicy::default(),
        // No escrow open in this scenario. §4.3.3: shares are per-escrow and
        // short-lived, so an empty list is the normal case, not a gap.
        escrow_shares: vec![],
        display_name: None,
        publish_payto: false,
        created: 0,
    };
    let passphrase = b"the correct passphrase";
    let blob = export(&bundle, passphrase, [0x11; 16], [0x22; 24]).expect("export");
    let path = "/tmp/ducat-restore-test.ducatbak";
    std::fs::write(path, &blob).expect("write backup");
    println!("  exported {} bytes to {path}", blob.len());

    // The seed must not be recoverable from the file without the passphrase.
    // Cheap to check, and catches the class of bug where a field is accidentally
    // written outside the sealed region.
    let first_word = seed.split_whitespace().next().unwrap();
    assert!(
        !String::from_utf8_lossy(&blob).contains(first_word),
        "seed material is visible in the exported file"
    );
    println!("  seed is not present in the ciphertext\n");

    // ---- 3. The new device ----------------------------------------------
    let recovered = import(&std::fs::read(path).expect("read backup"), passphrase).expect("import");
    assert_eq!(recovered.monero_seed, seed);
    println!("  imported on a device with no prior state");
    println!("    restore height {}\n", recovered.monero_restore_height);

    let dir = "/tmp/ducat-restored-wallet";
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("wallet dir");

    let bin = "/home/kara/Projects/SPECIE/research/monero-spike/monero-x86_64-linux-gnu-v0.18.5.1/monero-wallet-rpc";
    let mut child = Command::new(bin)
        .args([
            "--stagenet",
            "--daemon-address", RELAYS[0],
            "--untrusted-daemon",
            "--rpc-bind-port", &RESTORE_PORT.to_string(),
            "--disable-rpc-login",
            "--wallet-dir", dir,
            "--log-file", &format!("{dir}/rpc.log"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn wallet-rpc");

    // Poll rather than sleep a guessed interval.
    let mut up = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if std::net::TcpStream::connect(("127.0.0.1", RESTORE_PORT)).is_ok() {
            up = true;
            break;
        }
    }
    assert!(up, "restore wallet-rpc never came up");

    let started = std::time::Instant::now();
    let blank = Wallet {
        port: RESTORE_PORT,
        name: "restored".into(),
        address: String::new(),
        relay: std::cell::Cell::new(0),
    };
    let r = blank
        .call(
            "restore_deterministic_wallet",
            json!({
                "filename": "restored",
                "password": "",
                "seed": recovered.monero_seed,
                "restore_height": recovered.monero_restore_height,
                "language": "English"
            }),
        )
        .expect("restore_deterministic_wallet");
    let restored_addr = r["address"].as_str().unwrap_or_default().to_string();

    println!("  restored from seed in {:.1}s", started.elapsed().as_secs_f64());
    println!("    address  {}…", &restored_addr[..16.min(restored_addr.len())]);

    // ---- 4. Does it hold the same money? --------------------------------
    assert_eq!(restored_addr, src.address, "restored a different wallet");
    println!("    address matches the original\n");

    let scan = std::time::Instant::now();
    let _ = blank.refresh();
    let after = blank.balance().expect("restored balance");
    println!("  scanned from height {} in {:.1}s", recovered.monero_restore_height, scan.elapsed().as_secs_f64());
    println!("    balance  {} pXMR across {} unlocked outputs", after.unlocked, after.unlocked_outputs);

    // The comparison that matters. Not "a wallet opened" — the same money.
    assert_eq!(
        after.unlocked, before.unlocked,
        "restored wallet holds a different balance"
    );
    assert_eq!(
        after.unlocked_outputs, before.unlocked_outputs,
        "restored wallet sees a different number of spendable outputs — \
         §17.2 capacity would be wrong even though the balance looked right"
    );

    println!("\n  \x1b[32mrestore verified\x1b[0m — same address, same balance, same output count,");
    println!("  from one encrypted file on a wallet that had never seen this seed.");

    let _ = child.kill();
    let _ = child.wait();

    // ---- 5. The plausible way to get the height wrong -------------------
    //
    // "Stamp the current height at export" is the obvious implementation and it
    // is catastrophic: the wallet scans forward from *after* every output it
    // owns, finds nothing, and reports a balance of zero. The seed is intact,
    // the money is intact, and the user is looking at an empty wallet with no
    // error anywhere. Worth demonstrating rather than asserting.
    wrong_height_loses_everything(&recovered.monero_seed, height, bin, dir);
}

fn wrong_height_loses_everything(seed: &str, current_height: u64, bin: &str, dir: &str) {
    println!("\n\x1b[1m  the failure mode this field exists to prevent\x1b[0m");

    let dir2 = format!("{dir}-wrong");
    let _ = std::fs::remove_dir_all(&dir2);
    std::fs::create_dir_all(&dir2).expect("wallet dir");
    let port = RESTORE_PORT + 1;

    let mut child = Command::new(bin)
        .args([
            "--stagenet",
            "--daemon-address", RELAYS[0],
            "--untrusted-daemon",
            "--rpc-bind-port", &port.to_string(),
            "--disable-rpc-login",
            "--wallet-dir", &dir2,
            "--log-file", &format!("{dir2}/rpc.log"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn wallet-rpc");

    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
    }

    let w = Wallet {
        port,
        name: "restored-wrong".into(),
        address: String::new(),
        relay: std::cell::Cell::new(0),
    };
    w.call(
        "restore_deterministic_wallet",
        json!({
            "filename": "restored-wrong",
            "password": "",
            "seed": seed,
            "restore_height": current_height,   // <- the bug
            "language": "English"
        }),
    )
    .expect("restore");
    let _ = w.refresh();
    let b = w.balance().expect("balance");

    println!("    restore_height = {current_height} (the height at export)");
    println!("    balance  {} pXMR across {} unlocked outputs", b.unlocked, b.unlocked_outputs);
    assert_eq!(b.unlocked, 0, "expected the wrong-height restore to see nothing");
    println!("    \x1b[33mcorrect seed, correct address, zero balance, no error\x1b[0m");
    println!("    the height MUST be at or below the oldest unspent output, never the current one.");

    let _ = child.kill();
    let _ = child.wait();
}
