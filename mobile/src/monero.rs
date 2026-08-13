//! Talking to a Monero daemon (§17).
//!
//! # On Dandelion++
//!
//! **Dandelion++ is the daemon's, not the wallet's.** It runs in `monerod`'s
//! transaction relay: a node holds a new transaction through a stem phase
//! before fluffing it to its peers. A wallet has no peer connections of its
//! own, so it cannot stem anything — it calls `send_raw_transaction` and the
//! node it called does the rest.
//!
//! It has also been in every release since 0.15, in 2019, so "supports D++" is
//! not a property that distinguishes one live stagenet node from another. Any
//! node serving this network today does it.
//!
//! What a wallet actually chooses, and what these functions expose:
//!
//! - **Which node** it hands the transaction to. That node learns the link
//!   between your address for it and the transaction, which is the exposure
//!   D++ exists to prevent *between* nodes and cannot prevent at the one you
//!   talked to. Your own node removes it entirely.
//! - **What carries the request.** An onion endpoint hides the link from the
//!   network path even when the node itself is a stranger's.
//!
//! So the ranking is by **trust and transport**, not by protocol support, and
//! the UI says which one is in use rather than claiming a property it cannot
//! check.

use std::time::{Duration, Instant};

/// How much the node we are using can learn about us.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeTrust {
    /// Ours. It learns nothing we did not already know.
    Own,
    /// Reached through Tor. A stranger's node, but not one that also sees where
    /// the request came from.
    Onion,
    /// A stranger's node on the open network. It sees our address and our
    /// transactions together.
    PublicClearnet,
}

/// A node worth trying, and what using it costs.
#[derive(uniffi::Record, Clone)]
pub struct NodeCandidate {
    pub url: String,
    pub trust: NodeTrust,
    pub label: String,
}

/// The list, in the order it should be tried.
///
/// Own first, then onion, then clearnet — the order in which they give away
/// less. A user pointing at their own node should never be silently demoted to
/// a public one, so a configured node is returned alone rather than at the head
/// of a fallback list.
#[uniffi::export]
pub fn monero_default_nodes(own_url: Option<String>) -> Vec<NodeCandidate> {
    if let Some(u) = own_url.filter(|u| !u.trim().is_empty()) {
        return vec![NodeCandidate {
            url: u.trim().to_string(),
            trust: NodeTrust::Own,
            label: "your node".into(),
        }];
    }
    vec![
        NodeCandidate {
            url: "http://xmr-lux.boldsuck.org:38081".into(),
            trust: NodeTrust::PublicClearnet,
            label: "boldsuck (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://stagenet.xmr-tw.org:38081".into(),
            trust: NodeTrust::PublicClearnet,
            label: "xmr-tw (stagenet)".into(),
        },
        NodeCandidate {
            url: "http://node.monerodevs.org:38089".into(),
            trust: NodeTrust::PublicClearnet,
            label: "monerodevs (stagenet)".into(),
        },
    ]
}

/// What a probe found. Named apart from Veilid's `NodeStatus`: two things
/// called the same in one bridge is a footgun for every caller downstream.
#[derive(uniffi::Record, Clone)]
pub struct MoneroNodeStatus {
    pub url: String,
    pub reachable: bool,
    pub height: u64,
    /// The daemon's own view of whether it has caught up. A node still syncing
    /// will happily answer and report a height that is behind, which would show
    /// a wallet a balance that is merely old.
    pub synced: bool,
    /// `stagenet`, `mainnet` or `testnet`, straight from the daemon. Checked
    /// rather than assumed: pointing a stagenet wallet at mainnet is the kind
    /// of mistake that is only funny until it involves real money.
    pub nettype: String,
    pub rtt_ms: u64,
    pub error: Option<String>,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum MoneroError {
    #[error("{0}")]
    Failed(String),
}

/// Ask a daemon where it is. Blocking; call it off the main thread.
#[uniffi::export]
pub fn monero_probe(url: String, timeout_ms: u32) -> MoneroNodeStatus {
    let t0 = Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms as u64))
        .build();

    let mut out = MoneroNodeStatus {
        url: url.clone(),
        reachable: false,
        height: 0,
        synced: false,
        nettype: String::new(),
        rtt_ms: 0,
        error: None,
    };

    let res = agent
        .post(&format!("{}/json_rpc", url.trim_end_matches('/')))
        .set("Content-Type", "application/json")
        .send_string(r#"{"jsonrpc":"2.0","id":"0","method":"get_info"}"#);

    out.rtt_ms = t0.elapsed().as_millis() as u64;
    match res {
        Err(e) => {
            out.error = Some(short_error(&e.to_string()));
            out
        }
        Ok(resp) => match resp
            .into_string()
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).map_err(|e| e.to_string()))
        {
            Err(e) => {
                out.error = Some(format!("bad response: {e}"));
                out
            }
            Ok(v) => {
                let r = &v["result"];
                out.reachable = true;
                out.height = r["height"].as_u64().unwrap_or(0);
                out.synced = r["synchronized"].as_bool().unwrap_or(false);
                out.nettype = r["nettype"].as_str().unwrap_or("").to_string();
                out
            }
        },
    }
}

/// Try candidates in order and keep the first that is usable on this network.
///
/// "Usable" means reachable **and** synced **and** on the network we asked for.
/// A node that answers is not the same as a node worth trusting a balance to,
/// and taking the first that merely responds is how a wallet ends up reading a
/// chain that stopped hours ago.
#[uniffi::export]
pub fn monero_pick_node(
    candidates: Vec<NodeCandidate>,
    want_nettype: String,
    timeout_ms: u32,
) -> Result<MoneroNodeStatus, MoneroError> {
    let mut last: Option<MoneroNodeStatus> = None;
    for c in &candidates {
        let s = monero_probe(c.url.clone(), timeout_ms);
        if s.reachable && s.synced && s.nettype == want_nettype {
            return Ok(s);
        }
        last = Some(s);
    }
    Err(MoneroError::Failed(match last {
        Some(s) if s.reachable && s.nettype != want_nettype => {
            format!("that node is on {}, not {want_nettype}", s.nettype)
        }
        Some(s) if s.reachable => "reachable but still syncing".into(),
        Some(s) => s.error.unwrap_or_else(|| "unreachable".into()),
        None => "no nodes configured".into(),
    }))
}

/// ureq's errors carry the whole URL and a stack of causes. A person
/// troubleshooting needs the reason, not the transcript.
fn short_error(e: &str) -> String {
    let lower = e.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timed out".into()
    } else if lower.contains("dns") {
        "cannot resolve that host".into()
    } else if lower.contains("connection refused") {
        "connection refused".into()
    } else {
        e.chars().take(90).collect()
    }
}
