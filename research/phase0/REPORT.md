# DUCAT Phase 0 — Results

Run 2026-08-11 against `veilid-core 0.5.7`, the current published release.
Two independent runs; both reported where they disagree.

| | Experiment | Result |
|---|---|---|
| **0a** | Route-blob size vs. media budgets (O11) | **Measured** — and it resists being a constant |
| **0b** | Veilid throughput at sync volumes (O14) | **Measured** — provisionally adequate |
| **0c** | Status of Veilid issue #395 (§15.10) | **Answered** — open, no near-term fix |

---

## Retraction

An earlier version of this report concluded that Veilid needs inbound port forwarding to become usable. **That was wrong.** Veilid is designed so a NAT'd node determines an `OutboundOnly` network class and obtains an inbound relay from a peer; laptops behind NAT are the normal case.

The real cause of the early failures was **self-inflicted**: a long-running background node from a previous test was still holding the default port, so subsequent nodes started degraded and plateaued at 2 peers. After killing it, a clean node reached `AttachedFull` with **86–92 live peers and `public_internet_ready=true` in 4–6 seconds**, with no forwarding and no configuration changes.

Diagnostic lesson worth keeping: `live_peer_count` stuck at exactly 2 (the bootstrap nodes) means the local node is broken, not the network.

---

## 0a — Route blob size

```
token mode                     190 B   (32 B pointer + 158 B fixed)
inline, 1 hop      719–728 B → 877–886 B
inline, 2 hops    1097–1292 B → 1255–1450 B
inline, 3 hops     963–1401 B → 1121–1559 B
inline, 4 hops    1049–1511 B → 1207–1669 B
                  (blob)       (full TapPresent)
```

### The finding is the variance, not the numbers

**Size is not monotonic in hop count.** Run 2 produced a 3-hop blob of 963 B — *smaller* than its own 2-hop blob at 1097 B. The spread within a single hop count (963–1401 B at 3 hops) exceeds the spread between hop counts.

The cause is structural, confirmed by reading `route_assemble.rs`: a blob is nested onion encryption, one layer per hop, and while intermediate hops compress to a bare 32-byte node id under route optimization, **the entry hop always embeds full `PeerInfo`** — dial addresses and signatures belonging to a peer the allocating client neither chooses nor controls. Blob size is dominated by *which peer got selected*.

### What this invalidated

Draft 0.5 asserted that QR error-correction level H clears inline mode at every hop count. Across two runs on the same host minutes apart, level H (1273 B) **passed at every hop count in one run and failed from 2 hops up in the other.**

No static table can answer "will this fit." Only the blob in hand can. §15.3.1 now specifies measure-and-degrade.

### Media consequences

- **NTAG213** (144 B) — `TapStatic` only, as predicted.
- **NTAG216** (888 B) — cleared by 1-hop inline by 2–11 bytes across runs. That is a coin flip, not a margin. **Tags ship tokens.**
- **NFC single contact** (~1 KB / 300 ms) — reliably fits 1 hop only; every multi-hop route overflowed in both runs.
- **`token` mode is the privacy-preserving choice**, not merely the compact one: 190 B constant regardless of hop count, so anonymity stops competing with the medium.

---

## 0b — Throughput over a private route

| Payload | RTT | Throughput |
|---|---|---|
| 1 KB | 186–235 ms | 4.3–5.4 KB/s |
| 4 KB | 217–325 ms | 12.3–18.4 KB/s |
| 16 KB | 164–262 ms | 61.0–97.2 KB/s |
| 32 KB | 157–237 ms | 134.7–203.7 KB/s |

**Latency dominates and payload is nearly free** — RTT barely moves from 1 KB to 32 KB. A scanning client should therefore request the largest blocks it can per round trip and pipeline, rather than streaming small reads.

At 32 KB requests, a day of Monero blocks moves in minutes. A full-chain sync would take hours to days — which is exactly why §17.1's restore-height-of-now is load-bearing rather than a convenience.

**Caveats:** two samples, measured against a self-route rather than a real `relay/1` peer, sequential and unpipelined, and `app_call` caps payloads near 32 KB so bulk transfer is many calls rather than a stream. Enough to proceed; not enough to design against.

---

## 0c — Veilid #395 is open, and the fix is a redesign

Queried via the GitLab API rather than the rendered page:

```
state     opened
created   2024-07-22
updated   2026-06-12        (active, not abandoned)
labels    Privacy, Security, Veilid-Core
milestone Release 0.13.0 - Private Routing 2.0
notes     9 comments, 2 upvotes
```

Current release is **0.5.7**. The fix is milestoned eight minor versions out on a release named *Private Routing 2.0* — a redesign, not a patch. Corroborated in-source: `route_assemble.rs` carries the comment *"Temporary until PR2.0 removes route optimization entirely."*

### Correction: `token` mode does not mitigate it

§15.10 previously advised preferring `token` mode "since fetching a route from the DHT is not the same act as importing one handed to you." **That reasoning was wrong.** The exposure comes from importing and *using* a hostile route — the importer's routing table liveness-pings the first hop, and the publisher correlates the pinging address. Fetching the same blob from an adversary-controlled DHT record is byte-identical exposure.

### What it actually costs

- **Proximity profiles** — the counterparty is already co-present and §2.3 concedes anonymity does not reach the curb. The real harm is narrower: **an address becomes a cross-transaction linking identifier that survives session-key rotation**, so a merchant can recognize a returning customer whose ephemeral keys are all fresh. That defeats a property §4 claims.
- **Remote hail (§5.2)** — no co-presence, unmitigated, strictly worse. Anyone publishing an advert learns the address of everyone who hails them. Should not ship until 0.13.0 (O19).

---

## Running it

```
cd phase0
cargo build
DUCAT_WAIT_SECS=300 ./target/debug/ducat-phase0

# with Veilid's own diagnostics:
RUST_LOG=veilid_core::network_manager=debug ./target/debug/ducat-phase0
```

Needs no port forwarding. **Ensure no other node is holding the port** — check `pgrep -af ducat-phase0` first.
