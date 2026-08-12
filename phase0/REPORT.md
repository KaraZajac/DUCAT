# DUCAT Phase 0 — Results

Run 2026-08-11 against `veilid-core 0.5.7`, the current published release.

| | Experiment | Result |
|---|---|---|
| **0a** | Route-blob size vs. media budgets (O11) | **Partial** — token mode confirmed; inline mode blocked |
| **0b** | Veilid throughput at sync volumes (O14) | **Blocked** — depends on 0a's route allocation |
| **0c** | Status of Veilid issue #395 (§15.10) | **Answered, and worse than assumed** |

---

## 0c — Veilid #395 is open, and the fix is a redesign

Queried via the GitLab API rather than the rendered page:

```
iid       395
state     opened
created   2024-07-22
updated   2026-06-12        (active, not abandoned)
labels    Privacy, Security, Veilid-Core
milestone Release 0.13.0 - Private Routing 2.0
notes     9 comments, 2 upvotes
```

The current release is **0.5.7**. The fix is milestoned eight minor versions out, on a release explicitly named *Private Routing 2.0* — a redesign, not a patch. Corroborated in the source: `route_assemble.rs` carries the comment *"Temporary until PR2.0 removes route optimization entirely."*

**There is no near-term upstream fix.** Any DUCAT client shipping on 0.5.x inherits this.

### Correction: `token` mode does not mitigate it

§15.10 currently advises preferring `token` mode "since fetching a route from the DHT is not the same act as importing one handed to you." **That reasoning is wrong.**

The vulnerability lies in *importing and using* a hostile route — the importer's routing table liveness-pings the route's first hop, and the route's publisher correlates the pinging address. Fetching the same hostile blob from a DHT record the adversary controls delivers byte-identical exposure. Token mode changes the delivery channel, not the trust relationship. The advice must be replaced, not softened.

### What the exposure actually costs

Scoped honestly, because it is not uniform across profiles:

- **Proximity profiles** — the counterparty is already physically co-present, and §2.3 concedes that anonymity does not extend to the curb. The marginal harm is therefore *not* "a stranger learns you exist." It is that **an IP becomes a cross-transaction linking identifier that survives session-key rotation** — a merchant can recognize a returning customer whose ephemeral keys are all fresh. That defeats a property §4 explicitly claims.
- **Remote hail (§5.2)** — no co-presence, so the exposure is unmitigated and strictly larger. Any counterparty who publishes an advert can learn the address of everyone who hails them.

Remote hail is the profile that should carry the warning, and arguably the gate.

---

## 0a — Partial

### Confirmed without the network

Token mode is `158 B` fixed + `32 B` pointer = **190 B**:

| Medium | Capacity | 190 B token mode |
|---|---|---|
| NTAG213 | 144 B | **fail** |
| NTAG215 | 504 B | pass |
| NTAG216 | 888 B | pass |
| QR (H/Q/M/L) | 1273 / 1663 / 2331 / 2953 B | pass |
| NFC HCE | ~1 KB per 300 ms | pass |

This confirms §15.3.2's static-tag claim: the commodity NTAG213 holds a `TapStatic` and nothing more.

### Inline mode — blocked, and why

Route allocation fails with:

```
TryAgain: unable to allocate route until we have a valid PublicInternet network class
```

The node attaches but plateaus at **2 live peers / 0 reliable**, and `public_internet_ready` never becomes true (waited 15 min).

Ruled out:

- **Not the sandbox** — identical result with sandboxing disabled.
- **Not DNS** — `bootstrap.veilid.net` TXT resolves; both bootstrap node records parse.
- **Not egress** — TCP 5150 **open** to both bootstrap nodes; outbound UDP confirmed working.

The node reaches bootstrap and stops. Most likely this host has no inbound reachability, so it cannot determine its network class, and with only bootstrap peers it cannot obtain a relay to compensate.

**To finish 0a/0b:** run on a host with inbound reachability — a public IP, or forwarded UDP+TCP 5150. The harness produces the numbers immediately in that environment; nothing further needs writing.

### Structural finding that partly substitutes

Reading `route_assemble.rs` gives the shape even without a sample. A route blob is **nested onion encryption, one layer per hop**, and:

- Intermediate hops compress to a bare 32-byte `NodeId` under `optimized`.
- **The entry hop always embeds full `PeerInfo`** — dial info, addresses, signatures.
- `max_route_hop_count` defaults to **4**, hard-capped at **5**.

So blob size is **dominated by the entry hop's advertised dial info, not linearly by hop count** — and that peer is not one the allocating client chooses or controls.

**This changes what the spec should say.** A fixed byte budget for `inline` mode is not knowable in advance, because it varies per allocation with a third party's peer info. The correct client behavior is a runtime check, not a constant:

> Measure the blob you actually received, and degrade to `token` mode when it overflows the medium in hand.

That is more robust than any measured constant would have been, and it can be specified now without finishing 0a.

---

## 0b — Blocked

Same root cause: no route, no round trip, no throughput number. O14 stands entirely unmeasured. The harness's 0b stage is written and will run once 0a's precondition is met.

---

## Running it

```
cd phase0
cargo build
DUCAT_WAIT_SECS=900 ./target/debug/ducat-phase0
```

Needs inbound reachability on UDP+TCP 5150 to get past `public_internet_ready`.
