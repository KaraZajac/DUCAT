//! A thin monero-wallet-rpc client, enough for the simulator to settle for real.
//!
//! Deliberately minimal. The point is not to build a wallet library but to make
//! the simulator's FUND step move actual stagenet funds, so that §17.2's output
//! accounting is exercised against the chain rather than against bookkeeping
//! that cannot disagree with itself.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};

pub struct Wallet {
    pub port: u16,
    pub name: String,
    pub address: String,
}

#[derive(Debug)]
pub struct Balance {
    pub total: u64,
    pub unlocked: u64,
    pub blocks_to_unlock: u64,
    /// Outputs the wallet can spend right now. §17.2: consecutive payment
    /// capacity is this count, not a balance — a payment consumes a whole
    /// output and its change returns locked.
    pub unlocked_outputs: usize,
}

impl Wallet {
    pub fn new(name: &str, port: u16) -> Result<Self, String> {
        let mut w = Wallet {
            port,
            name: name.to_string(),
            address: String::new(),
        };
        w.call("open_wallet", json!({"filename": name, "password": ""}))
            .ok(); // already open is fine
        let r = w.call("get_address", json!({"account_index": 0}))?;
        w.address = r["address"].as_str().unwrap_or_default().to_string();
        if w.address.is_empty() {
            return Err(format!("{}: no address", name));
        }
        Ok(w)
    }

    /// Minimal JSON-RPC over a raw socket. Avoids pulling an HTTP stack in for
    /// four call sites; if this ever needs redirects, auth, or TLS it should be
    /// replaced rather than extended.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = json!({"jsonrpc":"2.0","id":"0","method":method,"params":params}).to_string();
        let req = format!(
            "POST /json_rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut s = TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|e| format!("{}: connect: {}", self.name, e))?;
        s.set_read_timeout(Some(Duration::from_secs(180))).ok();
        s.set_write_timeout(Some(Duration::from_secs(60))).ok();
        s.write_all(req.as_bytes())
            .map_err(|e| format!("{}: write: {}", self.name, e))?;
        let mut raw = String::new();
        s.read_to_string(&mut raw)
            .map_err(|e| format!("{}: read: {}", self.name, e))?;
        let start = raw
            .find("\r\n\r\n")
            .ok_or_else(|| format!("{}: malformed response", self.name))?
            + 4;
        // Chunked responses would need unpacking; wallet-rpc sends
        // Content-Length, and a body that does not parse is a hard error rather
        // than something to guess at.
        let v: Value = serde_json::from_str(raw[start..].trim())
            .map_err(|e| format!("{}: bad JSON ({}): {}", self.name, e, &raw[start..].trim().chars().take(120).collect::<String>()))?;
        if let Some(err) = v.get("error") {
            return Err(format!(
                "{}: {} — {}",
                self.name,
                method,
                err["message"].as_str().unwrap_or("?")
            ));
        }
        Ok(v["result"].clone())
    }

    pub fn refresh(&self) -> Result<(), String> {
        self.call("refresh", json!({})).map(|_| ())
    }

    pub fn balance(&self) -> Result<Balance, String> {
        let r = self.call("get_balance", json!({"account_index": 0}))?;
        // Count outputs against unlocked value rather than trusting
        // `transfer_type: "available"`, which includes locked outputs — a trap
        // measured directly in monero-spike/REPORT.md.
        let t = self.call(
            "incoming_transfers",
            json!({"transfer_type": "available", "account_index": 0}),
        )?;
        let unlocked = r["unlocked_balance"].as_u64().unwrap_or(0);
        let mut spendable = 0usize;
        let mut running = 0u64;
        if let Some(list) = t["transfers"].as_array() {
            let mut amounts: Vec<u64> = list
                .iter()
                .filter(|x| !x["spent"].as_bool().unwrap_or(true))
                .filter_map(|x| x["amount"].as_u64())
                .collect();
            amounts.sort_unstable_by(|a, b| b.cmp(a));
            for a in amounts {
                if running + a > unlocked {
                    break;
                }
                running += a;
                spendable += 1;
            }
        }
        Ok(Balance {
            total: r["balance"].as_u64().unwrap_or(0),
            unlocked,
            blocks_to_unlock: r["blocks_to_unlock"].as_u64().unwrap_or(0),
            unlocked_outputs: spendable,
        })
    }

    /// Settle. Returns the transaction hash.
    pub fn pay(&self, to: &str, amount_pxmr: u64) -> Result<String, String> {
        let r = self.call(
            "transfer",
            json!({
                "destinations": [{"amount": amount_pxmr, "address": to}],
                "account_index": 0,
                "priority": 1,
                "get_tx_key": true
            }),
        )?;
        r["tx_hash"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("{}: transfer returned no tx_hash", self.name))
    }
}
