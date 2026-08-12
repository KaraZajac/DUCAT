//! Minimal `monero-wallet-rpc` client — enough to fund and to verify.
//!
//! Deliberately not shared with `sim/src/wallet.rs`: this one has to answer the
//! payee's question, which is *did money arrive*, and it answers it by scanning
//! rather than by trusting the payer's TXID (§17.3, §17.4).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};

pub const RELAYS: &[&str] = &[
    "node.monerodevs.org:38089",
    "stagenet.xmr-tw.org:38081",
    "xmr-lux.boldsuck.org:38081",
];

/// How long to wait for a transaction to appear somewhere it was not sent.
const PROPAGATION_TRIES: u64 = 8;
const PROPAGATION_GAP_S: u64 = 5;

pub struct Wallet {
    pub port: u16,
    pub name: String,
    pub address: String,
}

impl Wallet {
    pub fn open(name: &str, port: u16) -> Result<Self, String> {
        let mut w = Wallet { port, name: name.into(), address: String::new() };
        let _ = w.call("open_wallet", json!({"filename": name, "password": ""}));
        // A relay that has died reports a cached height rather than an error
        // (§8.7.2), so pick one that answers now rather than one that answered
        // when this program was written.
        for r in RELAYS {
            let _ = w.call("set_daemon", json!({"address": r, "trusted": false}));
            if w.call("get_height", json!({})).is_ok() {
                break;
            }
        }
        let _ = w.call("refresh", json!({}));
        let r = w.call("get_address", json!({"account_index": 0}))?;
        w.address = r["address"].as_str().unwrap_or_default().to_string();
        if w.address.is_empty() {
            return Err(format!("{name}: no address"));
        }
        Ok(w)
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = json!({"jsonrpc":"2.0","id":"0","method":method,"params":params}).to_string();
        let req = format!(
            "POST /json_rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut s = TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|e| format!("{}: connect: {e}", self.name))?;
        s.set_read_timeout(Some(Duration::from_secs(180))).ok();
        s.write_all(req.as_bytes()).map_err(|e| format!("{}: write: {e}", self.name))?;
        let mut raw = String::new();
        s.read_to_string(&mut raw).map_err(|e| format!("{}: read: {e}", self.name))?;
        let start = raw.find("\r\n\r\n").ok_or("malformed response")? + 4;
        let v: Value = serde_json::from_str(raw[start..].trim())
            .map_err(|e| format!("{}: bad json: {e}", self.name))?;
        if let Some(err) = v.get("error") {
            return Err(format!("{}: {}: {}", self.name, method, err["message"]));
        }
        Ok(v["result"].clone())
    }

    pub fn pay(&self, to: &str, amount_pxmr: u64) -> Result<String, String> {
        let r = self.call(
            "transfer",
            json!({"destinations":[{"amount":amount_pxmr,"address":to}],
                   "account_index":0,"priority":1,"get_tx_key":true}),
        )?;
        r["tx_hash"].as_str().map(|s| s.to_string()).ok_or("no tx_hash".into())
    }

    /// §8.7.2: a txid from the submitting node is that node's word. Confirm on
    /// another before treating it as sent.
    ///
    /// **Bounded retry, not a single shot.** Propagation is not instantaneous,
    /// and a check that fires the moment `transfer` returns reports every
    /// healthy transaction as lost. This harness did exactly that on its first
    /// end-to-end run — the transaction was in two independent pools seconds
    /// later. The failure it exists to catch is a transaction that is *never*
    /// visible, which is only distinguishable from one that is not visible
    /// *yet* by waiting.
    pub fn confirm_propagated(&self, txid: &str) -> Result<String, String> {
        for attempt in 0..PROPAGATION_TRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(PROPAGATION_GAP_S));
            }
            if let Some(relay) = self.seen_anywhere(txid) {
                return Ok(relay);
            }
        }
        Err(format!(
            "{txid} was not visible on any relay after {}s — resubmit rather than wait",
            PROPAGATION_TRIES * PROPAGATION_GAP_S
        ))
    }

    fn seen_anywhere(&self, txid: &str) -> Option<String> {
        for relay in RELAYS {
            let (host, port) = relay.split_once(':').unwrap_or((relay, "38081"));
            let out = std::process::Command::new("curl")
                .args([
                    "-s", "-m", "20", "-X", "POST",
                    &format!("http://{host}:{port}/get_transactions"),
                    "-H", "Content-Type: application/json",
                    "-d", &format!("{{\"txs_hashes\":[\"{txid}\"]}}"),
                ])
                .output()
                .ok()?;
            let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
            if let Some(t) = v["txs"].as_array().and_then(|a| a.first()) {
                if t["in_pool"].as_bool().unwrap_or(false)
                    || t["block_height"].as_u64().unwrap_or(0) > 0
                {
                    return Some(relay.to_string());
                }
            }
        }
        None
    }

    /// **The payee's own answer to "was I paid".**
    ///
    /// §17.4: the payee *is* the recipient, so it scans with its own view key
    /// rather than trusting anything in the payer's message.
    ///
    /// **Bounded by §6.2's window, and the bound is a security property.** The
    /// first version scanned 30 times at 10-second intervals — five minutes of
    /// blocking on a value the counterparty supplies. An attacker sends a TXID
    /// naming a transaction that does not exist and the terminal is frozen for
    /// the cost of one message. Mempool visibility is near-immediate when a
    /// payment is real; a long wait is evidence of absence, not of slowness.
    pub fn scan_for(&self, txid: &str, tries: u32) -> Result<u64, String> {
        for _ in 0..tries {
            let _ = self.call("refresh", json!({}));
            if let Ok(r) = self.call("get_transfer_by_txid", json!({"txid": txid})) {
                if let Some(a) = r["transfer"]["amount"].as_u64() {
                    return Ok(a);
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
        Err(format!("{}: never observed {txid}", self.name))
    }
}
