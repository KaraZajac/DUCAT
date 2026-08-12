# DUCAT — A Peer-to-Peer Proximity Commerce Protocol
**Draft 0.19 — Consolidated**
*A ducat was a gold coin accepted from Venice to Vienna to the Levant for six centuries. It had no issuer relationship, no account behind it, and no permission attached — it was worth something because you were holding it, and it crossed borders the way a bearer instrument should.*
Status: Pre-alpha design document. Nothing here is final. Several sections rest on primitives that are not production-grade; §14 is the real agenda.

DUCAT composes two independent systems — Veilid for everything that isn't money, Monero for the money. They are joined by message-passing, not by cryptography, because Monero has no scripting layer to integrate *with*. §8.7 and §2.4 state what that costs; §8.5 names the one mechanism that would eventually close the seam.

---

## Contents

**Part I — Core Protocol**
1. Vision · 2. Threat Model (+2.5 RetoSwap case study) · 3. Layered Architecture · 4. L1 Identity · 5. L2 Discovery · 6. L3 Contract State Machine · 7. Profiles · 8. L4 Settlement · 9. L5 Trust · 10. Federation & Cold Start (+10.1 market descriptor) · 11. Sustainability · 12. Regulatory Posture · 13. Build Order · 14. Open Problems

**Part II — The Tap** (L2/L3 detail)
15.1 Tap as bootstrap · 15.2 The generating grid · 15.3 `TapPresent` + transport ladder · 15.4 `FullOffer` · 15.5 WYSIWYS · 15.6 Response leg · 15.7 Metered · 15.8 Split · 15.9 Static tags · 15.10 Residual attacks

**Part III — Identity & Persistent Contact** (L1 detail)
16.1 Two tiers of channel · 16.2 What identity means · 16.3 `CONTACT` sub-flow · 16.4 DHT rendezvous · 16.5 What rides on contact · 16.6 Privacy accounting · 16.7 Reputation tie-in · 16.8 Edge cases

**Part IV — Bonded Fast Settlement** (L4/L5 detail)
17.1 Bond once · 17.2 The float · 17.3 Zero-conf layers · 17.4 `fast/1` flow · 17.5 Slashing · 17.6 Cost to axiom A4 · 17.7 Denomination & oracle · 17.8 Residual risks

**Part V — Wire Format & Conformance** (interop detail)
18.1 Canonical encoding · 18.2 Money is integers · 18.3 Signing & domain separation · 18.4 State transition table · 18.5 Reject codes · 18.6 Version negotiation · 18.7 Transport bindings · 18.8 Strictness · 18.9 Test vectors · 18.10 Conformance levels

### Changelog
- **0.19** — **§5.2 replaced: providers listen instead of advertising.** Two observations drive it — reading a DHT record imports nothing, so matching can run with zero route imports; and the *publisher* learns the *importer's* address, so publishing is safe and importing is what exposes you. Hail is therefore inverted: providers watch a market record and publish nothing, consumers post a hail carrying a coarse cell and an ephemeral key but no route, providers answer sealed to that key, and only after mutual selection does one import occur — by the party that chose to initiate. **Providers become invisible**, which substantially retires O3 and removes hail's dependency on the Veilid 0.13.0 milestone (O19 contained rather than blocking). Also added §5.2.3: there is no map of nearby drivers, because a live driver map is a published surveillance database of workers' movements — strictly worse than the operator it replaces. The map lives *after* matching, over E2EE, where it is safe and expected. Optional provider visibility is excluded because in a competitive market it is not optional.
- **0.18** — **Added §2.5, the RetoSwap case study.** A Haveno-derived Monero DEX using 2-of-3 arbitrated multisig — §17.2's structure in production — was drained of ~7,000 XMR in May 2026 by a forged, out-of-order ACK that overwrote the arbitrator's address without any check against a known key. **Nothing about Monero failed**; the break was in the messaging layer, which here is Veilid. Route anonymity is not authentication. Four existing rules are the direct countermeasures (§18.3, §18.4, §18.8, §10.1), and §9.3 now states explicitly that arbiters come from the signed market descriptor and never from an address in a message. Second lesson recorded: Haveno was mature, had a prior exploit to learn from, and was breached again — this document's equivalent surface has had no adversarial review at all.
- **0.17** — **`fast/1` acceptance simplified: the recipient scans, the payer does not prove.** Source review of `monero-wallet` 0.2.0 found no transaction-proof support at all — and most of the requirement dissolved on inspection, because a tx proof exists to convince a non-recipient and the driver *is* the recipient. §17.3's Layer 1 and §17.4's flow now have the driver scan the mempool transaction with their own view key; `TXPROOF` becomes `TXID`. Proofs remain necessary for arbitration (§17.5) and are now DUCAT's to implement. Added O20 (scanning is block-oriented, so unconfirmed verification has no public API; plus the missing proof work) and O21 (burning-bug-immune outputs exist but are unspecified outside one implementation, so staying standard and detecting the attack beats adopting them).
- **0.16** — **Client architecture decided: embed a wallet, do not drive `monero-wallet-rpc`.** This dissolves the missing-API and halfway-stranding problems from 0.15 by construction, and permits FROSTLASS (`monero-oxide`, audited May 2025, O(1) per-signer vs native O(n!)) in place of wallet2's experimental multisig — the upstream warning in §8.2 describes wallet2's implementation, not threshold signing on Monero generally. Added the constraint this creates: **bond parties cannot mix schemes**, so `multisig_scheme` joins the market descriptor (§10.1). Also separated the `FLOAT`'s two halves in §17.2's user-facing guidance — "only load what you'll spend" describes `hot_wallet` and mislabels `bond_ms`, which is locked collateral backing fast-settle capacity. O1 reframed.
- **0.15** — **Monero multisig measured (O1).** A 2-of-3 ceremony converged in 2 rounds and 134 s on v0.18.5.1/stagenet — round-trip fragility was overstated. The real obstacles are that Monero ships multisig **disabled by default** with an upstream warning that funds may be unspendable or stealable (now quoted verbatim in §8.2), that **no RPC method enables it**, and that a wallet can be stranded halfway because `prepare` and `make` succeed while `exchange` refuses. O1 reframed from fragility to availability.
- **0.14** — **Field numbering fixed; transcripts exist.** Integer keys assigned for `TapPresent`, `FullOffer`, `ACCEPT`, and `RECEIPT`, with signed objects carried in an envelope `{1: body, 2: sig}` rather than a `sig` field inside the signed map. Encoded sizes measured and replacing estimates: token mode 217 B (was 190), inline 1-hop 915 B (was 886) — the earlier figures counted payload and omitted CBOR key and type headers. **Inline routes now fit no commodity tag at any hop count**, retiring the last argument against token-only tags. §18.9(4) closed: full `xfer/1`, `pos/1`, and `ride/1` transcripts ship in `vectors/v1/transcript.json`, chained and verified end to end, plus a tampered case. 92 vectors, 75 tests.
- **0.13** — **Conformance vectors exported (§18.9).** 88 language-neutral cases in `vectors/v1/`, each carrying a `why`, stated in §18.5 wire codes rather than any implementation's internal errors, deterministic on regeneration, with a runner that executes them against the reference client. O18 narrowed but explicitly *not* closed: vectors validated only by their own author encode that author's bugs as the spec. §18.9(4) full transcripts remain uncovered, blocked on fixing field numbering for `TapPresent`/`FullOffer`/`ACCEPT` — now the largest blocker in Part V.
- **0.12** — **P-256 suite implemented; two encoding-uniqueness bugs found and fixed.** §18.3 now states that uniqueness of encoding reaches past CBOR into values: ECDSA signature malleability (low-`s` mandatory, high-`s` refused rather than normalized) and SEC1 public-key encodings (compressed only, tag checked explicitly — a common parser accepts `0x05` and yields the same key as `0x03`). Both would have produced transcripts that hash differently while every signature still verified. Generalized as a rule: anywhere the protocol admits two byte representations of one value, it has a transcript-divergence bug. Core conformance's both-suites requirement (§4.1) is now satisfiable.
- **0.11** — **Negotiation implemented, and §18.6's suite rule corrected.** "Highest mutually supported" is right for versions and wrong for suites: identifiers are allocated in registration order and encode no preference, so highest-wins would have silently selected P-256 — a fallback forced by iOS hardware (§4.1) — over Ed25519 on every dual-capable pair. Suites are now chosen from an explicit preference list held by the payer. Also established that downgrade resistance needs no new machinery, since `offer_commit` already covers the advertised set — but the commitment MUST be checked *before* negotiating, not after. Extended §18.3's domain separation to commitments as well as signatures, since `offer_commit`, `H(RECEIPT)`, chain links, and `market_id` all hash canonical objects and a bare digest records which role it was for.
- **0.10** — **First implementation, and the gaps it found.** `ducat-core` implements §18.1–18.5: deterministic CBOR, domain-separated signing, the contract state machine, and reject codes, with 35 tests. Added §18.4.1 for six rules the transition table left open — message direction (only the payer may `ACCEPT`), `CANCEL`'s closing bound, the mode-dependent post-`ACCEPT` deadline (60 s direct/fast, 300 s escrow with fund recovery), the `FUNDED` deadline applying only under `fast/1`, terminal-state absorption with `CLOSED` deliberately excluded, and elapsed time in unbounded states being a no-op. Also established that no serde CBOR crate can satisfy §18.1, since none reject non-canonical input on decode.
- **0.9** — **Phase 0 completed; all three experiments have numbers.** 0a measured on veilid-core 0.5.7: `token` mode 190 B, `inline` mode 877–1669 B, **non-monotonic in hop count** with within-hop variance exceeding between-hop differences, because size tracks which peer was selected rather than how many hops were requested. This invalidated 0.5's claim that QR level H clears inline mode at every hop count — across two runs it passed in one and failed in the other, minutes apart. Tags now ship tokens; `token` mode reframed as the *privacy-preserving* choice since its size is constant regardless of hop count. 0b measured: 135–204 KB/s at 32 KB payloads, latency-dominated, adequate for incremental sync and not for a full chain — which is what §17.1's restore height already assumes. O11 and O14 closed. The earlier claim that Veilid needs inbound port forwarding was wrong and is retracted.
- **0.8** — **First empirical results (Phase 0).** 0c answered: Veilid #395 is open, milestoned to 0.13.0 against a current 0.5.7 — no near-term fix, and remote hail (§5.2) is gated on it (O19). **Corrected §15.10:** `token` mode does *not* mitigate hostile-route deanonymization, contrary to 0.5's claim — the exposure is in using the route, not in how the blob arrived. 0a dissolved rather than answered O11: `inline` blobs have no fixed size because the entry hop carries a third party's peer info, so §15.3.2 now mandates measure-and-degrade instead of a budget. 0b remains blocked on a host with inbound reachability. Harness and full results in `phase0/`.
- **0.7** — **Renamed SPECIE → DUCAT.** The old name was semantically exact and practically hostile: one letter from "species," pronounced *SPEE-shee* by almost nobody, and search-polluted by both biology and finance. Wire constants changed with it — domain-separation prefix `DUCAT-v1` (§18.3), NFC AID `F0 44 43 41 54` (§18.7), QR magic `DCAT`, URI scheme `ducat:`. Also: §9.4 scoped — the safety floor binds the high-exposure tier (rides, lodging, tasks) and not commerce as a whole, which narrows O5 and removes the apparent tension with §1's reframe. Four remaining gaps closed. §4.1: key storage, and the P-256 suite iOS's Secure Enclave forces into the registry. §6.2: clock skew — monotonic for elapsed time, ±120 s tolerance for absolute, both directions failing closed, skew detected but never applied. §8.8: transaction fees, `fee_policy`, the WYSIWYS requirement to display total outlay, and minimum-fee-tier refusal closing §17.8's cure-window abuse. §10.1: the market descriptor — self-certifying `market_id`, threshold-signed rotation chained from genesis. O8 narrowed from unspecified to mechanism-specified-accountability-open.
- **0.6** — **Added Part V (§18): wire format and conformance.** Canonical CBOR rules, integer-piconero money (correcting the float hazard in `amount_xmr`, now `amount_pxmr`), signature domain separation and the verify-received-bytes rule, the normative state transition table, reject codes, downgrade-resistant version negotiation, transport bindings (NFC AID, BLE UUIDs, QR envelopes with per-EC-level capacities), strict rejection of unknown fields, required test-vector coverage, and four conformance levels. §11's many-clients claim now points at something payable. Phase 1 ships the codec and identifiers rather than retrofitting them. O17–O18 added.
- **0.5** — **The tap is medium-agnostic; QR leads.** §15.1 restated: the bootstrap moves ~200 B–1 KB across a few feet so both phones can finish over Veilid, and only one direction needs to cross — which is why a one-way medium suffices. Added §15.3.1 (two reach modes, `inline` vs `token`, with concrete sizes) and §15.3.2 (the transport ladder, the NFC platform matrix, BLE channel security via Noise XX). NFC demoted to an Android-presenter optimization: iOS HCE is gated behind an EEA/organization/financial-regulatory entitlement incompatible with A4. Added QR's weaker relay resistance, hostile-route-blob deanonymization (Veilid #395), and mandatory length padding to §15.10. O11 downgraded, O16 added, Phase 0c added.
- **0.4** — **Reframe: the unit of ambition is the card terminal, not the rideshare app.** §1 restated with what DUCAT actually replaces; rides recast as the *hardest* profile rather than the representative one. Added `pos/1` (§7.1), `exchange/1` (§7.2 — the on-ramp as a profile), and `goods/1`. Added §7.3 cross-profile mechanics: refunds, cancellation/no-show, standing mandates, multi-payee splits, with `REFUND`/`CANCEL`/`MANDATE` in the §6 registry. Added §6.2 — timeouts on every state, plus the single-sided receipt for the post-FUND/pre-RECEIPT window. Fixed the Monero output-lock flaw in §17.2 (pre-split the float; capacity is over unlocked outputs, not balance). Build order resequenced: `pos/1` before `ride/1`. O15 added.
- **0.3** — **The settlement leg is a network path too.** Added §2.4 (two networks, two dependencies) and §8.7 (network path for settlement), specifying submission and scanning over Veilid, why Veilid does *not* replace Dandelion++, and why full monerod-over-Veilid is rejected. Added the `relay/1` profile (§8.7.2) — node access as a staked, verifiable service — answering "whose monerod?" without reintroducing a light-wallet server. T1 amended; O13–O14 added; Phase 0 split into 0a/0b and node access promoted into Phase 1.
- **0.2.1** — Consistency pass over the 0.2 merge. Reconciled `TapBlob`/`TapPresent` (§5.1 now points to the normative §15.3), completed §6's message registry with the Part II–IV additions, fixed the session-key lifetime contradiction between §4 and §16.3, added the missing `rated` value to `amount_authority`, named the static-tag object `TapStatic`, corrected a stale "Addendum A" cross-reference, and put Phase 0 before Phase 1. No design changes.
- **0.2** — Merged Parts II–IV into one document. Added `fast/1` settlement mode (§8.6, detailed §17) resolving the block-time/UX collision. Added reference-currency denomination and price oracle (§17.7). Added persistent-contact identity tier (§16). Build order and open problems updated; O8–O12 added.
- **0.1** — Initial core protocol skeleton.

---

## 1. Vision

DUCAT is a protocol, not an app. It specifies how two strangers with phones transact — discover each other, negotiate a service, exchange value, and part ways — with the privacy properties of a cash transaction and no operator in the middle.

The design target is a world where **private tap-to-pay is the default**, the way cash once was: final, bearer-held, requiring no account, no permission, and no third party's knowledge. Rides, person-to-person transfers, lodging, messaging, and file exchange are *profiles* layered on one shared transaction core, the way HTTP verbs layer on TCP.

**The unit of ambition is the card terminal, not the rideshare app.** Every scenario the protocol handles — a cab fare, a coffee, a tip, a busker, a market stall, a friend paid back, a donation box, a bar tab, a machine in the wall — is a point in one small grid of three fields (§15.2). There is no per-service plumbing. A protocol that gets the tap right gets all of them at once, which is why §7's profile list is open-ended rather than a product roadmap.

Rides are the **hardest** profile, not the representative one: a moving counterparty, a price that must be *derived* rather than stated, a delivery proof requiring both parties present at a destination neither controls, sub-three-second settlement, and real physical stakes. They are the proving ground precisely because anything that handles a ride handles a coffee. But the addressable surface is every place a card reader sits today — and every place one cannot.

| Today's rail | What it costs | What DUCAT removes |
|---|---|---|
| Card terminal | Interchange, terminal rental, and a merchant account you can be denied or lose | The account, the rental, the cut, and the processor's record of every sale |
| Platform marketplace | A large take rate, deactivation risk, a retained history of every trip | The operator — there is no one to take a cut, and no one who can switch you off |
| P2P payment app | A phone-number social graph, reversible transfers, freezable balances | The directory and the reversal |
| Cash | Nothing — but it can't be quoted, metered, sent, or proven | Nothing. DUCAT keeps cash's properties and adds a receipt only the two parties hold |

### 1.1 Cash-Parity Axioms

Every design decision is tested against how physical cash behaves:

| Axiom | Cash property | DUCAT realization |
|---|---|---|
| A1. Bearer | Whoever holds it, owns it | Keys on device; no accounts, no custodians |
| A2. Final | Handing over cash settles | Monero settlement; no chargebacks; disputes handled *before* release via escrow, never after |
| A3. Blind | The mint doesn't know where a bill goes | Monero (ring signatures, stealth addresses) + Veilid private routes; no party outside the transaction learns it occurred |
| A4. Permissionless | No one approves a cash sale | No registration, no KYC gate in the protocol itself |
| A5. Offline-tolerant | Cash works in a dead zone | Degraded modes for settlement when one/both parties lack connectivity (§8.4) |
| A6. Operator-free | Cash has no server bill | Veilid nodes are the participants; marginal infrastructure cost ≈ 0 |

### 1.2 Non-Goals

- **Not a global marketplace.** DUCAT is a federation of local markets (§10). There is no global order book to bootstrap or shard.
- **Not identity-free trust.** Anonymity is preserved at the *network and ledger* layers. Counterparties in physical-service profiles (rides, lodging) see each other's faces. The protocol removes *records*, not *presence* (§2.3).
- **Not a token.** No new coin. Settlement is Monero. The protocol earns nothing by default; a transparent, user-removable maintenance tip is the only funding hook (§11).
- **Not an arbiter of truth.** Dispute resolution is an opt-in market of arbiters, themselves DUCAT participants (§9.3).

---

## 2. Threat Model

### 2.1 Adversaries

- **T1 — Passive network observer** (ISP, IXP tap, airport Wi-Fi): must learn nothing beyond "device runs Veilid." Meeting this requires addressing the *settlement* path as well as the contract path (§2.4, §8.7) — Veilid hygiene alone does not deliver it.
- **T2 — Mass-surveillance aggregator** (data brokers, ad-tech SDKs, payment networks): the protocol emits no exhaust for them; no map-API calls, no payment-processor telemetry, no push-notification metadata tied to transactions.
- **T3 — Malicious counterparty**: fraud (non-payment, non-delivery), fare manipulation, double-spend attempts, reputation gaming, sybil flooding.
- **T4 — Malicious DHT participant**: record poisoning, eclipse attempts on discovery keys, harvesting of service adverts for surveillance.
- **T5 — Compelled third party**: there is deliberately no third party who *can* be compelled to produce transaction records. Arbiters (§9.3) hold only what disputants voluntarily disclosed.

### 2.2 Out of Scope

- Endpoint compromise (malware on the phone).
- Physical coercion at point of service.
- Global passive adversary with full-take on both Veilid path endpoints simultaneously (inherited limitation from onion-style routing).
- Monero protocol-level deanonymization (inherited; tracked as upstream dependency risk).

### 2.3 Honest Anonymity Accounting

For proximity profiles, the counterparties are physically co-present; the network's anonymity guarantees do not extend to the curb. What DUCAT guarantees is **record absence**: no server logs, no ledger entry legible to outsiders, no subpoena target. Design docs and UX must state this plainly rather than implying rider/driver mutual anonymity that doesn't exist.

### 2.4 Two Networks, Two Dependencies

DUCAT composes two systems and inherits the weaknesses of both. Two consequences are load-bearing enough to sit in the threat model rather than in an appendix:

- **The settlement leg is a second network path.** Contract negotiation rides Veilid; transaction broadcast and wallet sync reach the **Monero P2P network** separately, by a different route, with a different fingerprint. A device mid-transaction is running two clients, and T1's guarantee is only as strong as the weaker path. Left unaddressed, a clearnet monerod connection undoes every metadata property Parts I–III establish, precisely at the moment of payment. Addressed in §8.7.
- **Maturity is asymmetric, with a concrete instance.** Veilid issue #395 describes an importer-deanonymization side channel: liveness pings to a maliciously-supplied private route's first hop reveal the importer's address to whoever published the route. DUCAT's tap does exactly this — it imports a route handed over by an untrusted counterparty (§15.10). This is one open issue, not a verdict on the project, but it is the right kind of reminder that the newer half of the composite has not been through the adversarial decade the older half has. More generally: Monero's anonymity set is large and adversarially tested, while Veilid is young, and private-route safety is a function of network size — there must be enough nodes to route through. For the *metadata* properties, the composite is only as strong as the newer half, and at seed-market scale (§10) the Veilid network DUCAT runs on may be small. This is an upstream dependency risk on par with §2.2's Monero entry, and it cuts against the cold-start strategy: the smallest markets are the ones with the weakest routing.

### 2.5 Case Study: RetoSwap, May 2026

The closest production analogue to this protocol's trust machinery was drained of roughly 7,000 XMR (~$2.7 M). It is worth stating precisely, because it is the nearest thing to a real-world attack on the architecture described here — and because the layer it broke is the one people assume is the safe half.

**RetoSwap** is a Monero DEX forked from **Haveno**: peer-to-peer trades settled into a **2-of-3 multisig** between buyer, seller, and an arbitrator, messaged over Tor. That is §17.2's `FLOAT` and §9.3's arbitration, in production, since 2024.

The root cause, from the published post-mortems: a client **accepted a forged, out-of-order ACK message** and overwrote the arbitrator's stored network address with an attacker-controlled one, **without verifying the sender against the arbitrator's known public key**. The attacker then stood in as arbitrator, hijacked multisig wallet creation, and took the funds as they were deposited. It was the second exploit in the same protocol.

**Nothing about Monero failed.** The multisig performed exactly as instructed; it was instructed by an impostor. The break was in the *messaging layer* — which in DUCAT is Veilid, not Monero. A privacy-preserving transport carries an attacker's messages as faithfully as anyone else's, and route anonymity is not authentication.

Four rules in this document are the direct countermeasures, and this is the evidence they earn their cost:

| Rule | What it refuses |
|---|---|
| §18.4 — an unlisted message in a state is `STATE_VIOLATION`, **never a silent ignore** | An out-of-order ACK is exactly this. The state machine rejects it before it can mean anything. |
| §18.3 — domain-separated verification over **bytes as received** | A message accepted without checking the signer against a known key |
| §10.1 — the arbiter set is pinned in a **threshold-signed market descriptor** | Learning an arbiter's address from a message that can be spoofed |
| §18.8 — strict rejection of unknown fields and messages | Anything that "looked close enough to work" |

Two lessons that outlive the patch:

- **Peer addresses must never be learned from unauthenticated messages.** Any party whose identity matters is pinned by key, in a signed object, before the exchange begins. If a message can change who you are talking to, it can change who you are paying.
- **Maturity is not protection.** Haveno had years of development, a live user base, and a prior exploit to learn from, and was breached again. This document's arbitration and multisig surface is comparable in complexity and has had **no adversarial review whatsoever**. Nothing here should hold anyone's money until that changes.

---

## 3. Layered Architecture

```
L5  TRUST        stakes, attestations, arbitration
L4  SETTLEMENT   Monero: direct / fast / escrow / deferred
L3  CONTRACT     offer → quote → accept → receipt (signed state machine)
L2  DISCOVERY    DHT adverts, geohash cells, proximity tap
L1  IDENTITY     persona keys, session keys, rotation policy
L0  TRANSPORT    Veilid private routes, app_call/app_message; NFC/BLE for tap
                 settlement reaches the Monero P2P network by a separate path (§8.7)
```

Each layer is independently versioned. Crypto agility is mandatory: every signed object carries a cipher-suite identifier (Veilid's own upgradeable-crypto convention is adopted wholesale).

---

## 4. L1 — Identity

Three key classes, strict separation:

- **Session keys** — ephemeral X25519/Ed25519 pair per transaction. Generated at HAIL (or at the tap, §15.3); used for all L3 contract signatures in one-shot transactions. They outlive `RECEIPT` by a short **contact window** (default 120 s) so the optional `CONTACT` coda (§16.3) can run, then are destroyed with the session. Nothing survives teardown unless a persistent contact was explicitly established.
- **Persona keys** — long-lived Ed25519 identity a *service provider* chooses to maintain to accrue reputation (§9.2). A provider may hold many personas; personas are unlinkable by construction (separate Veilid route sets, separate Monero wallets, no cross-signing). Consumers never need one.
- **Stake keys** — key holding a provider's bonded deposit in a slashing multisig (§9.1). Bound to exactly one persona.

**Linkability trade, stated explicitly:** a persona's attestation history is a pseudonymous dossier. The protocol's job is to make persona use *optional per role*, rotation cheap, and cross-persona linkage cryptographically absent — not to pretend reputation and unlinkability fully coexist.

### 4.1 Key Storage, and What the Hardware Actually Offers

§16.8 refers to a "protected store" without saying what one is. The answer is platform-constrained in a way that reaches back into the wire format.

**iOS Secure Enclave supports P-256 (NIST) ECC only.** There is no hardware-backed Ed25519 or X25519 on iOS. A specification that mandates Ed25519 everywhere therefore forecloses hardware-backed keys on an entire platform — which is the concrete, immediate reason the `suite` field earns its byte, ahead of any post-quantum argument. Android Keystore and StrongBox vary by device and OS version and cannot be assumed either.

Consequently the suite registry MUST include a **P-256 suite** (ECDSA-P256 signatures, ECDH-P256 agreement) alongside the Ed25519/X25519 default, so an iOS client can hold reputation- and money-bearing keys in the Secure Enclave rather than in application memory.

Policy differs by key class, and the differences are not arbitrary:

| Key class | Storage | Rationale |
|---|---|---|
| **Session** | Software, ephemeral | Lives minutes and dies at teardown; hardware backing buys almost nothing and costs generation latency inside a 3-second budget |
| **Persona** | Hardware-backed where available | Long-lived, reputation-bearing, and the thing a lost device actually loses (O12) |
| **Stake / bond** | Hardware-backed **and** biometric- or passcode-gated | Directly spendable collateral; the highest-value key on the device |

**The honest limit:** hardware backing prevents key *extraction*. It does not prevent malware from asking the enclave to sign while the user is looking at something else. It raises the cost of a stolen phone, not of a compromised one — and §2.2 already places endpoint compromise out of scope. This is defense in depth, not a new guarantee.

**Cross-platform consequence:** a persona created under the P-256 suite is unverifiable by a client implementing only the Ed25519 suite, which would fragment personas by platform. Core conformance therefore requires *both* suites (§18.10).

---

## 5. L2 — Discovery

Two modes; the tap is primary.

### 5.1 Proximity Tap (primary primitive)

The physical-world equivalent of walking up to a taxi. QR, NFC, or BLE — whichever both devices support, in that preference order (§15.3.2) — conveys a **`TapPresent`**: a compact signed blob carrying, at minimum, the presenter's ephemeral session key, a route back, and a commitment to the offer that follows over the channel:

```
TapPresent {
  v, suite,
  profile,             // e.g. "ride/1", "xfer/1"
  session_pk,          // presenter's ephemeral key for this tap
  route,               // Veilid private-route import blob (or BLE fallback flag)
  offer_commit,        // H(FullOffer) — binds the offer sent over the channel
  nonce, expiry, sig
}
```

**Normative definition: §15.3**, which adds the role/authority/intent fields that generalize the tap beyond rides. The tap is a *bootstrap*, not the transaction (§15.1): the priced offer flows over the channel the tap opens, never across the gap itself. The medium carrying it is interchangeable — QR, NFC, or BLE (§15.3.2).

Tap → both devices open a Veilid (or direct BLE L2CAP, if offline) session → L3 contract state machine runs. Total target time to fare-lock: **< 3 seconds**.

### 5.2 Remote Hail — Providers Listen, They Do Not Advertise

For "find a ride to the airport" rather than "I'm standing at the taxi line."

Earlier drafts had providers publish signed adverts — profile, geocell, route — into rotating DHT keyspaces, and consumers read them and hailed. That design is withdrawn. It made supply a public bulletin board, which is the entire reason O3's harvesting problem existed, and it pointed Veilid #395 (§15.10) at exactly the wrong party: a provider could learn the network address of everyone who hailed them, while the consumer — who had more to lose and merely wanted a ride — carried the risk.

**Two observations reshape it.**

First, **reading a DHT record does not import a route.** Veilid #395 bites only on *importing* a counterparty-supplied route to open a channel. So the entire matching phase can run over DHT reads and writes with no imports at all, and the single import that remains happens after both parties have chosen each other.

Second, **the publisher learns the importer's address, not the reverse.** Publishing is safe; importing is what exposes you. So the consumer publishes a route and the provider reaches back — inverting the direction of risk onto whoever chooses to initiate contact.

#### 5.2.1 The flow

```
  provider          market hail record (DHT)          consumer
     │                        │                          │
     │◀── watch_dht_values ───┤                          │
     │   (publishes nothing)  │                          │
     │                        │◀── 1. HAIL ──────────────┤
     │                        │    profile, coarse cell, │
     │                        │    nonce, ephemeral pk   │
     │                        │    NO ROUTE              │
     │                        │                          │
     ├── 2. OFFER ───────────▶│                          │
     │   sealed to consumer's │──────────────────────────▶
     │   ephemeral pk         │   only the consumer       │
     │   NO ROUTE             │   can read it             │
     │                        │                          │
     │                        │◀── 3. route, sealed to ──┤
     │                        │    the chosen provider   │
     │◀───────────────────────┤                          │
     │                                                   │
     └── 4. imports it, opens the channel ──────────────▶│
                    one import, mutually consented
```

Providers subscribe to their market's hail record for a geocell (§10.1) and publish nothing. A consumer writes a hail carrying a coarse cell and an ephemeral public key but **no route**. Interested providers answer with an offer sealed to that ephemeral key — readable only by the consumer, and still carrying no route. The consumer selects one, seals its own route to that provider's key, and the provider imports it.

#### 5.2.2 What this fixes, and what it does not

**Providers become invisible.** A harvester watching a market learns nothing about supply — there is no advert, no persistent presence, no standing dossier of who works where. It learns only that *someone* hailed from a coarse cell at a time, which is ephemeral and unattributed. This is the substance of O3's improvement, and it also retires geocell epoch rotation, per-cell advert encryption, and the rate-card distribution sub-problem, all of which existed only to make persistent adverts survivable.

**#395 is contained, not closed.** One exposure per real transaction, to a counterparty the exposed party selected, instead of one per browse to anyone watching. That is the same posture the proximity tap already has and §15.10 already accepts. The upstream fix is still needed before the final import is clean.

**Hail spam is unsolved.** Anyone can write to a market record. Per-subkey rate limits in the record schema are the cheap mitigation; requiring a stake or bond proof to post is the strong one, at the cost of pulling consumers toward collateral — the same pressure O15 describes.

**Latency is two DHT round trips plus watch propagation.** Seconds, not milliseconds. Fine for summoning a ride, far too slow for a tap, which is why §5.1 remains the primary primitive.

#### 5.2.3 Location disclosure, and why there is no map of drivers

The obvious product question is whether a rider can see nearby drivers on a map. **No, and not because of an implementation gap.**

A live map of drivers requires drivers to continuously publish their positions. That is precisely the persistent advert this section removes, and in a permissionless system it is strictly worse than the platform it replaces: a centralized operator holds that data privately, whereas a DHT-published one hands every worker's movement history to anyone who cares to watch. A protocol that deletes the operator and then publishes the surveillance database has achieved nothing.

What is available instead is a **disclosure ladder**, tightening only as consent is given:

| Stage | Consumer reveals | Provider reveals |
|---|---|---|
| Hail | Coarse geocell (district) | Nothing |
| Offer | Nothing further | Session key, terms, offer |
| Selection | Route, sealed to the chosen provider | Nothing further |
| After mutual accept | Precise pickup point, over E2EE | Precise position, over E2EE |
| During service | Live position, over E2EE | Live position, over E2EE |

**The map exists — it just lives after the match rather than before it.** Once two parties have selected each other, live position sharing over the E2EE session is both safe and expected: they have consented, and they are about to be physically co-present anyway (§2.3). Watching your driver approach works exactly as riders expect. What does not exist is browsing strangers' locations before any relationship exists.

Before matching, the honest UI is *"looking for a driver"* rather than a map of six of them — which is what hailing a cab has always looked like. A market MAY publish a coarse aggregate supply signal ("providers active in this cell in the last 15 minutes") to distinguish thin supply from none, but only above a k-anonymity floor: a count of one in a small cell identifies a person.

**Optional visibility is not optional.** If providers may opt into publishing position, those who do will win more work, and the pressure to opt in becomes economic rather than free. Any such feature therefore re-creates the harvesting problem for the whole market, not just for volunteers, and is excluded for the same reason one-way contact is (§16.8).

---

## 6. L3 — Contract State Machine

All messages are canonical CBOR, signed, and chained (each carries the hash of its predecessor), so a completed transaction yields a compact, self-verifying transcript held only by the two parties.

```
ADVERT          provider → world          rate card, profile, terms, stake proof
HAIL            consumer → provider       session pubkey, profile request
QUOTE           provider → consumer       priced offer (deterministic from rate card + inputs)
ACCEPT          consumer → provider       signed fare-lock; names settlement mode
FUND            consumer → escrow/provider  settlement initiation (mode-dependent, §8)
TXPROOF         payer → payee             tx proof enabling zero-conf accept (fast/1 only, §17.4)
PROOF           either → either           delivery evidence (profile-defined)
RELEASE         consumer → escrow         escrow disbursal (escrow mode only)
RECEIPT         both                      co-signed closure; input to attestation
SETTLED         local                     finality observed; fast/1 obligation clears (§17.4)
ABORT           either                    pre-FUND cancellation, no penalty
DISPUTE         either → arbiter          post-FUND escrow contest (§9.3)
CANCEL          either                    post-ACCEPT cancellation; invokes terms.cancellation (§7.3)
REFUND          payee → payer             voluntary, receipt-bound reverse payment (§7.3)
MANDATE         payer → payee             capped standing authorization; unilaterally revocable (§7.3)
CONTACT_OFFER   either → either           optional post-RECEIPT identity coda (§16.3)
CONTACT_ACCEPT  either → either           completes the mutual contact (§16.3)
```

**The in-person tap collapses the first three.** `TapPresent` (§15.3) carries the advert commitment and the hail in one gesture; `FullOffer` (§15.4) *is* the QUOTE, delivered over the channel the tap just opened. The remote-hail path (§5.2) runs the same three roles over DHT records rather than a channel: the consumer's HAIL is a record write carrying no route, the provider's QUOTE is a sealed reply, and no ADVERT exists at all because providers no longer advertise. One state machine, two entry paths — this equivalence is normative, and a client that implements only the tap path must still produce transcripts a remote-hail client can verify.

### 6.1 Deterministic Pricing

A QUOTE must be reproducible: `price = f(rate_card, route_inputs)` where `f` is specified per profile and computed **locally** (on-device OSRM/Valhalla for rides; no external map or pricing API in the loop). The rate card is committed before the price is seen — in the ADVERT for remote hails, and via `offer_commit` in the `TapPresent` for in-person taps (§15.3), which binds the `FullOffer` carrying it — so a provider cannot quote off-card without detection. This makes pricing *more* auditable than any surge algorithm — transparency as a feature, not a compliance cost.

### 6.2 Timeouts and Failure Transitions

A state machine without deadlines is unimplementable. **Every await has a deadline and a defined transition**, because a dead Veilid route is indistinguishable from a silent counterparty and both must resolve the same way. Defaults, overridable per profile:

| Awaiting | Default | On expiry |
|---|---|---|
| `FullOffer` after tap | 10 s | Discard silently; no screen ever shown to the human |
| ACCEPT (offer displayed) | `TapPresent.expiry` (≤ 30 s) | ABORT, no penalty |
| FUND after ACCEPT | 60 s | ABORT, no penalty |
| TXPROOF (`fast/1`) | 30 s | Provider falls back to `direct` (wait for confirmations) or ABORTs |
| PROOF of delivery | Profile-defined | Profile-defined; `ride/1` falls to single-sided receipt |
| RECEIPT co-signature | 120 s | **Single-sided receipt** (below) |
| Multisig setup (escrow) | 300 s | ABORT + fund recovery path (§8.2) |
| RELEASE (escrow) | Profile-defined | DISPUTE becomes eligible (§9.3) |
| Confirmation (`fast/1`) | 20 blocks | CLAIMED — the cure window (§17.5) |
| Contact window post-RECEIPT | 120 s | Session teardown; session keys destroyed (§4) |

**The dangerous window is post-FUND, pre-RECEIPT** — the payer's money is gone and the co-signed record does not yet exist. A counterparty that vanishes here must not be able to erase the transaction, so the payer's client emits a **single-sided receipt**: its own signed record of `{ ACCEPT, TXPROOF, timestamp }`, valid as dispute evidence (§9.3) and as an attestation input (§9.2), and explicitly flagged as unilateral. It proves what the payer signed and paid; it cannot prove delivery, and it does not claim to.

**Clocks, and what happens when they disagree.** Every deadline above, plus `expiry` (§15.3), `rate_ts` (§17.7), and bond-attestation freshness (§17.4), depends on time — and phones have wrong clocks. NTP cannot be assumed: a device at the curb may have no network, and an NTP call is itself a network fetch with the leak profile §17.7 exists to avoid.

- **Elapsed time uses a monotonic clock.** Every timeout in the table above, every cure window, every meter duration. Monotonic time is immune to wall-clock skew and to the user changing the date, and there is no reason for these to consult wall time at all.
- **Absolute time gets a skew tolerance.** `expiry`, `rate_ts`, `ts`, and attestation freshness are wall-clock and vulnerable. Checks apply a default **±120 s** tolerance.
- **Widening tolerance widens replay.** The seen-nonce cache (§18.5) must therefore retain for `max_accepted_expiry + 2 × skew_tolerance`, not merely for the expiry window.
- **Both directions fail closed.** A generous reading of `expiry` accepts stale offers; a generous reading of `rate_ts` transacts on a stale exchange rate. Neither is a safe default, so uncertainty resolves to refusal in both cases.
- **Detect skew; never apply it.** A client whose clock disagrees with counterparties by more than the tolerance SHOULD tell *its own user* that this device's clock looks wrong — otherwise a phone three hours out appears to be surrounded by broken counterparties and the user has no way to diagnose it. But a counterparty's clock MUST NOT be used to correct your own: a hostile presenter that can nudge your clock forward can make a stale rate look fresh.

---

## 7. Profiles

A profile pins the abstract core to a concrete service: what a QUOTE prices on, what PROOF means, whether escrow is required, and geocell precision. New services ship as new profiles without touching L0–L4.

| Profile | QUOTE inputs | PROOF of delivery | Settlement default | Notes |
|---|---|---|---|---|
| `xfer/1` | amount | none (delivery = payment) | direct | Simplest possible; person-to-person send |
| `ride/1` | origin, dest, rate card | co-signed arrival (both tap at drop-off) | direct | Immediate physical delivery; escrow optional |
| `chat/1` | — | — | none | Veilid E2EE session; no settlement |
| `file/1` | size, optional price | hash of received blob | direct or escrow | Escrow when paid; PROOF = recipient signs content hash |
| `lodging/1` | dates, rate card | check-in + end-of-stay co-sign | **escrow required** | Time gap ⇒ escrow + stake mandatory (§9) |
| `task/1` | scope, price | co-signed completion | escrow | Generic labor; open-ended PROOF is the hard part |
| `pos/1` | basket of line items | none (delivery = goods handed over) | direct | Merchant sale. The highest-volume real case (§7.1) |
| `goods/1` | item, price | co-signed handoff; content hash if digital | direct, or escrow if not hand-to-hand | Objects rather than labor |
| `exchange/1` | amount, direction, rate | co-signed receipt of both legs | direct | Cash ⇄ XMR between two humans; the on-ramp (§7.2) |
| `relay/1` | — | tx observed in mempool/chain | none or direct | *Infrastructure, not commerce:* Monero node access as a staked, verifiable service (§8.7.2) |

**Escrow-required profiles are gated behind the maturity of L4 escrow and L5 trust.** The buildable-now set is larger than it first appears: `chat`, `file` (direct), `xfer`, `pos`, `goods` (hand-to-hand), `exchange`, and `ride` (direct) all settle directly and need no escrow at all. Lodging and open-ended tasks remain deferred until escrow and staking are proven. Note that `pos/1` reaches real commerce with none of `ride/1`'s routing machinery — see §13.

### 7.1 `pos/1` — Merchant Point of Sale

The largest real market, and the one that needs the protocol least dramatically and most often. A merchant presents, a customer taps, the customer's app renders and verifies, both walk away holding a receipt.

`FullOffer` for `pos/1` carries a `basket`:

```
basket {
  lines  : [ { sku?, label, qty, unit_price, line_total } ]
  subtotal, tax?, tip?, total
  currency_ref                 // reference currency, converted per §17.7
}
```

WYSIWYS (§15.5) applies unchanged and does most of the work: the customer's app **recomputes** subtotal, tax, and total from the lines rather than displaying the merchant's stated figure, and refuses any basket whose arithmetic doesn't reconcile. Line *labels* are advisory counterparty strings and MUST be rendered as such — the same rule that governs destination labels in rides. A hostile terminal cannot show "Coffee — 2.50" while charging 25.

Merchant-specific needs, none of which require new layers:

- **Staff terminals.** A merchant persona (§4) authorizes N *terminal keys*, each signing `TapPresent` on the persona's behalf under a signed delegation carrying its own expiry. Revoking a terminal is republishing the delegation set; a lost phone costs a device, not the persona or its accumulated reputation.
- **Day-end reconciliation.** The merchant's receipts *are* the books, and because both parties co-sign, they are cryptographically stronger than a processor statement — while remaining the merchant's alone to keep or destroy. Worth stating plainly in §12: the protocol produces no record for anyone else and a *better* one for the person who actually needs it.
- **Offline lane.** A stall or festival vendor with no connectivity is a common case, not an edge case. Until §8.4 resolves, `pos/1` degrades honestly rather than silently: the merchant holds the signed ACCEPT and TXPROOF and confirms when connectivity returns — bounded by the customer's bond where one exists (§17), and declined where it does not.

### 7.2 `exchange/1` — The On-Ramp Is a Profile

To transact, a consumer needs XMR; to transact *fast*, they also need a bond (§17). That is two acquisition steps before a first tap, and it is the real shape of §10's cold-start wall — not a shortage of drivers, but a shortage of anyone holding the settlement asset.

`exchange/1` makes the on-ramp a DUCAT service: two people meet, one hands over cash, the other sends XMR, both co-sign. It is the same move §9.3 makes for arbitration and §8.7.2 makes for node access — **the network bootstraps itself with itself**, and every piece of infrastructure the protocol depends on becomes a market its own participants can serve.

- **Both directions matter equally.** A driver earning XMR who pays rent in currency needs the off-ramp exactly as much as a rider needs the on-ramp, and a market with only on-ramps starves its supply side within a week. `direction` is a first-class field, not a mode.
- **The rate is negotiated, not oracled.** Unlike a fare (§17.7), an exchange *is* a price negotiation — the counterparty's rate is the product. The payer's app shows it against the market's cached reference rate as a **spread**, not as an error.
- **Physical handoff is the PROOF.** Cash changed hands or it didn't, and both parties co-sign that it did. There is no escrow because there is nothing to escrow. This is the most cash-like profile in the protocol and the most exposed to §9.4's safety floor, which applies here with full force.
- **Regulatory surface is highest here** (§12). Person-to-person cash-for-crypto is the activity most likely to be regulated as money transmission or exchange in a given jurisdiction, and that is true whether or not an operator exists. The protocol takes no fee and routes no funds through anyone, but a *participant* running `exchange/1` as a business is in a materially different position from one taking a ride. The UX must say so, and a real legal opinion is required before this profile is promoted anywhere.

### 7.3 Cross-Profile Mechanics

Four things every commercial profile needs, specified once instead of per-profile.

**Refunds.** A2's finality is a property of the *ledger*, not a prohibition on commerce. A merchant issuing a refund is not reversing a transaction — they are making a new, voluntary one. `REFUND` (§6) is an `xfer` bound to a prior receipt: `{ prior_receipt_hash, amount, txid, out_proof, sig }`, partial or full, producing its own co-signed receipt. It is payee-initiated only and can never be compelled; a customer refused a refund has exactly the recourse they have at a market stall today, which is reputation (§9.2). Building the clawback would mean building the arbiter that can seize funds — that is precisely the party DUCAT deletes.

**Cancellation and no-show.** ABORT is free pre-FUND, which is correct and insufficient — a rider who cancels after the driver has driven ten minutes has imposed a real cost. `CANCEL` covers the post-ACCEPT window and invokes `terms.cancellation` from the offer the payer already signed: a fee schedule, typically time-graded, that was visible on the confirm screen. The fee settles from the canceling party's bond (§17) where one exists. **A cancellation fee is only enforceable against collateral** — against an unbonded counterparty it is uncollectable, and the spec does not pretend otherwise. Providers price that risk through `accept_unbonded` policy (§17.6).

**Standing mandates.** Rent, dues, a weekly delivery, a subscription. A `MANDATE` is a payer-signed authorization for a named payee to request up to `cap` per `period` until revoked, bound to a persona pair (§16) rather than to a session. Two properties are non-negotiable, and together they are the entire difference from a card-network subscription: the cap is enforced by the payer's *own* client, and **revocation is unilateral, instant, and requires no cooperation from the payee.** You stop honoring requests and that is the end of it — no cancellation flow to navigate, no retention offer, no one to email.

**Multi-payee splits.** §15.8 splits one bill across N payers. The mirror — one payer, N payees — is a band splitting a door take, a courier relay, a driver and a vehicle owner dividing a fare. Monero settles multiple outputs in a single transaction, so this is an offer field rather than new machinery: `payout_split : [ { payto, share } ]`, committed under `offer_commit` and verified by the payer's app before signing. Every payee's share is visible to the payer, which is the honest default — **a split you cannot see is a fee.**

---

## 8. L4 — Settlement (Monero)

Monero has no scripting layer. This is the single hardest constraint in the protocol and it shapes L4 entirely. Four settlement modes ship — direct (§8.1), escrow-multisig (§8.2), escrow-bond (§8.3), and fast (§8.6). Two further tracks (§8.4, §8.5) are research, not modes.

### 8.1 Direct
Consumer sends XMR to a provider subaddress conveyed in QUOTE. No recourse. Correct **only** when delivery is immediate and concurrent with payment (rides, transfers, live file send). Simplest, most cash-like, ships first.

### 8.2 Escrow — 2-of-3 Multisig
Buyer, seller, and a mutually chosen arbiter (§9.3) form a Monero 2-of-3 multisig. Happy path: buyer + seller co-sign RELEASE, arbiter never touches it. Dispute: arbiter co-signs with the party it rules for.

**Known hazards (must be engineered around, not assumed away):**
- **Monero ships multisig disabled by default, and says why.** Quoted from v0.18.5.1's own refusal, because a protocol whose escrow depends on this feature should carry its maintainers' assessment verbatim rather than paraphrased: *"Multisig is an experimental feature and may have bugs. Things that could go wrong include: funds sent to a multisig wallet can't be spent at all, can only be spent with the participation of a malicious group member, or can be stolen by a malicious group member."* Enabling it requires a per-wallet flag set through `monero-wallet-cli`; **there is no RPC method**, so a client must drive the CLI out-of-band or link the wallet library and bypass the RPC. This constrains client architecture more than any fragility does.
- **A wallet can be stranded halfway.** `prepare_multisig` and `make_multisig` both succeed with the flag off; only `exchange_multisig_keys` refuses. The wallet is then multisig-but-unfinalized, and `prepare` rejects it as already multisig. There is no rewind — recovery means discarding it and restarting with all three parties. **Check the flag before step 1**, because nothing checks it after.
- Multi-round key exchange is *not*, in measurement, the fragile part: 2 rounds, 134 s, deterministic (§O1). Earlier drafts treated wallet-sync and key-exchange failure as the primary hazard; that was inherited caution rather than an observation.

**The intended path avoids wallet2's multisig entirely.** DUCAT clients embed a wallet rather than driving `monero-wallet-rpc`, which removes the missing-API problem by construction and — more importantly — allows a different multisig implementation. `monero-oxide`'s `monero-wallet` implements **FROSTLASS**, a formalized threshold signing protocol for CLSAGs, audited by Cypher Stack in May 2025, with O(1) per-signer upload against Monero's native O(n!). The upstream warning quoted above describes *wallet2's* implementation; it is not a statement about threshold signing on Monero in general.

Two consequences follow, and neither is optional:
- **All parties to a bond must run the same multisig scheme.** FROSTLASS is a different signing protocol from Monero's native multisig, so a FROSTLASS group and a wallet2 group cannot co-sign together even though both settle as ordinary CLSAGs on chain. A market therefore declares its scheme in the descriptor (§10.1), because a bond is only formable among parties who agree on one. **Interoperability between the two is undocumented and MUST be verified rather than assumed.**
- **Embedding moves the risk rather than deleting it.** A client that ships its own wallet owns that wallet's correctness, including its multisig. The trade is a smaller, audited, formally-analysed implementation against a larger, unaudited, self-described-experimental one — favourable, but it is a trade, and `monero-oxide` is pre-1.0 with API stability still pending. The protocol must treat multisig setup as a fallible sub-state-machine with explicit timeouts and fund-recovery paths, not a single call.
- Arbiter selection must precede FUND (the arbiter is a multisig participant from the start).
- Funds are unspendable if a party vanishes mid-setup ⇒ mandatory setup timeout returning to ABORT.

### 8.3 Escrow — Deposit/Bond (simpler alternative)
Rather than lock the *fare* in multisig, lock a **provider bond** (§9.1) and settle the fare directly. Cheaper cryptographically; shifts protection from "buyer's money is safe" to "seller has skin in the game." Good default for mid-value proximity services where the fare is small but repeat fraud must be deterred.

### 8.4 Offline / Deferred (research track)
Cash works in a dead zone; XMR does not. Options under study, none committed:
- **Signed payment promise** redeemable when the payer regains connectivity, backed by a slashable bond ⇒ trust-minimized IOU, not settlement.
- **Pre-funded channel** between frequent counterparties.
This is the least-solved axiom (A5) and is flagged as open research, not a v1 feature.

### 8.5 Adaptor Signatures (long horizon)
Scriptless-script escrow via adaptor signatures could give atomic escrow without a third-party arbiter. Research-grade on Monero today. Tracked as the eventual "right answer" for §8.2's ugliness; not on the near roadmap.

### 8.6 Fast — Bonded Zero-Confirmation
**The mode the flagship use case actually requires.** §8.1 is only "instant" in the sense that the payer clicks quickly; Monero's ~2-minute blocks and ~10-confirmation finality convention mean a driver either waits twenty minutes or accepts unconfirmed risk. `fast/1` resolves this: the consumer posts a small bonded float once, and thereafter a provider accepts an unconfirmed transaction in seconds against a bounded, provable, slashable downside.

The bond is set up **once**, off the critical path, which also amortizes the §8.2 multisig fragility into a calm retryable onboarding flow — and gives the consumer wallet a restore height of *now*, which is what makes self-custodial mobile Monero tractable at all.

Fully specified in **Part IV (§17)**. Providers choose per-profile whether to accept unbonded consumers in `direct` mode (slow, fully permissionless) or require a bond (fast, collateralized); the network runs both lanes.

### 8.7 Network Path for Settlement

Every mode above assumed the settlement leg is invisible. It is not. The contract rides Veilid; the transaction is broadcast to the Monero P2P network, which is reached separately (§2.4). This section specifies how, and states what it does not fix.

**Veilid does not replace Dandelion++.** The two defend different layers, and conflating them loses one of the defenses:

- **Dandelion++** is a *topology* defense. It exists to defeat a spy node **inside** the gossip network that peers widely and estimates a transaction's origin from relay timing. Its stem phase ensures the node that finally diffuses a transaction is not the one that created it.
- **Veilid private routes** are a *network-layer* defense. They hide the address behind a participant.

Carrying Monero's peer-to-peer traffic over Veilid would change what a spy node learns from an IP address to a route identifier — a real improvement, since an IP is far more identifying — but the first-spy estimator over the peer graph is structurally untouched. You would have relabeled the nodes, not broken the correlation. **Both defenses, each at its own layer.**

**Full monerod-over-Veilid is not the design.** Monero's `--tx-proxy` expects a SOCKS5 endpoint; Veilid's API is application-level messaging and DHT operations, not a stream transport. Bridging them needs either a tunnel shim — whose far end is an operator — or a patched daemon, whose small forked network would have *worse* anonymity than the main one. DUCAT therefore does not tunnel a node. It speaks the narrow interface a wallet actually needs, over Veilid, to a peer that happens to run one.

#### 8.7.1 The two things a wallet needs

**Submission.** The payer sends the signed transaction blob over a **fresh Veilid private route** to a relay, which admits it to the Monero network; Dandelion++ proceeds normally from there. Both layers stay intact. A hostile relay's powers are thin by construction: it can refuse to relay — detectable, since the payer is already watching for the transaction to appear, and under `fast/1` non-appearance is exactly what the cure window (§17.5) handles — or it can record that *someone* submitted this transaction. It never holds a view key and cannot attribute the submission to a person.

**Scanning.** The harder half, and Part IV already solved it without saying so. A float wallet has a restore height of *now* (§17.1), so "fetch every block since bond creation" is a bounded, affordable request rather than a years-long sync.

**Measured on stagenet.** A wallet with its restore height set 200 blocks back synced in **35 seconds** — 201 blocks at ~5.7 blocks/sec against a remote node. At that rate a full scan of stagenet's 2.18 M blocks would take roughly **106 hours**. §17.1's restore-height argument is not a convenience: it is the difference between half a minute and four days, and it is what makes a self-custodial phone wallet possible without handing a view key to a light-wallet server.

**Phase 0b measured this, and the answer is cautiously yes.** Round trips over a private route on veilid-core 0.5.7:

| Payload | RTT | Throughput |
|---|---|---|
| 1 KB | 186–235 ms | 4.3–5.4 KB/s |
| 4 KB | 217–325 ms | 12.3–18.4 KB/s |
| 16 KB | 164–262 ms | 61.0–97.2 KB/s |
| 32 KB | 157–237 ms | 134.7–203.7 KB/s |

**Latency dominates; payload size is nearly free.** RTT barely moves from 1 KB to 32 KB, so throughput scales almost linearly with request size — which means a scanning client should request the largest blocks it can per round trip and pipeline aggressively, not stream small reads. At 32 KB requests a day of Monero blocks moves in minutes, which is exactly the workload restore-height-of-now creates. A *full-chain* sync at these rates would take hours to days, which is precisely why §17.1's restore height is load-bearing rather than a convenience.

Caveats, since these are two samples: measured against a self-route rather than a real `relay/1` peer, sequential and unpipelined, and `app_call` caps payloads near 32 KB so bulk transfer means many calls rather than a stream. O14 is answered well enough to proceed and not well enough to design against. Pulling full blocks over a Veilid route leaks no query pattern, discloses no view key, and exposes no address — the three things a light-wallet server would otherwise learn. **Clients MUST NOT disclose a view key to a remote node.** The restore-height property is what makes that prohibition practical rather than aspirational.

#### 8.7.2 `relay/1`

Node access is a service, so it is a profile — the same move §9.3 makes for arbitration.

A `relay/1` provider advertises that it runs a Monero node and will submit transactions and serve blocks. It is staked (§9.1), optionally paid, and **verifiable**: the payer can confirm the transaction reached the mempool or the chain, so a relay that silently drops traffic is detectable and its stake is slashable. Unlike an arbiter, a relay exercises no judgment — only liveness — which makes it the cheapest possible thing to hold accountable and the easiest dispute class in the protocol.

Relay selection SHOULD be per-transaction and drawn at random from a market's advertised set; a client that always uses one relay has built itself a single observer. Submitting the identical transaction to several relays is harmless — Monero nodes deduplicate — and defeats a single dropping relay outright.

#### 8.7.3 Recommended posture, and its honest limit

In preference order:

1. **Tor for transaction submission**, where reachable. Tor-submitted Monero transactions join a large, established anonymity set. DUCAT cannot manufacture one and should not pretend otherwise.
2. **`relay/1` over Veilid** — the fallback, and the path for users who cannot reach Tor.
3. **Your own node, reached over a Veilid private route.** This is the tightest available composition of the two systems: self-custodial sync, no operator, no view-key disclosure, no clearnet exposure. Recommended for anyone who runs a node; not assumable for a phone-only user.

The limit, stated plainly: DUCAT users submitting through DUCAT relays are a small, distinguishable population. The transaction's anonymity set *on the Monero side* is unaffected — it joins the general mempool like any other — but **"who used a DUCAT relay" is its own observable**, and at seed-market scale that set is a few hundred people. This is §10's cold-start problem wearing a different hat: it improves only with adoption, and no protocol design fixes it early.

### 8.8 Transaction Fees

Monero fees are paid by the sender and vary with network conditions and transaction size. On a $3 fare that is not a rounding error, and the protocol currently leaves it implicit — `breakdown` (§15.4) itemizes the fare and says nothing about what the payer actually spends.

**`terms.fee_policy` is explicit in every offer:**

- **`payer_pays`** (default) — the payee receives `amount_pxmr` exactly; the fee is additional to the payer's outlay.
- **`payee_absorbs`** — the payee receives `amount_pxmr` minus the fee; the quote is gross.

**This has a WYSIWYS consequence, not just an accounting one.** Under `payer_pays`, the confirm screen MUST display the payer's **total outlay** — fare plus estimated fee — and not the fare alone. A payer who signs one number and spends another has been shown something untrue by their own client, which is exactly what §15.5 exists to prevent. The signed `ACCEPT` covers the fare; the screen shows both.

**Under `fast/1` the fee level is a protocol concern, not merely a cost.** §17.5's cure window exists for non-confirmation, and §17.8 names habitual fee underpayment as a way to slow-walk providers without ever being slashed. Closing it: a provider MAY name a minimum fee tier in `terms`, and a payer offering below it is refused with `POLICY_REFUSED` (§18.5) *before* the transaction rather than nursed through a cure window after it. Fee discipline becomes a pre-condition instead of an unenforceable norm.

Fee estimation follows the same rule as the price oracle (§17.7): obtained on a background schedule over the §8.7 relay path, cached, and **never fetched at tap time** — a fee lookup correlated with a transaction is the same leak as a rate lookup correlated with one.

---

## 9. L5 — Trust

The layer that Uber/Airbnb *are*, rebuilt without a company. No component here proves identity; all of it prices risk.

### 9.1 Bonded Stakes (cost-of-identity, not proof-of-identity)
A provider locks XMR against a stake key. Fraud, adjudicated via escrow dispute, slashes it. A scammer *can* regenerate a persona — but must re-bond each time, so fraud costs real money per instance. Stake amount is provider-advertised and consumer-visible in the ADVERT; the market prices trust. This is the sybil-resistance workhorse (T3), and it works precisely because it never tries to know who anyone is.

### 9.2 Pseudonymous Reputation
Attestations = RECEIPTs, optionally rated, signed to a persona key, stored in DHT records the persona controls. A consumer weighs advert stake + attestation history. Caveats made explicit in §4: reputation and unlinkability trade against each other; attestation-stuffing is countered by weighting attestations by *counterparty stake* (a review from a bonded counterparty costs something to forge), not by raw count.

### 9.3 Arbitration Market
Arbiters are DUCAT participants running an `arbiter/1` profile. Chosen per-transaction, before FUND, from the **market descriptor's signed arbiter set** (§10.1) by their own stake + reputation — *never* from an address supplied in a message, which is precisely how RetoSwap was drained (§2.5). Paid per dispute. They see only what disputants disclose (T5). Multiple arbiters can co-sign for higher-value escrows. This dogfoods the protocol — the dispute layer is itself a P2P service market — and means no central court, only a competitive field of stakers whose own bonds are slashable for provable misconduct.

### 9.4 Safety Floor (honest limit, and where it actually binds)

Bonds deter fraud, not assault. The protocol deliberately deletes the deactivation switch and the subpoena target that platforms provide, and for some profiles that is a real cost paid by real people.

**But the floor is profile-specific, and that distinction matters more than the original framing allowed.** What varies is whether the transaction puts your body in a stranger's control:

| Exposure | Profiles | What deleting the platform actually costs |
|---|---|---|
| **None** | `xfer`, `pos`, `goods` (hand-to-hand), `file`, `chat`, `relay` | Nothing. You stand at a counter, or never meet at all. No platform was protecting you here — the merchant across the counter is exactly as accountable as they have always been: present, local, and reputationally exposed. |
| **Bounded** | `exchange/1` | Real but manageable. You meet a stranger to hand over cash. Mitigated by meeting in public, which the protocol contributes nothing to and should not pretend it does. |
| **High** | `ride/1`, `lodging/1`, `task/1` | Genuine. You get into a car, sleep in a room, or let someone into your home. This is where a platform's deactivation switch and subpoena target did real work, and removing them removes real recourse. |

The constituency claim is therefore narrower than this section originally stated: **the cap binds the high-exposure tier, not commerce as a whole.** A coffee shop does not need a trust-and-safety department, and the largest profiles added in §7 sit entirely in the no-exposure row. §1's reframe is not in tension with this section — it mostly routes around it.

For the high-exposure tier the limit stands undiminished. What the protocol offers there is honest but thin: a bonded stake means a provider has money at risk (§9.1), and a persistent contact (§16) can be handed to a friend before you get in the car. Neither is a safety system, and a client that presents them as one is lying to its user. A market running high-exposure profiles should say so plainly in its own UX.

---

## 10. Federation & Cold Start

Two-sided markets die empty; there is no treasury to subsidize both sides. So DUCAT does not launch as "global decentralized Uber." It launches as **one dense, motivated, bounded market** and federates outward:

- A **market** is a namespace over geocells + profiles (e.g. a single conference, a single city's couriers). Adverts and reputation are scoped to it.
- Markets are discovered by well-known DHT keys; joining is subscribing to a market's keyspace. No global order book ever exists.
- Reputation is portable across markets at the persona's option (linkability trade again).
- Ideal seed markets are ideologically-motivated dense venues (hacker cons, privacy-community meetups) where both sides already want this to exist. Design for "works at one con," not "beats Uber in a metro."

### 10.1 The Market Descriptor

§17 made market arbiter sets load-bearing for fast settlement (O8), so "a market is a namespace" is no longer a sufficient definition. A market is a signed object:

```
MarketDescriptor {
  market_id      // H(genesis descriptor) — self-certifying, never chosen
  name           // advisory display string only; never a decision input
  geocells[]     // geographic scope
  profiles[]     // which profiles run here
  arbiter_set    // { members[], threshold, epoch, rotation_policy }
  multisig_scheme// which threshold scheme bonds use (§8.2). Parties cannot
                 // form a bond across schemes, so this is a joinability
                 // constraint, not a preference.
  relay_set[]    // optional advertised relay/1 providers (§8.7.2)
  rate_feed_key? // optional signed median-rate publisher (§17.7)
  suite_floor    // minimum acceptable cipher suite (§18.6)
  policy         // accept_unbonded defaults, cancellation norms (§7.3)
  epoch, expiry
  sig            // threshold signature by the previous epoch's arbiter set
}
```

**`market_id` is the hash of the genesis descriptor**, so it is self-certifying: nobody can hand you a different market under an id you already trust. Discovery is a well-known DHT key derived from `market_id`; joining is subscribing to that keyspace and leaving is ceasing to read it. There is no membership roll, which means there is nothing to purge and no one to ask.

**Arbiter-set rotation is a hash chain.** Each descriptor version is signed by a threshold of the *previous* epoch's set, so a client that has seen epoch N can verify N+1 unaided, and continuity is provable back to genesis. A client joining fresh must trust-on-first-use the genesis descriptor — which is precisely the same act as choosing which market to join, so the trust decision is surfaced rather than hidden inside a lookup.

**What this does not solve, stated plainly.** The chain proves *continuity*, not *honesty*. A market captured at genesis is captured forever, and a set that rotates itself into capture produces perfectly valid signatures the whole way. Divergent descriptors claiming the same lineage are a fork, and clients MUST surface a fork rather than silently picking a branch — but detection is not resolution. **O8 narrows rather than closes: the mechanism is now specified; the accountability is not.** A market's arbiter set is a trust anchor its participants choose and can only leave, which is the same recourse a market stall gives you and less than a court does.

---

## 11. Sustainability

Workers keep 100%; therefore the protocol earns 0% by default. But security-critical money-moving software that goes unmaintained is a wallet-drainer. Funding, in preference order:

1. **Removable maintenance tip** — default 0.5–1%, one tap to zero out, always visible. Precedent: Monero mining dev-fee conventions. Honest and opt-out beats hidden and mandatory.
2. **Grants / donations** — the Veilid and Tor model.
3. **Never**: a protocol-level mandatory cut, a token pre-mine, or venture capital that needs a rent-extraction exit. Any of these reintroduces the middleman the protocol exists to delete.

Multiple independent client implementations are a design goal: a protocol with many clients can't be acquired or shut down like an app, and it keeps the spec honest. That goal is unreachable on behavioral description alone — it requires pinned bytes, a normative state table, strict rejection, and a vector set a second implementation can fail against. **Part V (§18) is what makes this claim payable rather than aspirational**, and a client count of one means the spec has not yet been tested.

---

## 12. Regulatory Posture (not legal advice)

- **Spec author ≠ operator.** Publishing a protocol and reference client is a different posture from running infrastructure or sitting in the flow of funds. DUCAT is built so no one *is* an operator.
- The `xfer` (person-to-person money movement) profile is the component most likely to read as money transmission if anyone facilitates it for a fee. The protocol takes no fee on it and routes no funds through any operator — decisions made deliberately to keep it self-hosted-software rather than a service.
- **`exchange/1` (§7.2) now carries the highest surface of any profile**, and it differs in kind from the others: cash-for-crypto is regulated activity in many jurisdictions *at the level of the participant*, independent of whether an operator exists or a fee is charged. Publishing the profile is a different act from running it as a business, and the client should say so where a user enables it.
- **Receipts cut the other way, and that is a feature.** `pos/1` produces a co-signed record the merchant holds and no one else does (§7.1). A merchant with tax obligations is better served by that than by a processor statement, and the protocol neither reports it nor prevents its disclosure. Record *absence* is a property of the network, not a constraint on what participants voluntarily keep about their own trade.
- These are design constraints chosen to reduce regulatory surface; they are not a legal opinion and a real one should be obtained before any launch.

---

## 13. Build Order

Grounded in what current primitives actually support (veilid-core 0.5.x is stable; Monero multisig is not):

**Phase 0 — Measure before building (cheap, blocking)**
0a. **Done** (O11). Measured on veilid-core 0.5.7: `token` mode 190 B, `inline` mode 877–1669 B and non-monotonic in hop count. §15.3.1 carries the numbers; §15.3.2 specifies measure-and-degrade instead of a budget.
0c. **Done, and it found the worst result available** (O19). Veilid #395 is open against milestone 0.13.0 while the current release is 0.5.7. Remote hail (§5.2) should not ship until that lands. Proximity profiles proceed with the residual documented in §15.10.
0b. **Done** (O14). 135–204 KB/s at 32 KB payloads over a private route, latency-dominated (§8.7.1). Adequate for incremental sync from a restore height of now; inadequate for a full chain, which the design already avoids. Re-measure against a real `relay/1` peer before Phase 3.

**Phase 1 — Buildable now on stable primitives**
1. `chat/1` — Veilid E2EE session. Proves L0–L1. **Ships with the Part V codec, not after it**: canonical CBOR, domain-separated signatures, strict rejection, and the first test vectors. Retrofitting canonicality onto a working implementation means re-signing every transcript format already in the field — cheap now, expensive at Phase 3. Assign the §18.7 identifiers (NFC AID, BLE UUIDs) here too, since the AID cannot change without a simultaneous update to every iOS client.
2. `file/1` (direct) — adds content-hash PROOF. Proves L3 transcript.
3. `xfer/1` — thin Monero wallet + L3 payment-request flow. Proves L4 direct. **Requires answering "whose monerod?" first** — §8.7's submission and scanning paths, and at minimum a hand-configured node, are prerequisites rather than Phase 3 polish. `relay/1` as a discoverable staked service can follow later; node access cannot.

**Phase 2 — The tap, and the first real commerce**
4a. `pos/1` (§7.1) — adds the proximity TAP (§5.1, §15) and the WYSIWYS confirm screen (§15.5) with **none** of `ride/1`'s routing machinery: no OSRM/Valhalla, no geocells, no reverse-geocoding, no arrival co-sign. A merchant sale is the cheapest possible proof that the tap primitive works end to end, and it de-risks item 4b rather than competing with it. Ship it first.
4b. `ride/1` (direct settlement) — adds on-device routing/pricing (§6.1, §15.5) and co-signed arrival PROOF on top of a tap already proven by 4a. This is the tap-to-ride demo; it needs no escrow.
4c. `exchange/1` (§7.2) — the on-ramp. Gates adoption of everything above it, since none of these profiles matter to a user holding no XMR. Cheap to build (direct settlement, physical-handoff PROOF) and easy to underestimate.
5. Persistent contact (§16) — the rendezvous DHT record. Small, and it makes `chat/1` survive the ride.

Cross-profile mechanics (§7.3) land alongside these: `REFUND` with `pos/1`, `CANCEL` with `ride/1`, `MANDATE` and multi-payee splits once persistent contact exists. None require new layers.

**Phase 3 — Trust-dependent**
6. Consumer float + `fast/1` bonded settlement (§8.6, §17), incl. slashing state machine and arbiter sets. *This is what makes the tap-to-ride demo feel instant rather than merely correct.*
7. Provider bonded stakes (§9.1) + full arbitration market (§9.3).
8. Escrow (§8.2/8.3), then `lodging/1`, `task/1`.

**Phase 4 — Research**
9. Offline settlement (§8.4), adaptor-signature escrow (§8.5).

Ship Phase 1–2 as a working federation at a single seed market before touching Phase 3. Note that Phase 3's item 6 can precede item 7: fast settle needs an arbiter set the *market* trusts, not a mature open arbiter economy.

---

## 14. Open Problems (honest list)

- **O1.** **Multisig — reframed twice, now largely a client-architecture question.** A 2-of-3 ceremony on wallet2 v0.18.5.1 converged in 2 rounds and 134 seconds, so round-trip fragility was overstated (`monero-spike/REPORT.md`). The remaining wallet2 problems — disabled by default, no RPC enablement, halfway-stranding — are dissolved by the decision to **embed a wallet rather than drive the RPC**, which further permits FROSTLASS (audited May 2025, O(1) per-signer) in place of wallet2's experimental implementation. What stays open: `monero-oxide` is pre-1.0, an embedded wallet means owning wallet correctness directly, and **cross-scheme interoperability is undocumented**, so a bond's parties must all run the same scheme (§10.1).
- **O2.** Offline settlement (A5) has no trust-minimized answer yet.
- **O3.** **DHT harvesting — largely retired by §5.2's inversion.** Providers no longer advertise; they watch. A harvester learns nothing about supply, only that someone hailed from a coarse cell at a time — ephemeral and unattributed, rather than a standing dossier of who works where. What remains: hail traffic is observable in aggregate, and hail spam is unsolved (rate limits are cheap, stake is strong but pulls consumers toward collateral, per O15).
- **O4.** Reputation vs. unlinkability is a genuine trade with no free lunch (§4, §9.2).
- **O5.** The safety floor (§9.4) structurally caps the addressable market **for high-exposure profiles** — rides, lodging, open-ended tasks. Scoped in 0.7: it does not bind the no-exposure tier (`pos`, `xfer`, `goods`, `file`, `chat`), which is where most of §1's addressable surface now lives. The cap is no smaller where it applies; it simply applies to less of the protocol than first stated.
- **O6.** Open-ended PROOF for `task/1` (what counts as "done"?) resists specification.
- **O7.** Cold start still requires a real, motivated seed community; the protocol enables it but cannot manufacture it.
- **O8.** **Arbiter-set governance is load-bearing** (§17.8), and now *half* specified. §10.1 gives the mechanism — a self-certifying `market_id`, threshold-signed rotation chained from genesis, forks detectable. What remains open is **accountability**: the chain proves continuity, not honesty, and a market captured at genesis or rotating itself into capture produces valid signatures throughout. Fast settlement is worthless in such a market because a slash would never pay out, and a participant's only recourse is to leave.
- **O9.** Hot-wallet exposure (§17.2): a small float on a phone is malware- and seizure-reachable. Mitigated by keeping it small; not eliminated.
- **O10.** Price-oracle integrity and the `capacity_remaining` side channel (§17.7, §17.8) — both leak or can be manipulated in ways that are mitigated, not closed.
- **O11.** **Route-blob size — measured, and it resists being a constant.** Phase 0a: `token` mode is exactly 190 B; `inline` mode ranged 877–1669 B for a `TapPresent` across two runs, non-monotonic in hop count, with within-hop-count spread exceeding between-hop-count differences. Size tracks *which peer was selected*, not how many hops were asked for. §15.3.1 therefore specifies measure-and-degrade rather than a budget. Closed as a question; the variance itself is the finding.
- **O12.** Persona loss and recovery (§16.8): losing a device loses the persona and every rendezvous keyed to it. Signed key-rotation announcements to existing contacts are an unsolved sub-problem of L1.
- **O13.** **Relay anonymity set** (§8.7.3): routing settlement through DUCAT relays makes "is a DUCAT user" observable to anyone watching the relay set, and a seed market's set is a few hundred people. Mitigated by preferring Tor and by growth; not solved. Note this compounds O7 — the smallest markets have both the thinnest liquidity and the thinnest cover.
- **O20.** **Two wallet-layer gaps block Phase 3** (`monero-rs/REPORT.md`). `monero-wallet` 0.2.0 exposes scanning only over blocks — `scan_transaction` exists but is private — so a driver cannot verify an *unconfirmed* payment through the public API, which is exactly what `fast/1` acceptance requires. And it implements no transaction proofs at all, so §17.5's arbitration evidence must be built. Neither blocks Phase 1, which needs no bonds and no zero-conf.
- **O21.** **Burning bug versus the standard.** `monero-wallet` offers burning-bug-immune 'guaranteed' outputs, but its own source says they are *"not officially specified by the Monero project ... No support outside of monero-wallet is promised."* Adopting them would lock DUCAT funds to one implementation, cutting against A1's bearer property and §11's many-clients goal. The recommendation is to stay standard and have `pos/1` merchants **detect** duplicate one-time keys instead — but the underlying hazard, that an attacker can make a merchant see two payments they can spend only one of, is real and unmitigated by §15.10's fresh-subaddress rule, which narrows the window without closing it.
- **O14.** **Veilid at sync volumes — measured, provisionally adequate.** Phase 0b: 135–204 KB/s at 32 KB payloads, latency-dominated so throughput scales with request size (§8.7.1). A day of blocks moves in minutes; a full chain would not, which is why §17.1's restore-height-of-now is load-bearing. Two samples, self-route, unpipelined — enough to proceed, not enough to design against. Re-measure against a real `relay/1` peer before Phase 3.
- **O19.** **Veilid #395 is contained, not closed.** §5.2's inversion means matching runs entirely over DHT reads — which import nothing — and the single remaining import happens after mutual selection, by the party that chose to initiate. Exposure is now one per real transaction to a chosen counterparty, the same posture the tap already carries (§15.10), rather than one per browse to anyone watching. Remote hail therefore no longer waits on the 0.13.0 milestone. The upstream fix is still required before that last import is clean.
- **O15.** **Cancellation fees erode the permissionless lane.** §7.3 makes no-show fees enforceable only against collateral. The pressure this creates — providers preferring bonded counterparties precisely because cancellation *costs* them something — pushes the network toward the collateralized lane and quietly hollows out the slow permissionless one A4 depends on (§17.6). Whether the unbonded lane survives contact with real no-show rates is an empirical question no amount of spec work answers.
- **O16.** **iOS cannot present over NFC, permanently.** Apple's HCE entitlement is conditioned on EEA establishment, organization enrollment, and financial-regulatory standing (§15.3.2) — structurally incompatible with A4, and not a hurdle an open protocol clears. The best-UX medium is therefore available to roughly half the supply side, and QR carries the rest. This is outside DUCAT's control and will not improve through protocol design; it is stated so no one plans around a tap that cannot exist.
- **O17.** **Identifiers are unassigned** (§18.7): the NFC AID is a placeholder pending real RID registration, and the BLE service/characteristic UUIDs are unallocated. Neither is hard, both are blocking for cross-implementation testing, and the AID is effectively immutable once iOS clients ship with it declared at build time.
- **O18.** **Conformance suite exists, single-sourced.** 92 vectors in `vectors/v1/` now cover §18.9(1)–(7) including full per-profile transcripts, executable and language-neutral, with a runner proving the artifact matches the implementation. Still not closed, and for an unchanged reason: a vector set generated and validated by one implementation encodes that implementation's bugs as the specification. It closes when a second, independent client runs these files and disagrees. Remaining gaps are escrow and `fast/1` transcripts, which await `TXPROOF` and the escrow objects.

---
*End of Part I. The remaining parts specify the three mechanisms Part I leans on hardest: the tap that opens every transaction, the identity that optionally survives one, and the settlement that makes it fast enough to matter.*

---

# Part II — The Tap
**L2/L3 detail: the capability exchange**

This part specifies the single primitive everything else rides on: what crosses the gap when two phones bump, what happens over the channel that bump opens, and the one screen that keeps a silent transaction from becoming a skimming attack.

---

## 15.1 Core realization: the tap is a *bootstrap*, and the medium is interchangeable

The entire job of the tap is to move **enough information between two phones that they can finish the conversation over Veilid.** Nothing more. It carries:

1. enough to **identify** the presenter for the next few seconds (a session key),
2. enough to **reach** them (a route, or a pointer to one), and
3. a **commitment** to the offer (so the full offer, delivered a moment later over the channel, can't be swapped).

The full offer — rate card, price breakdown, destination detail — flows over the E2EE channel that bootstrap opens. The tap is the handshake; the channel is the conversation. This keeps the payload small and makes the gesture identical across every profile.

Two consequences follow, and they drive everything in §15.3:

**The medium doesn't matter.** ~200 bytes to ~1 KB has to cross a few feet of air. QR, NFC, BLE, or a rendezvous token read aloud all satisfy that. "Tap" is the *gesture* and the user-facing word; it is not a commitment to NFC. Any medium that moves the payload is conformant, and clients negotiate downward through a ladder (§15.3.2) rather than failing when one medium is unavailable.

**Only one direction needs to cross.** The presenter shows; the reader reaches back over Veilid using what it just learned. The bootstrap is strictly one-way, which is precisely why a QR code — a one-way medium — is sufficient, and why the loss of symmetric NFC peer-to-peer costs the protocol nothing.

> Honoring "the tap should contain destination information" literally: destination **coordinates** are ~16 bytes and *do* ride inside the tap's compact offer. What flows over the channel is the bulky part — the signed rate card and the fare breakdown the payer verifies the coordinates against.

---

## 15.2 Two axes + one intent generate every case

Every scenario you've raised (cab, tip, payback, split, tab, donation) is a point in a small grid. There is no per-service plumbing; there are these three fields:

- **`presenter_role`** — who is holding out their phone. Almost always the **payee** (the cab, the merchant, the busker, the friend being paid back). The presenter is the one who owns the receive-capability.
- **`amount_authority`** — `fixed` (presenter set the number: cab fare, shop total), `open` (reader types the number: tip, donation, splitting a bill however you like), or `rated` (no number yet — the presenter advertises a rate and the total is derived at `stop`: meters, tabs, anything billed per unit of time or use).
- **`meter.intent`** — `oneshot` (normal), `start` / `stop` (bracket a duration: metered cab, scooter, bar tab).

| Scenario | presenter | amount_authority | meter |
|---|---|---|---|
| Cab, fixed fare | driver terminal | `fixed` (price = f(rate_card, dest)) | oneshot |
| Cab, metered | driver terminal | `rated` | start → stop |
| Merchant / POS | merchant | `fixed` | oneshot |
| Tip / busker | busker (or static tag) | `open` | oneshot |
| Pay a friend back | friend (payee) | `open` or agreed | oneshot |
| Split a bill | one payee | `fixed` per-head **or** `open` | N × oneshot |
| Bar tab / scooter | vendor | `rated` | start → stop |
| Donation box | org (static tag) | `open` | oneshot |

Split and group-pay are **not** new protocol — they're N independent taps against one payee, reconciled in the payee's app. Metered is **not** new protocol — it's two taps sharing a `session_ref`. That's the payoff of getting the primitive right.

---

## 15.3 `TapPresent` — what crosses the gap

Compact CBOR, signed by the session key. Emitted by the presenting phone (or held on a static tag, §15.9).

### 15.3.1 The payload, and its two reach modes

`route` is the only field with meaningful size variance, so it determines which media can carry a `TapPresent`. It has two modes, and the choice is a real trade rather than a fallback:

- **`inline`** — the Veilid private-route blob itself crosses. Self-contained: the reader needs no network to learn how to reach the presenter. Costs ~200–800 bytes.
- **`token`** — a 32-byte rendezvous token crosses; the reader fetches the current route from the DHT. Costs ~32 bytes and fits *any* medium, but requires DHT reachability at tap time, which an offline curb does not guarantee.

Everything else is fixed-size and small — 158 bytes total, of which the signature is 64:

| Field | Bytes (approx) | Meaning |
|---|---|---|
| `v` | 1 | protocol version |
| `suite` | 1 | cipher-suite id (crypto agility; matches Veilid convention) |
| `profile` | ~4 | e.g. `ride/1`, `xfer/1` |
| `presenter_role` | 1 | `payee` \| `payer` |
| `amount_authority` | 1 | `fixed` \| `open` \| `rated` |
| `intent` | 1 | `oneshot` \| `start` \| `stop` |
| `nonce` | 16 | fresh per tap; anti-replay |
| `expiry` | ~4 | short TTL, seconds (e.g. 30 s) |
| `session_pk` | 32 | presenter's ephemeral Ed25519 key |
| `rmode` | 1 | `inline` \| `token` \| `ble` — how the reader reaches back |
| `route` | 32–800 | per `rmode`: inline private-route blob, 32-byte rendezvous token, or BLE handle |
| `offer_commit` | 32 | `H(FullOffer)` — binds the offer delivered over the channel |
| `dest` | 16 | destination lat/lng, when the profile has one (rides) — present in-tap by design |
| `session_ref` | 32 | present only when `intent = stop`; ties this tap to its `start` |
| `sig` | 64 | Ed25519 over all the above |

**Measured against veilid-core 0.5.7 (Phase 0a), across two runs.** An earlier draft estimated inline mode at 360–960 bytes. That was optimistic by roughly a factor of two — and, more importantly, the estimate was the wrong *shape*:

| Reach mode | Route blob | `TapPresent` |
|---|---|---|
| `token` | 32 B | **217 B** sealed, deterministic |
| `inline`, 1 hop | 719–728 B | **877–886 B** |
| `inline`, 2 hops | 1097–1292 B | **1255–1450 B** |
| `inline`, 3 hops | 963–1401 B | **1121–1559 B** |
| `inline`, 4 hops | 1049–1511 B | **1207–1669 B** |

**Size is not monotonic in hop count, and the spread within one hop count exceeds the gap between hop counts.** One run produced a 3-hop blob (963 B) *smaller* than its own 2-hop blob (1097 B). The cause is structural: a route blob is nested onion encryption, one layer per hop, and while intermediate hops compress to a bare 32-byte node id under route optimization, **the entry hop embeds full peer info** — dial addresses and signatures belonging to a peer the allocating client neither chooses nor controls. Blob size is therefore dominated by *which peer happened to be selected*, not by how many hops were requested.

Three consequences, and the first invalidates a claim this document previously made:

1. **Whether a given QR error-correction level suffices is luck, not arithmetic.** §15.3.2 once asserted that even level H clears inline mode at every hop count. Across two runs, level H (1273 B) passed at every hop count in one and failed from 2 hops up in the other — same code, same host, minutes apart. No table can answer "will this fit"; only the blob in hand can.
2. **NFC's single-contact budget reliably fits only a 1-hop route.** At ~1 KB per 300 ms of contact, every measured multi-hop route overflowed it in both runs.
3. **There is a privacy/reach trade, and it should be stated rather than discovered.** More hops means better route anonymity and (on average) a bigger blob, and media narrow as blobs grow. A merchant on a printed tag is pushed toward one hop — the weakest anonymity — or toward `token` mode. **`token` mode is the privacy-preserving choice, not merely the compact one**, because its 190 bytes are constant regardless of hop count, decoupling anonymity from the medium entirely.

Clients therefore MUST NOT assume a fixed inline budget. The normative behavior is a runtime check:

> **Measure the blob you actually received, and degrade to `token` mode when it overflows the medium in hand.**

This stays correct as Veilid's peer-info encoding and hop defaults change underneath, which a measured constant would not.

### 15.3.2 The transport ladder

Clients try media in order and settle on the first both devices support. All four carry the identical `TapPresent`; none of them changes the security model, because WYSIWYS (§15.5) operates on parsed typed fields and never on the transport.

| Rank | Medium | Capacity | Carries | Availability |
|---|---|---|---|---|
| 1 | **QR** | 2,953 B (v40, binary, low EC) | both modes, comfortably | Universal. No entitlement, no gatekeeper, works iPhone ↔ iPhone |
| 2 | **NFC (HCE)** | ~1 KB per ~300 ms of contact; APDUs chain | both modes | **Android presenter only** — see below |
| 3 | **BLE** | ~469 B fragments (GATT); L2CAP CoC for streams | both modes | Cross-platform (`CBL2CAPChannel` iOS 11+, Android 10+). No line of sight required |
| 4 | **Rendezvous token, any channel** | ~190 B | `token` only | Fits a small QR, an NTAG213, or a short string read aloud. The accessibility path |

**QR ranks first on availability, not on elegance.** NFC is the better gesture wherever it works, and clients SHOULD prefer it when both devices support it. But QR is the only medium with no platform gatekeeper, and it is the only one that works between two iPhones — so it is the baseline every client MUST implement, and NFC is an optimization layered on top.

**The NFC platform reality, stated plainly.** Symmetric NFC peer-to-peer no longer exists: Android Beam was deprecated in Android 10 and removed in Android 14. Phone-to-phone NFC now means host card emulation on one side and reader mode on the other. Android exposes HCE to any app. **iOS does not** — Apple's HCE entitlement requires the developer to be established in the EEA, enrolled as an organization rather than an individual, compliant with PCI DSS and EMVCo, and, for payments, to hold the relevant regulatory permissions; outside the EEA the path is Secure Element access, which is tighter still. That entitlement is conditioned on being a regulated financial entity, which is structurally incompatible with A4. It is not a hurdle an open protocol grinds through; it is a door that does not open.

| Presenter | Reader | Works? |
|---|---|---|
| Android (HCE) | Android (reader mode) | ✓ |
| Android (HCE) | iOS (Core NFC, foreground, AID pre-declared) | ✓ |
| iOS | anything | ✗ |
| Static tag (§15.9) | either | ✓ read-only |

**This promotes `presenter_role` from convenience to load-bearing.** §15.2 observes that the presenter is almost always the payee — the driver, the merchant, the busker. That is the supply side, and on iOS it cannot present over NFC. Two escapes, both already in the protocol: invert the roles so the payer presents (works when the payer is on Android), or fall to QR (works always). An iPhone merchant serving iPhone customers is a QR deployment, and the protocol should say so rather than let an implementer discover it at a market stall.

**Static tag sizing, measured twice.** A bare `TapStatic` (§15.9) fits the commodity NTAG213's 144 bytes. A `TapPresent` does not: `token` mode is 217 B sealed and needs an NTAG215, and `inline` mode **no longer fits any commodity tag at any hop count** — the encoded 1-hop object is 915 B against an NTAG216's 888. The earlier 2-byte margin was an artifact of counting payload rather than encoding. **Tags ship tokens**, and there is no longer a case to argue.

**Securing the BLE channel.** When the session runs over BLE rather than Veilid — offline, or while a route is still building — the channel needs its own encryption, and the spec previously left this unstated. DUCAT adopts **`Noise_XX_25519_ChaChaPoly_SHA256`**: mutual authentication and forward secrecy, with the presenter's `session_pk` from the `TapPresent` as the expected static key, which binds the BLE session to the bootstrap that started it. This is the same construction bitchat uses for live BLE sessions, and reusing a Noise pattern with existing cross-platform implementations is the point. Note the negative lesson from the same source: bitchat's *offline* courier envelopes use the `X` pattern and knowingly give up forward secrecy — acceptable for undelivered chat, not for a payment authorization sitting in someone's outbox. DUCAT's store-and-forward work (§8.4) MUST NOT inherit that trade.

---

## 15.4 `FullOffer` — what flows over the channel

Delivered by the presenter immediately after the channel opens; the reader checks `H(FullOffer) == offer_commit` before showing anything to the human.

| Field | Meaning |
|---|---|
| `payto` | **fresh** Monero subaddress, one per tap (unlinkable on-chain; never reuse) |
| `amount_pxmr` | the number, when `amount_authority = fixed` — **unsigned integer piconero**, never a decimal (§18.2) |
| `rate_card` | presenter's signed rate card (or hash + fetch ref) — lets the reader *reproduce* the price |
| `route_inputs` | for rides: origin, `dest` (must equal the tap's `dest`), distance, duration |
| `breakdown` | how `amount_pxmr` was derived: base + per-km + per-min, etc. |
| `persona` | optional long-lived key + its stake proof + attestation pointer (trust surface, §15.6) |
| `terms` | profile-specific (cancellation, escrow flag, meter rate) |
| `sig` | presenter session-key signature over the whole `FullOffer` |

---

## 15.5 WYSIWYS — the confirm screen is the security boundary

"They don't even need to speak" is only safe if the payer's screen tells the whole truth. The rule, stated as a hard invariant:

> **What You See Is What You Sign.** The bytes the payer's key signs in `ACCEPT` are *exactly* the typed fields the payer's app rendered on the confirm screen. **No display string ever travels from the counterparty to the payer's screen.**

Concretely, the payer's app MUST:

1. **Compute the amount itself.** In `fixed` mode it recomputes the fare from `rate_card` + `dest` using **on-device** routing (OSRM/Valhalla — no map API call leaks the trip). It shows one number and flags any mismatch: *"Provider is charging 0.021; rate card implies 0.018."* The presenter *proposes*; the payer's app *derives*. Those decimals are **display**, produced by the payer's own client from integer piconero after verification — never a wire representation (§18.2).
2. **Reverse-geocode the destination locally.** The human-readable place name is computed from `dest` coordinates on-device. Any label in `FullOffer` is shown only as advisory (*"provider labels this: …"*), never as the authoritative destination. Coordinates are truth; a hostile terminal cannot show "Airport" while routing to "warehouse."
3. **Render trust from structure.** Show the payee as either a bonded persona (*"bonded 0.5 XMR · 47 receipts"*) or *"one-time payee, no history."* That line is the entire trust decision, surfaced at the moment of consent.
4. **Sign only what it displayed.** `ACCEPT` covers `{ tap.nonce, H(FullOffer), amount_final, dest, ts, reader_session_pk }`. If the presenter later claims a different price or destination, the payer's signed `ACCEPT` is the authoritative record — and the payer never signed anything they didn't see.

Silence between the two humans is fine. A silent *payer's app* is not: the confirm tap is the one mandatory human checkpoint, and it renders solely from data the payer's own app parsed and verified.

---

## 15.6 The response leg, and "the other person gets a notification"

```
tap ──▶ channel opens ──▶ FullOffer ──▶ [payer confirm screen]
                                              │
                              ACCEPT (signed, WYSIWYS) ──▶ presenter
                                              │
                              FUND  (XMR to payto) ──────▶ (chain)
                                              │
                    RECEIPT (co-signed by both) ◀────────▶
```

The **notification** you described is the arrival of `RECEIPT` on the presenter's side: *"Received 0.02 XMR — ride to [dest]."* Both sides end holding the same co-signed receipt, which is the only record of the transaction and lives nowhere else. For `fixed` (cab), the payer confirms and the payee is notified. For `open` (tip), the payer types the amount and taps, and the payee is notified of receipt. Same leg, both directions.

---

## 15.7 Metered flow (two taps, one `session_ref`)

- **Tap 1 — start:** `intent = start`, `amount_authority = rated`. Payer confirms the **rate** (*"Start meter at 0.001 XMR/min?"*). A `session_ref = H(start exchange)` is minted and held by both.
- **Tap 2 — stop:** `intent = stop`, carries `session_ref` + a `FullOffer` whose `amount_pxmr` is computed from elapsed time/distance. Payer confirms the **final total**; settlement fires.

The `session_ref` binding means you can only be billed for a meter you actually started — a `stop` with an unknown `session_ref` is rejected before any confirm screen appears.

---

## 15.8 Split / group flow (N taps, app-side reconciliation)

One payee presents (`fixed` per-head, or `open` so each person pays what they choose). Each payer taps in turn; each is an independent `oneshot` to the same (or a fresh-per-payer) `payto`. The payee's app tracks receipts against the target total and shows *"3 of 4 paid."* No new message types.

---

## 15.9 Static tags (donation boxes, tip jars)

A passive NFC tag or printed QR can hold a **`TapStatic`** — a receive-only capability, and a *different object type* from `TapPresent`: `payto` + optional pinned `persona`, no `session_pk`, no per-tap signature. Readers MUST distinguish the two and never treat a `TapStatic` as a live counterparty. Consequences, stated honestly:

- Works for **`open`, receive-only** cases (donations, tips) where there's no phone on the payee side.
- **No freshness:** a static tag is inherently replayable — but for a donation, "replay" just means choosing to pay again, which is benign.
- **No co-signed receipt, no meter, no negotiation.** For anything with a price to verify or a service to deliver, the payee needs a live phone emitting a fresh `TapPresent`.
- **Chip capacity is the deciding constraint.** A `TapStatic` fits the commodity NTAG213 (144 B user memory). Anything richer does not — `TapPresent` needs an NTAG215 or NTAG216 (§15.3.2). Printed QR sidesteps this entirely at 2,953 bytes and is the better static medium wherever a printed sticker is acceptable.

Use static tags only where the worst case of a swapped tag is "money went to the wrong subaddress" — i.e. pin the persona and let the payer's app warn on an unrecognized one.

---

## 15.10 Residual attacks (not hand-waved)

- **Relay, and why QR is worse than NFC here.** Contactless relay attacks are real — an attacker bridges two exchanges over a network to make distant devices appear adjacent. NFC at least makes proximity a weak authentication factor; **QR does not.** A QR code can be photographed and forwarded across the world instantly, so ranking QR first (§15.3.2) buys availability at a real cost in relay resistance. Mitigations: short `expiry` (seconds, not minutes), the WYSIWYS confirm screen, a round-trip challenge over the opened channel as a coarse liveness check, and — specific to QR — presenters SHOULD render a **fresh, screen-displayed** code per transaction rather than a printed one, so a captured image is stale on arrival. Distance-bounding is not solved on commodity phones. Residual, and slightly more residual than it was.
- **Hostile route blobs deanonymize the reader — measured, open, and not mitigable at this layer.** The reader imports a Veilid private route supplied by an untrusted counterparty, which is exactly the shape of Veilid issue #395: an adversary publishes an unreachable "evil" route, the importer's routing table liveness-pings its first hop directly, and the adversary correlates the pinging address to the importer. **Phase 0c confirmed the issue is open**, last active 2026-06, milestoned to *Release 0.13.0 — Private Routing 2.0* against a current release of 0.5.7. That is a redesign eight minor versions out, not a patch; every client shipping today inherits it.

  **`token` mode does not help, and an earlier draft of this section wrongly said it did.** The exposure comes from importing and *using* a hostile route, not from how the blob reached you — fetching the same blob from a DHT record the adversary controls is byte-identical exposure. The delivery channel changes; the trust relationship does not.

  What it actually costs differs sharply by profile, and only one case is severe:
  - **Proximity profiles** — the counterparty is already physically co-present and §2.3 concedes anonymity does not reach the curb. The real harm is narrower and subtler: **an address becomes a cross-transaction linking identifier that survives session-key rotation**, so a merchant can recognize a returning customer whose ephemeral keys are all fresh. That defeats a property §4 explicitly claims.
  - **Remote hail (§5.2)** — no co-presence, so the exposure is unmitigated and strictly larger. Anyone publishing an advert learns the address of everyone who hails them. **Remote hail SHOULD be gated behind the upstream fix**, or carry an explicit warning; it is the one flow where this is disqualifying rather than merely corrosive.
- **Traffic analysis by message length.** `TapPresent` and `FullOffer` vary in size with profile and content, so a passive observer can distinguish a tip from a fare without decrypting anything. Encrypted DUCAT messages MUST be padded to fixed buckets (256/512/1024/2048 bytes, PKCS#7-style) before transmission — the same defense bitchat applies to its Noise packets, and cheap at these sizes.
- **Payload budget:** largely resolved, and narrower than it looked. HCE is round-trip-bounded rather than byte-bounded (~1 KB per ~300 ms of contact, APDUs chaining), and QR carries 2,953 bytes — so a live presenter has room in either mode. The genuine constraint is *static tags*: NTAG213's 144 bytes hold a `TapStatic` and nothing more (§15.3.2). Real Veilid route-blob sizes remain unmeasured (O11) and still gate `inline` mode on the tightest media.
- **Subaddress reuse:** reusing `payto` across taps clusters a payee on-chain and undoes Monero's unlinkability. Fresh subaddress per tap is **mandatory**, not optional.
- **Rate-card swap:** prevented by `offer_commit` binding the tap to `H(FullOffer)` and the payer recomputing price locally — but only if the payer's app actually holds/fetches the correct `rate_card`. Rate-card distribution and freshness is an open sub-problem for remote (non-tap) hails.

---
*End of Part II. The whole security story reduces to one line: the tap bootstraps a channel, the channel carries a committed offer, and the payer signs only the typed fields their own app verified and displayed.*

---

# Part III — Identity & Persistent Contact
**L1 detail: turning a tap into a relationship**

The tap opens a channel. This part says what identity, if any, survives it — so that chat works during a ride, *and* two people who bumped phones can message each other tomorrow, without making persistent linkage the silent default.

---

## 16.1 Two tiers of channel — the whole design in one distinction

There are two very different things people mean by "we can chat now," and conflating them is where privacy dies. DUCAT keeps them separate:

| | **Session-scoped** | **Persistent contact** |
|---|---|---|
| Identity used | ephemeral `session_pk` | long-lived `persona` key |
| Lifetime | dies at `RECEIPT` | until revoked |
| Reachability | the route the tap opened | a DHT **rendezvous record** (§16.4) |
| Privacy cost | none beyond the transaction itself | a linkable pairing of two personas |
| Consent | implicit in transacting | **explicit, mutual, opt-in** |
| Use | "I'm at door 3." "Two minutes." | "Same driver next Tuesday." A friend you keep. |
| Default | **on** | **off** |

**Tier 1 costs nothing and is already built.** The tap opened a Veilid private route to an ephemeral key — that *is* an E2EE channel. `chat/1` for the duration of a ride needs no new identity: rider and driver message over the session, and it evaporates at `RECEIPT`. This covers ~all in-transaction messaging ("which door," "running late") and leaks nothing past the trip.

**Tier 2 is the new capability, and it is a deliberate linkability decision.** It's the digital equivalent of swapping numbers after sharing a cab: useful, sometimes wanted, never automatic. Everything below is about doing Tier 2 *safely* and *by choice*.

---

## 16.2 What "identity" means here

A persistent contact is exactly two things:

1. **A persona key** — the long-lived Ed25519 identity from L1 (§4). This is the same key that accrues reputation, so handing it over is also what lets a counterparty attest to you (§16.7).
2. **A rendezvous record** — a Veilid DHT record that lets the other party find your *current* private route later, even though private routes rotate for safety. You can't save a route blob and reuse it tomorrow; you save a stable **place to look up** today's route (§16.4).

A contact card is therefore: `{ persona_pubkey, rendezvous_key }`, signed by the persona.

**Persona selection is a privacy control, not a detail.** You almost certainly hold several personas: a *driver* persona you hand every fare who asks, a *personal* persona you give almost no one. Handing your personal persona to every transaction slowly builds a linkable social graph around it — so the app defaults to a **role-appropriate persona** for the active profile and makes "which identity am I sharing" a visible choice, never a silent one.

---

## 16.3 The `CONTACT` sub-flow — after the money, never before

Identity exchange happens **after `RECEIPT`**, as an optional coda. The transaction completes fully anonymously; only then may either side offer a lasting contact. This ordering matters: the deal never depends on identifying yourself, and a coerced or hostile counterparty can't gate payment on it.

```
… RECEIPT (transaction complete, session keys still live) …
        │
   CONTACT_OFFER   A → B   { persona_pubkey, rendezvous_key,
                             bind = sign_persona( H(RECEIPT) ‖ session_pk ) }
        │
   CONTACT_ACCEPT  B → A   { persona_pubkey, rendezvous_key,
                             bind = sign_persona( H(RECEIPT) ‖ session_pk ) }
        │
   both write initial route into the shared rendezvous (§16.4)
```

Three properties, each load-bearing:

- **Mutual.** A contact exists only if *both* offer and accept. You cannot be silently added to someone's contacts by tapping their terminal; nothing about you persists unless you affirmatively hand over a persona.
- **Session-bound.** The `bind` field is the persona signing over `H(RECEIPT) ‖ session_pk`. This proves *the persistent identity I'm handing you is the same entity you just transacted with* — a hostile terminal can't relay you a stranger's persona, because it can't produce that signature against the session you actually spoke to.
- **Declinable in silence.** No `CONTACT_OFFER`, or no `CONTACT_ACCEPT`, and the session keys expire on schedule leaving zero persistent trace. Declining requires doing nothing.

---

## 16.4 The rendezvous — how Veilid makes "reach me later" work

Veilid private routes rotate for safety, so a saved route goes stale. The durable object is a **shared DHT record** used as a mailbox. Veilid DHT records carry a schema of individually-addressable subkeys with per-subkey sequence numbers and multiple writers — which is exactly a two-party (or N-party) rendezvous:

- On contact creation, one side creates a DHT record (an owned keypair, per Veilid's `create_dht_record`) and shares its key; this is `rendezvous_key`.
- The record schema gives **each party its own writable subkey**. Each writes their *current* private-route blob (HPKE-sealed to the other's key so only the intended contact can import it — Veilid provides `hpke_seal`/`hpke_open`, DHKEM-X25519 today, ML-KEM available under the newer suite).
- When your route rotates, you rewrite your subkey. Your contact **watches** the record (`watch_dht_values`) and picks up the new route without any live coordination — asynchronous, offline-tolerant, no server.
- To message a contact: read their subkey → import their current route → open a channel → `chat/1` (or any profile) rides it.

This is not hypothetical plumbing; it's the same pattern Veilid-based apps already use, where each peer publishes its route ID into a shared DHT record that identifies the relationship. DUCAT just scopes that record to a pairwise contact and seals its contents.

```
        rendezvous DHT record  (key = rendezvous_key)
        ┌───────────────────────────────────────────┐
subkey0 │ A's current route, sealed to B  seq=17     │  ← A rewrites on rotation
subkey1 │ B's current route, sealed to A  seq=09     │  ← B rewrites on rotation
        └───────────────────────────────────────────┘
   A watches subkey1 · B watches subkey0 · neither is ever "online" for the other
```

---

## 16.5 What rides on a persistent contact

Once the rendezvous exists, re-contact is one lookup away and any profile can run over it:

- **`chat/1` that persists** — the messaging thread survives the ride; this is the "add them and keep talking" case.
- **One-tap repeat transactions** — the standing-agreement pattern: the second ride with your regular driver skips negotiation because the persona pairing already exists. You reach them via the rendezvous instead of needing to be physically adjacent for an NFC tap.
- **Remote hail of a known provider** — message a saved courier/driver directly rather than broadcasting to a geocell.
- **Attestation delivery** — hand over the co-signed receipt as reputation (§16.7).

---

## 16.6 Privacy accounting (the honest part)

Adding identity is adding linkage. Precisely what a persistent contact does and doesn't cost:

- **It creates a durable, mutual link between two personas.** Those two keys now appear in each other's contact lists. That is a real correlation an adversary who compromises one endpoint can read.
- **It does not deanonymize you globally.** The link is pairwise and persona-scoped. A driver persona with 300 rider contacts reveals a driver with 300 fares — not who they are — provided that persona is never cross-signed with a personal one (L1 keeps personas unlinkable by construction; a persistent contact does **not** break that unless *you* reuse a persona across roles).
- **The graph grows on the persona you chose.** This is why §16.2 defaults to role-appropriate personas: the linkage accretes where you decided it could, not everywhere.
- **Revocation is unilateral and clean.** Either party stops writing their subkey and tears down the rendezvous record; the contact goes dark. There's no central directory to purge, no "delete my account" request to trust someone to honor — you simply stop publishing. Blocking is the same act plus dropping inbound routes from that persona.
- **Forward-safety.** Because route blobs in the rendezvous are sealed and rotate, a rendezvous record captured today doesn't yield a working route tomorrow; an attacker who reads the record still can't reach you without the sealing key.

The rule the UX must enforce: **a transaction leaves no persistent identity behind unless the user, after it completed, chose to leave one.**

---

## 16.7 Reputation tie-in

The persona you exchange for chat is the same key that accrues receipts (§9.2). So a persistent contact is also the channel over which attestations flow: after a good ride you can hand your driver a signed, rated `RECEIPT` bound to their persona, and it's weighted by your own stake so it isn't free to forge. Contact and reputation are the same mechanism seen from two angles — which is why persona selection governs both how reachable *and* how reputationally-legible you are.

---

## 16.8 Edge cases

- **Static tags / one-time payees (§15.9).** No session key, no persona ⇒ no persistent contact. A donation tag can't become a contact, which is correct: you tipped a jar, not a person.
- **One-way contact.** Not supported by design. Contact is mutual or it doesn't exist; asymmetric "I can reach you but you can't reach me" is a stalking primitive and is excluded.
- **Group rendezvous.** The same DHT-record-with-N-subkeys shape extends to a small group (a recurring carpool, a set of couriers). Deferred, but the substrate is identical — each member gets a subkey, each watches the others.
- **Lost device.** A persona lives in the protected store; losing the device loses the persona and every rendezvous keyed to it. Persona backup/rotation and telling contacts "this is my new key, signed by the old one" is an open sub-problem, tracked with L1.

---
*End of Part III. One line: the transaction stays anonymous to the end, and identity is an optional coda the user adds afterward — a mutual, persona-scoped, revocable rendezvous on the DHT that turns a bump into a relationship only when both people choose it.*

---

# Part IV — Bonded Fast Settlement
**L4/L5 detail: making the tap actually settle in seconds**

Everything prior assumed settlement was instant. It isn't. Monero targets ~2-minute blocks and the customary finality convention is ~10 confirmations, so a naive tap-to-ride leaves the rider at the curb for twenty minutes or the driver eating unconfirmed-transaction risk. This part closes that gap with a pre-loaded, bonded consumer float.

---

## 17.1 The core insight: bond once, ride hundreds of times

The objection to Monero multisig (8.2) was that it's multi-round and brittle — hostile to a 3-second curbside exchange. A **consumer bond is not per-transaction**. The user loads a float once; the awkward multisig setup happens once, in a calm, retryable onboarding flow where failure means "tap retry," not "I'm standing in the rain." That single bonded float then backs hundreds of rides.

Consequences, all favorable:

- **Fragility is amortized and off the critical path.** Setup happens at leisure with unlimited retries and a clean abort.
- **Mobile wallet sync becomes tractable.** A wallet created at load time has a **restore height of now** — it scans forward only, never years of chain history. This is the difference between a viable self-custodial phone wallet and one that needs a view-key light-wallet server (i.e. exactly the operator DUCAT exists to delete, and exactly the thing worth subpoenaing). The float isn't only a bond; it's what makes self-custodial mobile Monero practical.
- **Consumer-side sybil resistance appears for free.** Main spec §9.1 staked only providers. Now both sides have skin in the game.

---

## 17.2 The float

```
FLOAT = {
  hot_wallet     : fresh Monero wallet, restore_height = creation block
  outputs        : N pre-split spendable outputs (see below) — NOT one lump
  bond_ms        : 2-of-3 multisig { user, market_arbiter_set, (recovery) }
  bond_amount    : user-chosen, e.g. ~$100 equivalent
  spend_ledger   : local record of in-flight (unconfirmed) obligations
}
```

- **Recommended, not mandated, cap.** The spec RECOMMENDS a small float (order $100) and the UX should discourage loading savings. This is a hot wallet on a phone: malware-reachable and seizure-reachable in ways cold funds are not. Keep the mass of funds elsewhere.
- **The two halves carry different risks, and UX must not blur them.** "Only load what you intend to spend" is correct guidance for `hot_wallet` and incomplete for `bond_ms`. The bond is *locked collateral held in threshold multisig* — it is not spendable pocket money, it backs the user's fast-settle capacity, and a defect in the multisig implementation strands it rather than merely losing a fare (§8.2). Both are small by design, so the worst case is bounded at roughly a float's worth; but a client that tells the user "this is just spending money" has described one half and mislabelled the other. Say plainly: *this much is spendable, this much is posted as a deposit and locked until you withdraw it.*
- **Outputs, not balance — the constraint that breaks a naive implementation.** A freshly-received Monero output, *including change*, is not spendable for **10 blocks** (≈20 min). This is `CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE = 10` in `cryptonote_config.h` — a protocol config constant, not a wallet display preference, which settles an ambiguity earlier drafts hedged on. (Eliminating it is live research: monero-project/research-lab #95.) Note also `CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW = 60`: coinbase outputs lock for 60 blocks, which matters to anyone funding a test float by mining rather than transfer. A float held as a single output funds exactly one payment per lock interval: the second tap fails with a full balance showing on screen. This is not a corner case, it is the second ride.

  **Observed on stagenet.** A freshly received 0.01 XMR reported `balance: 10000000000, unlocked_balance: 0, blocks_to_unlock: 9` — the entire float visible and none of it spendable. Note the lock bites on the *first* payment after any receipt, not only the second; a client that funds a float and immediately offers to transact will fail at the curb with a full balance on screen. (The same observation confirms §18.2's atomic unit: 0.01 XMR arrived as exactly 10,000,000,000 piconero.) At load time the client MUST **pre-split the float into N spendable outputs** (default N ≈ 20, each sized around the user's typical transaction), and MUST re-split opportunistically as unlocked outputs are consumed, warning the user before the count reaches zero rather than at the curb.
- **Bond ≠ balance.** The bond is locked collateral; the hot wallet holds spendable XMR. A rider's *fast-settle capacity* is therefore computed over **unlocked outputs**, never over balance:

  ```
  capacity = min( unlocked_output_value − fee_reserve,
                  bond_amount − in_flight_obligations )
             gated by unlocked_output_count ≥ 1
  ```

  A client that reports capacity from `hot_balance` will promise fares it cannot pay. **Two traps, both observed on stagenet:**
  - **The fee must be reserved.** A payment was refused with exactly its own amount sitting unlocked — the fee had nowhere to come from. Capacity computed as the full unlocked value overstates by at least one fee, and the failure surfaces at the moment of payment.
  - **"Available" does not mean unlocked.** `incoming_transfers` with `transfer_type: "available"` reported 13 outputs while `unlocked_balance` covered a fraction of them. A client counting outputs from that call will believe it has spendable funds it does not have. Count against `unlocked_balance`, not against an availability flag.
- **Withdrawal** requires a cooldown (default 24 h) with no in-flight obligations, so a rider cannot ride and instantly drain the collateral backing that ride.

---

## 17.3 Three layers of zero-conf safety

The driver accepts instantly not because double-spend is impossible, but because it is *bounded, detectable, and collateralized*.

**Layer 1 — Broadcast, and the recipient scans.**
Rider broadcasts and hands the driver the txid over the already-open Veilid session. **The driver then determines for themselves whether it pays them**, by scanning the mempool transaction with their own view key.

Earlier drafts had the rider supply an `OutProofV2`-style tx proof here. That was redundant: a tx proof exists to convince someone who is *not* the recipient, and the driver **is** the recipient — their own keys answer the question directly, unforgeably, and without trusting anything the payer says. A proof handed over by the payer is a claim the driver would have to validate independently anyway. Proofs remain genuinely necessary for **arbitration** (§17.5), where an arbiter is not the recipient and must verify payment without being handed the driver's view key, which would expose their entire income. Defeats "I sent it, honest" and wrong-address fraud outright. A genuine double-spend now requires racing a conflicting key image to miners, which is hard *and* self-revealing: the conflicting key image is visible on-chain and permanently attributable to the bond.

**Layer 2 — Bounded exposure.**
`in_flight_obligations` is tracked locally and asserted in the offer; capacity shrinks as unconfirmed rides stack up. One float cannot underwrite unlimited simultaneous fraud.

**Layer 3 — Slashable bond.**
If the tx never confirms, or a conflicting key image lands, the driver files a slash claim with the arbiter set. Payout comes from the bond.

Note what makes this stronger than Bitcoin zero-conf: Monero's mempool opacity makes *probabilistic* zero-conf risk harder to reason about, so DUCAT doesn't rely on probability at all — it relies on collateral. The bond is the answer to "how do I know this confirms," not mempool observation.

---

## 17.4 Settlement mode `fast/1` — message flow

Extends the tap flow (Part II, §15) with two fields and one new message.

Additions to `FullOffer` / `ACCEPT`:
- `settle_mode : fast | direct | escrow`
- `bond_proof` — rider's bond attestation: `{ bond_ms_address, bond_amount, arbiter_set_id, capacity_remaining, sig_by_bond_key }`, freshness-bounded and signed.

```
  … tap → FullOffer → [WYSIWYS confirm] …
        │
  ACCEPT      rider → driver    + bond_proof (capacity ≥ fare)
        │
        │  driver's app verifies: arbiter set is one it trusts,
        │  bond ≥ fare, capacity ≥ fare, attestation is fresh
        │
  FUND        rider broadcasts tx to the Monero network
        │
  TXID        rider → driver    { txid }
        │
        │  driver scans the mempool tx with its own view key,
        │  confirming amount and destination  →  ACCEPTS RIDE (seconds)
        │
  RECEIPT     co-signed, marked provisional (unconfirmed)
        │
        ⋮  ~10 confirmations later, both apps observe finality
        │
  SETTLED     local state transition; obligation clears; capacity restored
```

Total added latency over the pure-anonymous flow: one broadcast plus one proof verification. Target remains **< 3 s to ride accepted**.

---

## 17.5 Slashing state machine

```
   HEALTHY ──ride accepted──▶ IN_FLIGHT ──confirmed──▶ HEALTHY
                                  │                    (capacity restored)
                                  │
                    ┌─────────────┴──────────────┐
             timeout (N blocks)          conflicting key image
                    │                            │
                    └──────────▶ CLAIMED ◀───────┘
                                  │
                    ┌─────────────┴─────────────┐
              rider cures                  arbiter rules
              (re-broadcasts,                    │
               confirms)              ┌──────────┴──────────┐
                    │              SLASHED              DISMISSED
                    ▼           (driver paid from      (frivolous claim;
                 HEALTHY         bond; rider bond       claimant's own
                                 marked degraded)       stake penalized)
```

Rules that keep this honest:

- **Cure window.** Non-confirmation is usually a fee or propagation problem, not fraud. The rider gets a window (default 20 blocks ≈ 40 min) to re-broadcast or bump. Only then does a claim mature.
- **Conflicting key image skips the cure window.** It is unambiguous evidence of a double-spend attempt, on-chain and self-authenticating. Straight to `CLAIMED`.
- **Evidence is compact and verifiable.** The claim carries `{ signed ACCEPT, tx proof, RECEIPT, txid }` — and this is the one place a proof is irreplaceable, since the arbiter is not the recipient and cannot scan for itself. **`monero-wallet` provides no proof implementation, so this is DUCAT's own work** (`monero-rs/REPORT.md`) — the arbiter only needs to check signatures and query the chain. No he-said/she-said; this is the *easiest* possible dispute class, which is why fast-settle disputes are cheap to arbitrate.
- **Frivolous claims cost.** The claimant's own provider stake (9.1) is at risk on dismissal, so drivers can't grief riders with bogus claims.
- **Degraded bonds.** A slashed rider's bond is marked; drivers may set policy to refuse degraded bonds or demand confirmations. Reputation without identity, again — the bond *is* the reputation.

---

## 17.6 Honest cost: this partially retreats from axiom A4

Main spec §1.1 A4 says permissionless — no registration to spend, the way cash needs no enrollment. A pre-loaded, pre-bonded float is closer to a **transit card** than to cash. Naming this plainly rather than letting it slide in:

- **What is preserved:** no identity, no KYC, no counterparty approval, no operator custody. The bond is self-custodial collateral, not an account with anyone.
- **What is conceded:** spontaneity. You must have prepared to transact quickly.
- **The mitigation that keeps A4 mostly intact:** *bonding buys speed, not permission.* Unbonded riders transact fine in `direct` mode — the driver simply chooses whether to wait for confirmations or decline. Every provider sets their own policy (`accept_unbonded: never | under X | always`), so the network has a slow permissionless lane and a fast collateralized lane. Cash-parity survives in the slow lane.

---

## 17.7 Denomination and the price oracle

XMR is the settlement asset; that is not negotiable, since it is the only mechanism delivering the privacy properties the whole protocol rests on. Volatility is therefore a *presentation* problem, not a settlement one.

**Rate cards SHOULD be denominated in a reference currency** (e.g. "$2.50/km") and converted to XMR at quote time. Otherwise providers are repricing their labor by hand every hour.

**The oracle problem, and why it's smaller than it looks.** A price feed is a network call, and a network call at tap time is both a leak and a latency hit. Design constraints:

- **Never fetch at tap time.** The client maintains a **cached rate**, refreshed on a background schedule (every few minutes) decoupled from any transaction. A fetch therefore correlates with nothing — it happens whether or not you ride.
- **Route the fetch privately.** Over Veilid or Tor, never a direct call from the transacting device to a named exchange API.
- **Multi-source median.** Several independent sources, take the median, reject outliers beyond a threshold. A single source is a manipulation vector and a single point of failure.
- **Both sides bind the rate.** `FullOffer` carries `{ ref_currency, ref_amount, xmr_rate, rate_ts, rate_sources_hash }`. The payer's app compares against *its own* cached rate and refuses (or warns loudly) if they diverge beyond tolerance (default 2%) or if `rate_ts` is stale (default > 10 min). This preserves WYSIWYS: the payer never trusts the counterparty's exchange rate, it verifies against one it fetched independently.
- **Staleness fails safe.** No fresh rate ⇒ fall back to XMR-denominated quoting with the rate shown as unverified, or decline. Never silently transact on an old number.
- **Market-published feeds (optional).** A market (10) may publish a signed median rate into its DHT keyspace, letting thin clients read a rate from the network they're already talking to rather than reaching out to the clearnet at all. The market's arbiter set stakes its reputation on the feed's honesty.

**Bond coverage floats too.** A bond denominated in XMR covers less fiat value after a drop. The client SHOULD re-express bond capacity in the reference currency continuously and warn when coverage falls below the user's typical transaction size — the bond doesn't need topping up for correctness, only for capacity.

---

## 17.8 Residual risks

- **Hot wallet exposure.** $100 on a phone is reachable by malware and by physical seizure. Mitigated by keeping the float small; not eliminated.
- **Arbiter set trust.** Fast settlement requires the driver to trust the rider's arbiter set enough to believe a slash would actually pay out. This makes arbiter sets a market-level trust anchor, and a market with a captured arbiter set is a market where fast settle is worthless. Arbiter-set governance is now load-bearing (open problem).
- **Bond capacity as a side channel.** `capacity_remaining` in `bond_proof` leaks a coarse signal about a rider's recent activity to every provider they tap. Consider bucketing (e.g. "capacity ≥ fare" as a boolean, or coarse tiers) rather than exact values.
- **Cure window abuse.** A rider could habitually underpay fees, force cure windows, and slow-walk drivers without ever being slashed. Track cure-window invocations against the bond and degrade it after repeated use.
- **Multisig setup remains the fragile step.** Amortized, off the critical path, and retryable — but still the least-proven machinery in the stack (main spec O1 stands).

---
*End of Part IV. One line: the rider posts collateral once, so the driver can accept an unconfirmed transaction in seconds against a bounded, provable, slashable downside — and prices are quoted in real money via a cached, privately-fetched, independently-verified rate.*

---

# Part V — Wire Format & Conformance
**Making "many clients" achievable rather than aspirational**

§11 makes multiple independent client implementations a design goal, on the argument that a protocol with many clients cannot be acquired or shut down like an app. Everything before this part describes *behavior*. None of it pins the **bytes**, and two competent implementers working from Parts I–IV would produce incompatible wire formats within a week — different CBOR encodings, different signing inputs, different ideas about what a malformed message means. This part closes that gap.

It is deliberately opinionated where interop demands one answer, and honest about the parts that must be produced alongside a first implementation rather than invented here.

---

## 18.1 Canonical Encoding

All signed objects are **CBOR** (RFC 8949) restricted to the **core deterministic encoding requirements** of §4.2.1, with the following additional constraints. Determinism is not a nicety here: two clients that encode the same object differently produce different hashes, and every commitment in the protocol — `offer_commit`, the message chain in §6, `H(RECEIPT)` in §16.3 — breaks.

- **Definite-length only.** No indefinite-length arrays, maps, strings, or byte strings.
- **Smallest form for integers**, and preferred serialization for all major types.
- **Map keys are unsigned integers, not strings.** COSE convention. This is a size decision as much as a canonicality one: at a ~190-byte token-mode budget (§15.3.1), string keys are unaffordable.
- **Map keys sorted bytewise-lexicographically on their encoded form**, ascending. Duplicate keys are a fatal error, not a last-wins.
- **No floats. Ever.** Not in monetary fields, not in coordinates, not in rates. See §18.2.
- **No CBOR tags** except from a per-version allowlist, currently empty. An unrecognized tag is a fatal error.
- **Text strings are UTF-8, NFC-normalized**, and appear only in advisory display fields (§15.5) — never in a field a decision depends on.

## 18.2 Money Is Integers

Every monetary quantity in the protocol is an **unsigned integer count of piconero** (10⁻¹² XMR, Monero's atomic unit). The field is `amount_pxmr`. There is no decimal representation anywhere on the wire.

This corrects a real hazard in the prose of Parts II–IV, where `amount_xmr` and examples like `0.021` invite a floating-point implementation. Binary floats cannot represent most decimal fractions exactly; two clients rounding differently disagree about the fare, the disagreement lands inside a signed `ACCEPT`, and the resulting dispute is unresolvable because both parties signed what their own client computed. **Decimal display is a presentation-layer conversion performed after verification, never a wire format.**

The same rule governs the reference currency (§17.7): `ref_amount` is an integer in the currency's minor unit, with `ref_exponent` naming the decimal places, and `xmr_rate` is an integer scaled by a fixed factor declared in the suite.

## 18.3 Signing, Domain Separation, and Re-encoding

Two failure modes, both classic, both currently unaddressed.

**Re-encoding attacks.** A verifier that decodes an object, re-encodes it, and checks the signature over its *own* encoding will accept an object that a canonicality-violating sender encoded differently. The rule: **signatures are verified over the exact bytes received.** A client MUST retain the received byte string, verify against it, and independently confirm it satisfies §18.1 — rejecting non-canonical encodings even when the signature checks out.

**Cross-context signature replay.** The same persona and session keys sign `TapPresent`, `ACCEPT`, `RECEIPT`, `CONTACT_OFFER`, `bond_proof`, and attestations. Without domain separation, a signature harvested from one context can be presented as another wherever the signed byte strings can be made to coincide. Every signature input is therefore:

```
sig_input = "DUCAT-v1" ‖ 0x00 ‖ object_type ‖ 0x00 ‖ suite_id ‖ 0x00 ‖ canonical_bytes
```

with `object_type` a fixed short ASCII label per message type (`"TapPresent"`, `"ACCEPT"`, …). The `0x00` separators prevent concatenation ambiguity between adjacent variable-length fields. `suite_id` is bound in so a signature is not portable across cipher suites — which is half of §18.6's downgrade defense.

**Uniqueness of encoding is the real requirement, and it reaches past CBOR.** §18.1 makes objects have exactly one byte representation. That guarantee is worthless if the *values inside* them do not, and two places under the P-256 suite would otherwise break it. Both were found by implementation, and both have the same shape: something that verifies correctly under two distinct encodings, so two parties end up holding transcripts that hash differently while every signature still checks out.

1. **ECDSA signatures are malleable.** For any valid `(r, s)`, the pair `(r, n − s)` is equally valid over the same message under the same key. Ed25519 has no such property. Since §6 chains messages by hash and a completed transaction is a self-verifying transcript, an in-flight `s` flip leaves both signatures valid while silently diverging every downstream commitment — and would hand a `fast/1` slash claim (§17.5) evidence that verifies but does not match the counterparty's copy. **P-256 signatures MUST be emitted in low-`s` form, and the high-`s` twin MUST be rejected on verification rather than normalized.** Normalizing on receipt would mean two distinct byte strings are each "the" signature, and the transcript hash would depend on which arrived.
2. **SEC1 public keys have several encodings, and parsers are lenient about the tag.** Compressed (`0x02`/`0x03`, 33 bytes), uncompressed (`0x04`, 65), and hybrid (`0x06`/`0x07`) all encode the same point — and a widely-used parser additionally accepts `0x05`, reading y-parity from the tag's low bit and yielding the same key as `0x03`. Public keys appear *inside* signed objects (a persona in `FullOffer`, a contact card in §16.3), so a second encoding of one key is a second canonical object and a second hash. **Exactly one encoding is legal: compressed, 33 bytes, tag checked explicitly rather than left to the parser.** Legality in SEC1 is not the standard; uniqueness is.

Implementers should treat these as instances of a general rule rather than two special cases: **anywhere the protocol admits two byte representations of one value, it has a transcript-divergence bug**, whatever the signatures say.

**The same rule governs commitments, not just signatures.** The protocol hashes canonical objects in several unrelated roles — `offer_commit = H(FullOffer)` (§15.3), `H(RECEIPT)` inside the CONTACT bind (§16.3), the predecessor link in §6's message chain, `H(genesis descriptor)` for `market_id` (§10.1) — and a bare digest records none of that. Every commitment is therefore computed as:

```
commit = SHA-256( "DUCAT-v1" ‖ 0x00 ‖ purpose ‖ 0x00 ‖ canonical_bytes )
```

with `purpose` a fixed label (`"offer_commit"`, `"receipt"`, `"chain"`, `"market_genesis"`). Domain separation costs nothing here, and without it a digest computed for one role can be presented as another wherever an attacker can arrange for the underlying bytes to coincide.

## 18.4 The State Transition Table

§6 lists messages and §6.2 gives deadlines. Neither says which message is legal in which state, so this table is normative and exhaustive. **Any message not listed for the current state is a `STATE_VIOLATION` reject (§18.5), never a silent ignore** — silently ignoring unexpected messages is the single most reliable way for two implementations to diverge invisibly.

| State | Meaning |
|---|---|
| `IDLE` | No transaction in progress |
| `OFFERED` | Bootstrap received and verified; awaiting `FullOffer` |
| `QUOTED` | `FullOffer` verified against `offer_commit`; confirm screen rendered |
| `ACCEPTED` | Payer signed `ACCEPT`; price locked |
| `FUNDED` | Payment broadcast, or escrow funded |
| `PROVISIONAL` | `fast/1` only: `TXPROOF` verified, service may proceed, awaiting finality |
| `DELIVERED` | Profile-defined `PROOF` exchanged |
| `CLOSED` | `RECEIPT` co-signed — the transaction's normal terminal state |
| `SETTLED` | `fast/1` only: finality observed, obligation cleared, capacity restored |
| `ABORTED` · `CANCELLED` · `DISPUTED` | Terminal |

| From | On | Guard | To |
|---|---|---|---|
| `IDLE` | `TapPresent` (or ADVERT+HAIL) | Signature valid, unexpired, `nonce` unseen | `OFFERED` |
| `OFFERED` | `FullOffer` | `H(FullOffer) == offer_commit` | `QUOTED` |
| `OFFERED` | timeout 10 s | — | `IDLE`, silently — no screen is ever shown |
| `QUOTED` | `ACCEPT` | Human confirmed; locally recomputed price within tolerance | `ACCEPTED` |
| `QUOTED` | `ABORT` / timeout | — | `ABORTED` |
| `ACCEPTED` | `FUND` | Mode-appropriate per §8 | `FUNDED` |
| `ACCEPTED` | `CANCEL` | `terms.cancellation` applied (§7.3) | `CANCELLED` |
| `ACCEPTED` | timeout 60 s | — | `ABORTED` |
| `FUNDED` | `TXPROOF` | `fast/1`; proof verifies and tx is in mempool | `PROVISIONAL` |
| `FUNDED` / `PROVISIONAL` | `PROOF` | Profile-defined | `DELIVERED` |
| `DELIVERED` | `RECEIPT` | Both signatures present | `CLOSED` |
| `DELIVERED` | timeout 120 s | — | `CLOSED`, single-sided receipt (§6.2) |
| `CLOSED` | N confirmations | `fast/1` | `SETTLED` |
| `CLOSED` | cure window expiry, unconfirmed | `fast/1` | `CLAIMED` (§17.5) |
| `FUNDED` … `DELIVERED` | `DISPUTE` | Escrow modes only | `DISPUTED` |
| `CLOSED` | `CONTACT_OFFER` / `CONTACT_ACCEPT` | Within the 120 s contact window (§4) | `CLOSED` — contact is a side effect, not a state change |

### 18.4.1 Rules the table does not show

Implementing §18.4 surfaced six decisions the transition table leaves open. Each is now normative, because an implementer who guesses differently produces a client that interoperates until it suddenly doesn't.

1. **Direction is checked, not assumed.** The table is role-agnostic, but not every message is legal from every side. **Only the payer may emit `ACCEPT`**; a payee able to accept its own offer could drive the entire flow with no human checkpoint, which defeats §15.5. Likewise only the payee may emit `REFUND` (§7.3). Wrong-direction messages are `STATE_VIOLATION`.
2. **`CANCEL` has a closing bound as well as an opening one.** It is legal only between `ACCEPTED` and `FUND` — before the price is locked `ABORT` is the free exit and there are no cancellation terms yet, and once funds have moved cancellation is not a thing that exists. Post-`FUND` recourse is dispute (escrow) or slash (`fast/1`).
3. **The post-`ACCEPT` deadline is mode-dependent.** §6.2 lists "FUND after ACCEPT: 60 s" and "multisig setup: 300 s" as though they were different states. They are the same state under different settlement modes: 60 s for `direct` and `fast`, **300 s for `escrow`**, whose window is spent on multi-round multisig setup (§8.2). Its expiry MUST run the fund-recovery path, not a bare abort.
4. **The `FUNDED` deadline applies only under `fast/1`.** That 30 s bounds the wait for `TXPROOF`. Under `direct` and `escrow`, `FUNDED` awaits profile-defined delivery and carries no wall-clock deadline.
5. **Terminal states are absorbing.** `ABORTED`, `CANCELLED`, `DISPUTED`, `SETTLED`, and `CLAIMED` accept no further events, including timeouts. `CLOSED` is deliberately *not* terminal: it still admits the contact coda and `fast/1` finality.
6. **Elapsed time in an unbounded state is a no-op, not an error.** Clients poll on their own schedule, and a client that polls more often than another must not thereby reach a different state.

---

## 18.5 Rejects and Error Codes

A typed `REJECT { code, object_type, detail? }` is emitted on every refusal. Two implementations must fail *the same way* for a test suite to mean anything, and `detail` is advisory text that MUST NOT influence any automated decision.

| Code | Name | Code | Name |
|---|---|---|---|
| 1 | `BAD_SIG` | 9 | `UNKNOWN_FIELD` |
| 2 | `EXPIRED` | 10 | `MALFORMED` (non-canonical encoding) |
| 3 | `REPLAY` (nonce seen) | 11 | `STATE_VIOLATION` |
| 4 | `COMMIT_MISMATCH` | 12 | `INSUFFICIENT_CAPACITY` |
| 5 | `PRICE_MISMATCH` | 13 | `UNTRUSTED_ARBITER_SET` |
| 6 | `UNSUPPORTED_VERSION` | 14 | `RATE_STALE` |
| 7 | `UNSUPPORTED_SUITE` | 15 | `TIMEOUT` |
| 8 | `UNSUPPORTED_PROFILE` | 16 | `POLICY_REFUSED` |

`REPLAY` requires a seen-nonce cache. Its retention floor is the maximum `expiry` the client accepts — nonces cannot be forgotten sooner without reopening the replay window, and need not be kept longer.

## 18.6 Version and Suite Negotiation

§3 mandates crypto agility and every object carries `v` and `suite`. Neither is negotiated, and an unnegotiated agility field is the classic downgrade surface.

- The presenter advertises `supported = { versions[], suites[] }` in `FullOffer`.
- **Versions:** the reader selects the **highest mutually supported**. Version numbers are ordered by construction — higher means newer, which is the entire point of a version number.
- **Suites: highest-wins is wrong, and this document said so until 0.11.** Suite identifiers are allocated in registration order and encode no preference. Suite 1 is Ed25519/X25519; suite 2 is P-256, which exists *only* because iOS's Secure Enclave holds no Ed25519 key (§4.1). P-256 is a fallback forced by hardware, not an upgrade — so "highest wins" would silently select the weaker option on every platform pair supporting both. Worse, any suite added later for being cheaper or narrower would outrank its predecessors purely by arriving late. **Suites are therefore selected from an explicit preference list held by the payer**, over the intersection of offered and permitted; the numeric identifier is never compared. The payer decides because the payer's money is at risk — the same reasoning that puts `ACCEPT` in the payer's hands (§18.4.1).
- **Downgrade resistance needs no new machinery.** The advertised set lives inside `FullOffer`, and `TapPresent.offer_commit` already commits to the whole of it (§15.3). Stripping a suite changes the offer, changes its digest, and fails the commitment check. **Clients MUST verify the commitment before negotiating**, not after: negotiating first means selecting from an attacker-chosen menu and only then noticing.
- Clients MUST refuse an impermissible suite even when both sides "support" it. Backward compatibility is not a reason to accept broken cryptography. The permitted set is the intersection of the client's own policy with the market's (§10.1) — **a market narrows what its participants accept and can never widen it.** A market naming a suite the client excludes does not re-enable it.

## 18.7 Transport Bindings

Behavior is portable; identifiers are not. These must be pinned before any cross-implementation test.

**NFC.** A fixed ISO 7816 application identifier, in the proprietary range: `F0 44 43 41 54` (`0xF0` + `"DCAT"`) as a placeholder pending real RID registration. **This value cannot be discovered at runtime** — iOS readers must declare selectable AIDs at build time in `com.apple.developer.nfc.readersession.iso7816.select-identifiers`, so an AID change is an app update on every iOS client simultaneously. Fix it early and treat it as immutable.

**BLE.** One 128-bit service UUID and two characteristics — bootstrap-write and session-notify — plus the L2CAP PSM discovery mechanism for CoC sessions. Values TBD, but they must be assigned in this document before the first interop test, not chosen per-implementation.

**QR.** Two encodings for two purposes:
- **`inline` mode:** raw binary in QR byte mode behind a 4-byte magic `DCAT`. No text encoding, no base64 — the payload is bytes and byte mode carries them at full density.
- **`token` mode:** a `ducat:<base64url>` URI. The ~33% expansion is irrelevant at ~190 bytes, and a URI is shareable, linkable, and openable by a handler registration.

Error correction is a capacity trade worth stating precisely, since §15.3.2's 2,953-byte figure is the level-L best case: v40 byte mode carries **2,953 / 2,331 / 1,663 / 1,273** bytes at EC levels L / M / Q / H. Screen-displayed codes SHOULD use level M; printed codes exposed to wear and poor light SHOULD use Q. Measured inline routes reached 1,669 B against level Q's 1,663 B (§15.3.1), so a printed code SHOULD carry a `token` rather than gamble error correction against a blob size nobody controls.

## 18.8 Strictness: Postel's Law Is Wrong Here

**Unknown fields in a signed object MUST cause rejection** (`UNKNOWN_FIELD`). Unknown message types MUST be rejected. Unknown profile identifiers MUST be rejected rather than partially handled.

This inverts the usual "be liberal in what you accept," deliberately. WYSIWYS (§15.5) requires that the bytes a payer signs are exactly the fields their app rendered and verified; a client that tolerates fields it doesn't understand is by definition signing something it did not display. Liberal acceptance is also how protocol extensions become de-facto forks — one implementation starts emitting a field, others silently ignore it, and the "same" protocol quietly means two things.

Extension is therefore explicit only: a new version, or a new profile identifier, both negotiated per §18.6.

## 18.9 Test Vectors and the Conformance Suite

Vectors cannot be authored ahead of a reference implementation, so this section specifies **required coverage** rather than pretending to contain them. The suite is a deliverable of Phase 1 and gates the claim of a second client.

Required coverage:

1. **Encoding round-trips**, including maps whose key ordering differs between naive and canonical encoders, and every integer boundary (0, 23, 24, 255, 256, 65535, 65536, 2³²−1, 2⁶⁴−1).
2. **Per object type:** one valid instance, plus invalid mutations that MUST each produce the specified reject code — bad signature, expired, replayed nonce, non-canonical encoding, unknown field, wrong domain-separation label.
3. **Cross-context replay:** a signature valid for one `object_type` presented as another. MUST reject (§18.3).
4. **Full transcripts** for every buildable-now profile (§7), each verifiable end to end from `TapPresent` through `RECEIPT`.
5. **Failure paths:** every timeout in §6.2, and the single-sided receipt.
6. **Negotiation:** successful selection, plus a stripped-`supported` downgrade attempt that MUST fail verification.
7. **Money:** amounts at piconero granularity that a float implementation would round incorrectly — this vector exists specifically to fail non-conformant clients.

Format: a JSON manifest of cases with hex-encoded inputs, expected outputs, and expected reject codes, one directory per cipher suite. Vectors are versioned with the protocol and a client claims conformance against a named vector-set release.

## 18.10 Conformance Levels

So that "DUCAT client" means something specific:

| Level | Requires |
|---|---|
| **Core** | §18.1–18.8 in full, **both the Ed25519/X25519 and P-256 suites** (§4.1 — otherwise personas fragment by platform), QR transport, `xfer/1`, `direct` settlement. The floor for using the name. |
| **Proximity** | Core + NFC and/or BLE transport + `pos/1` |
| **Fast** | Proximity + `fast/1`, bonded float, `TXPROOF` verification, slashing state machine |
| **Full** | Fast + escrow modes, arbitration, escrow-gated profiles |

A client MUST declare its level and the vector-set release it passes. A client that cannot pass Core vectors is not a DUCAT client regardless of what it implements — the point of levels is to make partial implementations legible rather than to make the name negotiable.

---
*End of Part V. One line: behavior is specified in Parts I–IV, but interoperability lives in canonical bytes, domain-separated signatures, an exhaustive state table, strict rejection, and a vector set that a second implementation can fail.*
