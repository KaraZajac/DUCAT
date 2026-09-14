# Trust, identity-wide: what DUCAT has, what others built, what to build

*2026-09-14. Research memo for the post-1.0 track. Question asked: should DUCAT add a
second blockchain (Ethereum, or a token on it) so that holding coins conveys trust,
and has anyone solved peer-to-peer trust through a public ledger?*

## 1. The short answer

**No second chain, no token.** A public ledger cannot give DUCAT anything it needs
for trust that Monero plus the DHT cannot already give, and it would take away the
property the whole spec is built on: that a persona's actions are unlinkable unless
the person chooses otherwise (§4, §5, §16.6). A token balance is not trust in any
case — it is purchasable, transferable, and the single most Sybil-farmed mechanism
in the industry (§3.4 below).

What conveys trust between strangers who will never meet a company is **money at
risk under rules both sides can verify**, and the spec already says so: §9 "L5 —
Trust" is "the layer that Uber/Airbnb *are*, rebuilt without a company. No component
here proves identity; all of it prices risk." The layer is designed and, apart from
the per-deal stakes and the board stamps, **not built**. The clients carry
`attestation_records: vec![]`, the bond and slash objects exist only in `core/`
(the security pass's finding M14), and §9.2's rated receipts are never issued.

The realistic plan is therefore to build §9 rather than replace it, with one
addition the spec does not have and Monero makes uniquely good: **costly identity
that is private** — proof of burn (and of bond funding) verified with Monero's own
transaction proofs, shown only to the counterparty who asks. Bitcoin systems that
do this (Bisq, JoinMarket) pay for it with a public, linkable bond address; on
Monero the burn is on a public chain but the amount and the sender are hidden from
everyone except the person the proof is handed to. That is the one genuinely new
thing DUCAT can offer here.

## 2. What DUCAT already has (as of draft 1.1.0-dev10)

| Mechanism | Where | State |
|---|---|---|
| Persona keys, unlinkable per role; stake keys bound to one persona | §4 | built (personas); stake keys unused |
| In-person card exchange as the root of trust; claim-once cards; signed inbox halves | §16.9 | built |
| Proof-of-work stamps on every board notice, bound to a Monero block | §16.18.1 | built |
| Per-deal symmetric stakes (2-of-2) on rentals and bonded rides (2-of-3 with an arbiter contact) | §15.12, §17.9 | built |
| "Established" badge: a listing's per-listing key is stable across refreshes | §16.18.1 | built |
| Bonded stakes as cost-of-identity ("a scammer *can* regenerate a persona — but must re-bond each time") | §9.1 | designed only |
| Pseudonymous reputation: rated `RECEIPT`s signed to a persona, in DHT records the persona controls, weighted by the *counterparty's* stake | §9.2, §16.7 | designed only; backup slot exists, always empty |
| Arbitration market: market-signed arbiter set, mechanical vs judgment disputes, bonded arbiters accountable for provable misconduct | §9.3, §10.1 | designed only; clients use a chosen contact |
| Bonded zero-confirmation float (`fast/1`), bond proofs, slash claims, `TXPROOF` | §8.6, §17 | core objects + vectors; no client path |

The design already answers the Sybil question the way the best deployed systems do
(§3): identity is cheap, *credibility* costs money, and fraud costs money per
instance. What is missing is the plumbing, and one primitive.

## 3. Who has built this, and what happened

### 3.1 Bond-and-burn reputation — Bisq 2 (closest working analogue)

Bisq 2 (Bisq Easy) sells bitcoin for fiat between strangers with no escrow on the
fiat side, so it needs seller reputation. Sources, all Sybil-resistant because each
costs something:

- **Burned BSQ**: 100 points per BSQ burned, minimum burn 5.46 BSQ, persists for ever.
- **Bonded BSQ**: 10 points per BSQ, locked for at least 50,000 blocks (~1 year);
  the points vanish when the bond unlocks. Burn is weighted 10× bond.
- **Account age**: 2.5 points/day, capped at 5,000; **signed account age witness**
  (a past counterparty signed that a real trade happened): 5 points/day, capped at
  10,000.
- **Minimum 10,000 points to have a sell offer accepted**; the score is shown as
  0–5 stars relative to active users; bonds can be confiscated by DAO vote for
  scamming.

Lessons: the score gates *what you may do*, not who you are; burn beats bond
because it cannot be recovered; age only counts once it is witnessed by a
counterparty. ([Bisq wiki: Reputation2](https://bisq.wiki/Reputation2),
[Reputation](https://bisq.wiki/Reputation),
[account age witness](https://github.com/bisq-network/bisq-docs/blob/master/payment-account-age-witness.adoc))

### 3.2 Fidelity bonds — JoinMarket (the Sybil math)

Chris Belcher's design for JoinMarket makers: bond value = `V²` for burned coins,
`V²·(e^{rT} − 1)²` for coins time-locked for `T` at an assumed rate `r`. Quadratic
so that one big bond beats many small ones (an attacker running many bots cannot
win by splitting). Takers pick makers weighted by bond value. With real liquidity
numbers an attacker would need to lock 30,000–80,000 BTC for six months, or burn
45–120 BTC, for a 95% chance of surrounding a taker. The stated cost: the bond
transaction is public, so makers must mix first or the bond deanonymises them.
([design gist](https://gist.github.com/chris-belcher/18ea0e6acdb885a2bfbdee43dcd6b5af),
[docs](https://github.com/JoinMarket-Org/joinmarket-clientserver/blob/master/docs/fidelity-bonds.md),
[bitcoin-dev](https://gnusha.org/pi/bitcoindev/985792b1-e7aa-677b-a7a1-6a5f672da884@riseup.net/))

### 3.3 Bonds with no reputation at all — RoboSats

RoboSats mints a fresh "robot" identity per session and has no reputation system;
every trade is backed by maker and taker fidelity bonds (2–15%, default 3%) as
Lightning hold invoices that are slashed on cheating or unilateral cancel. It works
because the bond is per trade and sized to the trade. This is DUCAT's existing
per-deal stake, and it is the floor: reputation is what lets two people trade
*without* a full bond each time. ([bonds doc](https://github.com/RoboSats/robosats/blob/main/docs/_pages/docs/03-understand/04-bonds.md),
[trust vs privacy issue #39](https://github.com/RoboSats/robosats/issues/39))

### 3.4 What failed — OpenBazaar, and token airdrops

OpenBazaar had star ratings feeding a reputation score plus 2-of-3 escrow with
open-market moderators paid a percentage of released funds. Ratings were inflated
by sock puppets; sellers and moderators colluded (or were the same person) to open
a dispute the moment an order was paid; the "verified moderators" program died with
OB1. The mechanism that survives from that experience is the one DUCAT's §9.3
already specifies: arbiters named in a signed set, bonded, paid from a published
schedule, accountable for *mechanical* rulings the chain can contradict.
([reputation part 1](https://medium.com/openbazaarproject/decentralized-reputation-in-openbazaar-1a577fac5175),
[part 2](https://medium.com/@therealopenbazaar/decentralized-reputation-part-2-6233cf2127bb),
[moderators](https://medium.com/openbazaarproject/how-moderators-and-dispute-resolution-work-in-openbazaar-7c98bfa15388))

"Give new accounts a few coins" is exactly an airdrop, and 2025 is the year the
numbers came in: Linea filtered ~800,000 Sybil wallets against ~749,000 genuine
claimants; LayerZero removed 803,273 wallets; zkSync flagged 60% of addresses; 64%
of recipients sold at the token generation event. A balance handed out for free is
farmed, and a balance that can be bought measures wealth, not conduct.
([CoinLaw 2026](https://coinlaw.io/token-airdrop-statistics/),
[Streamflow](https://streamflow.finance/blog/sybil-attack-problem),
[Dragonfly report, SEC copy](https://www.sec.gov/files/dragonflys-state-airdrops-report-2025.pdf))

### 3.5 Proof of personhood — the road not to take

World ID (12M+ verified by iris scan, retail verification stations), Human
Passport (ex-Gitcoin; "stamps" aggregated into a Unique Humanity Score with a
machine-learning Sybil model, attestations minted through the Ethereum Attestation
Service), Proof of Humanity v2 (soulbound IDs with vouching and Kleros
challenges), BrightID (social-graph verification parties). Every one of these
answers "is this one human?" and does it with biometrics, a scoring service, or a
public registry. DUCAT does not need uniqueness — a person may hold many personas
by design — and cannot accept a public registry. ([World](https://world.org/blog/world/benefits-proof-personhood-numbers),
[Human Passport × Base](https://human.tech/blog/human-passport-x-base-scaling-sybil-resistance-for-the-next-wave-of-builders),
[PoH v2](https://hackmd.io/@andreimvp/poh), [Wikipedia](https://en.wikipedia.org/wiki/Proof_of_personhood))

Soulbound tokens ("Decentralized Society") were the on-chain version of a dossier;
the critiques that followed — public exposure of credentials, no real revocation,
consent that cannot be enforced — are the reasons the spec keeps attestations in
records the persona controls (§9.2) rather than on any ledger. ([Chainlink SBT overview](https://chain.link/article/what-are-soulbound-tokens),
[Plural identity frontier](https://arxiv.org/pdf/2208.11443))

### 3.6 Trust graphs — Circles, Trustlines, Nostr

Circles v2 and Trustlines make trust *literal credit*: a trust link means "I will
accept your IOU / your personal currency", and payments route along chains of
friends. Nostr's web of trust is computed client-side from the follow graph (seed
pubkeys, PageRank-style propagation, local cutoffs), with no global score. Both
show a graph rooted in real relationships can be computed privately on the
client. DUCAT's contact cards are exchanged in person, which is a stronger edge
than a follow. ([Circles 2.0](https://www.gnosis.io/blog/introducing-circles-v2-money-for-a-multipolar-world),
[Trustlines](https://docs.trustlines.network/resources/wp_content/how_trustlines_works/),
[Nostr WoT](https://nostrcompass.org/en/topics/web-of-trust/),
[nostr-wot toolkit](https://github.com/nostr-wot/nostr-wot))

### 3.7 The academic line

EigenTrust (PageRank over ratings; vulnerable to collusion), Advogato's max-flow
metric from a trusted seed, SybilLimit (bottlenecks in a social graph bound the
number of Sybils per honest edge) — all assume a social graph with scarce edges,
which in-person card exchange provides. Anonymous reputation systems from the
crypto literature — one-show credentials from blind signatures (RepChain), linkable
ring signatures per purchase (ARS-Chain), anonymous credentials with reputation
(CLARC) — show how a rating can be provably from a real customer without naming
the customer, the same trick Monero's ring signatures play with spends.
([EigenTrust](https://nlp.stanford.edu/pubs/eigentrust.pdf),
[SybilLimit](https://www.researchgate.net/publication/4339925_SybilLimit_A_Near-Optimal_Social_Network_Defense_against_Sybil_Attack),
[Resisting Sybils in P2P markets](https://dl.ifip.org/db/conf/ifiptm/ifiptm2007/Traupman07.pdf),
[RepChain](https://www.researchgate.net/publication/353810997_Anonymous_and_Verifiable_Reputation_System_for_E-commerce_Platforms_based_on_Blockchain),
[CLARC](https://dl.acm.org/doi/10.1145/3230833.3234517))

Zero-knowledge group membership (Semaphore, RLN) proves "I am one of this set"
on a phone in under three seconds with a proving key under 5 MB; BBS+ credentials
(W3C Data Integrity BBS, AnonCreds v2) give unlinkable selective disclosure. Both
are the later, private form of "show me your record". ([RLN docs](https://rate-limiting-nullifier.github.io/rln-docs/),
[Waku RLN](https://research.logos.co/rlog/rln-anonymous-dos-prevention),
[W3C vc-di-bbs](https://www.w3.org/TR/vc-di-bbs/),
[AnonCreds v2](https://github.com/anoncreds/anoncreds-v2-rs))

## 4. The Monero primitives, and one door that closed

- **Transaction proofs.** `get_tx_proof` produces an `OutProofV2`: a proof, made by
  the sender from the transaction's secret key, that transaction `txid` paid the
  stated amount to address `D`, bound to a message of the sender's choosing.
  Anyone with a node verifies it with `check_tx_proof <txid> <address> <proof>`
  using only the address's public keys. DUCAT already carries this blob inside a
  `SlashClaim` (`TXPROOF`, core/src/escrow.rs) and verified a real one in
  `monero-spike/`. ([prove payment](https://www.getmonero.org/resources/user-guides/prove-payment.html),
  [wallet RPC](https://docs.getmonero.org/rpc-library/wallet-rpc/))
- **Reserve proofs.** `get_reserve_proof` proves control of at least X XMR without
  the view key, by disclosing chosen unspent outputs with signed key images. It
  proves wealth, not sacrifice, and it links the outputs it discloses; useful at
  most as an optional "capitalised seller" signal. ([PR #3027](https://github.com/monero-project/monero/pull/3027))
- **Timelocks are gone.** Custom `unlock_time` is deprecated (relay rule blocks it,
  ignored at consensus after the FCMP++ fork), so a JoinMarket-style time-locked
  bond cannot exist on Monero. A bond is therefore either a multisig escrow with
  the arbiter set (§17.2, as the spec already concluded) or a burn.
  ([getmonero.org, May 2026](https://www.getmonero.org/2026/05/10/deprecating-unlock-time.html),
  [research-lab #125](https://github.com/monero-project/research-lab/issues/125))
- **Not in our crates.** `monero-wallet`, `monero-interface`, `monero-daemon-rpc`
  and `monero-primitives` have no proof generation or verification; the spike used
  `monero-wallet-rpc`. Generating an OutProofV2 needs the transaction secret key
  kept at send time; verifying one is a Schnorr-style check over the transaction
  public key and the recipient's public view key. Both are a bounded piece of
  Rust with `check_tx_proof` as the test oracle.

## 5. Recommendation: build §9, with a burn leg, all of it verifiable by the counterparty

Trust in a peer-to-peer market is bilateral: the person about to hand over money or
goods needs to check, nobody else does. So every element below is **a proof the
persona hands over inside the sealed thread or on its listing, verified by the
reader's own client and node**. No global score, no ledger anyone else reads.

### 5.1 Three legs and a floor

| Leg | What it proves | How it is verified | Sybil cost |
|---|---|---|---|
| **Floor: per-deal stakes and stamps** (built) | skin in the game for *this* deal; work spent for *this* notice | 2-of-2/2-of-3 escrow; Argon2 stamp | none for identity, real for each act |
| **Cost of identity: burn** (new) | this persona sacrificed X XMR to a provably unspendable DUCAT address, at block h | `BURN_PROOF { txid, amount, height, OutProofV2 }`, message = persona key ‖ purpose; reader checks it against its node and the second-opinion node (§17.5's three answers) | X per persona, unrecoverable |
| **Cost of identity: bond** (§9.1/§17.2, unbuilt) | X XMR is locked in a 2-of-3 with the market's arbiter set and slashable | `BOND_PROOF` (exists in core) + arbiter attestation freshness (§17.4) | X per persona, recoverable if honest |
| **History: rated receipts** (§9.2, unbuilt) | N counterparties, each with their own burn/bond, signed a rated `RECEIPT` to this persona | receipts in the persona's attestation record; each receipt's weight = the *signer's* verified burn+bond, so a fake reviewer costs as much as a fake seller | proportional to the reviewers' cost |
| **Vouching along in-person edges** (new, optional) | a contact who exchanged cards in person signs "known" | signed attestation from a contact the reader also holds; the reader's client computes distance locally (Nostr-style, never published) | scarce edges (SybilLimit's assumption) |

### 5.2 Scoring, shown as words not numbers

Follow Bisq's shape and JoinMarket's arithmetic, stated in XMR so no oracle is needed:

- burn points = 100 per 0.01 XMR burned, no cap, **quadratic in the single largest
  burn** (many small burns from many personas do not add up to one big one);
- bond points = 10 per 0.01 XMR bonded while locked;
- age points = 2.5 per day since the persona's *first burn block* (a birth
  certificate nobody can backdate — the block hash is in the proof), capped;
- history points = Σ over receipts of min(signer's own points, cap), rated;
- vouches shown as "2 of your contacts know this person", never as points.

The client renders a badge — "bonded 0.20 XMR · burned 0.05 XMR · 14 receipts" —
and, the part that matters, **gates**: a seller may set a minimum buyer score, and
a buyer's client warns when the amount at risk exceeds what the seller's
burn+bond covers (Bisq's rule, RoboSats' floor). Fraud then costs the scammer more
than one scam yields, per identity, which is the only property any of these
systems actually delivers.

### 5.3 Privacy accounting (§16.6's habit)

- A burn is a Monero transaction: the chain shows a transaction exists, not the
  amount, not the sender, not the persona. The proof reveals the amount and binds
  the persona **only to the reader it is handed to**. Fund the burn from the
  persona's own wallet so nothing links personas.
- Receipts name the signer's persona to the reader. That is §4's stated dossier
  trade; keep it opt-in per receipt, and plan the private form (blind-signed or
  linkable-ring receipt tokens, §3.7) for when it is worth the complexity.
- Nothing above is published to a board or a ledger. The DHT record that holds a
  persona's attestations is readable by whoever holds its key, which the persona
  hands out with the listing or in the thread.

### 5.4 Why not Ethereum, in one table

| Ethereum / token | DUCAT equivalent |
|---|---|
| public, permanent, linkable balance and history | Monero burn: public existence, private amount and sender |
| balance is buyable and transferable | burn is sacrificed and bound to one persona |
| airdrop to new accounts | farmed at scale (§3.4); DUCAT gives new personas nothing and lets them earn the floor by staking per deal |
| gas, bridges, a second wallet and network on a phone | one chain the app already scans |
| attestations on EAS / SBTs readable by everyone | attestation records the persona controls |
| regulatory surface of issuing a token | none |

The one thing a public ledger buys is *global* verifiability without asking. A
market between two people does not need it: the counterparty asks, and Monero's
proofs answer.

## 6. Steps, in order

1. **Spec.** A §9.5 "Costly identity on Monero": the burn address (nothing-up-my-
   sleeve derivation, e.g. spend and view public keys hashed from
   `"DUCAT-BURN-v1"`, published with the derivation so anyone can check no secret
   exists), `BURN_PROOF` as a wire object (new type code, registry rows), the
   verification rule (node + second opinion, `unknown` never `yes`), the score
   curve, and what a reader MUST refuse (a proof whose message is not this persona,
   an amount below the floor). Register `ATTESTATION` (type 12 already) as the
   rated receipt of §9.2 with its fields. Vectors for all of it.
2. **Bridge.** Keep the transaction secret key at send time; implement OutProofV2
   generation and verification in `mobile/src/monero.rs` with `check_tx_proof`
   (monero-wallet-rpc, stagenet) as the oracle in `harness/`.
3. **Clients.** Burn from the wallet screen with the cost stated plainly; store
   the proof in the persona's attestation record and the backup (`attestation_records`
   is already in the bundle format, always empty today); issue and store rated
   receipts after a settled deal (§16.7); show the badge on listings, hails, tills
   and in the thread; gate amounts against it; "Ask for their record" in the thread.
4. **Bonds.** Build §17.2's float against a market arbiter set once §9.3's arbiter
   market exists; until then the per-deal 2-of-3 with a chosen contact stays the
   bonded path.
5. **Later.** Private receipt tokens (blind signatures or linkable ring
   signatures); ZK "one of the bonded set vouches" (RLN-class proofs run on phones);
   vouching attestations along contact edges.

## 7. What this does not solve

Trust priced in money protects against fraud up to the amount at risk; it does not
protect a person from a counterparty who is dangerous rather than dishonest, and
it says nothing about quality (§9.3.1's judgment class stays out of scope). A
determined scammer who expects to net more than the burn will burn. The score is
a floor under which one should not trade unbonded, not a promise.

## 8. Build plan (agreed 2026-09-14)

Order of work, each step landing on its own with its gates green. Sizes are
working days for one person; "proof" is what closes the step.

### Phase 1 — money lost today (before any trust work)

| Step | What | Proof |
|---|---|---|
| 1.1 | Kiosk: `Seen` renders as settling; the paid panel and Ready need `Confirmed`; an operator setting "hand over on first sight up to X" (default off) with the risk stated beside it. Both clients. Pool-sighted orders take the second opinion too. (M1, M10) | kiosk walk on two phones: a sighting shows settling, a block shows paid; the cap works |
| 1.2 | Phone second opinion to the desk's rule: `InBlock` settles, pool/unknown/silence defer, ten-minute stall → "settle anyway", small-sale floor, confirmations by amount, own node honoured, never the node in use. (M3/N2 phone side, N22) | `SecondOpinion` unit tests on a walked clock; bar-tab walk |
| 1.3 | Fee ceiling: a real `max_per_weight`, refuse a built fee over the quote by 25% or over 5% of the amount, both send paths and the escrow proposer; co-signers refuse a fee over the ceiling. (N1, N10) | unit tests with a lying quote; a stagenet send |
| 1.4 | Escrow consent from the transaction, not the balance: `frost_destinations` returns inputs total and fee; the co-signer requires inputs ≥ funded, sizes its residual from inputs − fixed − fee, bounds the fee; the arbiter's screen lists parsed outputs and refuses any address outside the two parties; the joiner checks fare against the accept and stake against the schedule; the consent tap carries the displayed figure. (M2, M4, M5, M11) | ceremony tests with a hostile proposer; a bonded ride on the emulators |
| 1.5 | Fetch byte cap and index validation at decode; seeder back-pressure; per-kind budgets. (N4/D2, N5, N26) | a hostile index in the harness is refused before a byte lands |

### Phase 2 — the spec

| Step | What | Proof |
|---|---|---|
| 2.1 | §9.5 "Costly identity on Monero": the burn address and its derivation, `BURN_PROOF` (txid, amount, height, OutProofV2, message = persona ‖ purpose), the verification rule (own node plus second opinion; *unknown* never *yes*), the score curve, what a reader MUST refuse. | audit_spec 0 problems |
| 2.2 | `ATTESTATION` (type 12) given its fields: the rated `RECEIPT` of §9.2 — subject persona, rating, the settled amount, the signer's persona, the txid or receipt hash it stands on — and the attestation record's layout in the persona's DHT record; the backup slot that already exists. | vectors for the burn proof, the attestation, and the refusals; 2 implementations agreeing |
| 2.3 | The gate rule: a client MUST warn when the amount at risk exceeds what the counterparty's burn + bond covers, and MAY refuse; a seller MAY set a minimum buyer score. Words, not numbers, in the UI. | the sentence in §9.5 and the client behaviour in 4.2 |

### Phase 3 — the bridge

| Step | What | Proof |
|---|---|---|
| 3.1 | Keep the transaction secret key at send time (per send, in the wallet store, owner-only). **Done 2026-09-14**: the send path derives the key the crate derives and hands it back; both clients keep it on the send record. | a burn can be proved a day later |
| 3.2 | `OutProofV2` generation and verification in Rust (`mobile/src/txproof.rs`): the Schnorr-style proof over the transaction public key and the recipient's public view key, with the message bound. **Done 2026-09-14.** | **Proven**: the first burn to the stagenet burn address (txid `0b1a7ac3…9301b`, block 2207293, 0.01 XMR); monero-wallet-rpc's proof verifies in our code for exactly that amount, our proof from the transaction key is accepted by monero-wallet-rpc (`good: true, received: 10000000000`), and a wrong message fails both ways (`mobile/examples/txproof_oracle.rs`) |
| 3.3 | Burn: send to the DUCAT burn address from the wallet, keep the proof in the persona's attestation record and the backup. | a burn on stagenet, restored from a backup, still verifies |

### Phase 4 — the clients

| Step | What | Proof |
|---|---|---|
| 4.1 | Burn from the wallet screen, the cost said plainly and irreversibly; the proof stored and carried. Both clients. | walk on phone and desk |
| 4.2 | The badge on listings, hails, tills and in the thread; the gate; "ask for their record" in a thread, answered from the attestation record; a seller's minimum. **Desk, thread part, done 2026-09-14**: the badge in the thread header ("burned 0.01 XMR, since block N · 3 receipts, 1 from burned personas · 4.7 ★"), "Show my burn" / "Check" on a `ducat:burn/` bubble, the gate warning in the pay form whenever the amount exceeds the counterparty's verified burn. Still to do: listings, hails, tills; a seller's minimum; the phone. | marketplace walk with a burned seller and an unburned one |
| 4.3 | Rated receipts after a settled deal, signed to the counterparty's persona, weighted by the signer's own burn; shown on request. **Desk done 2026-09-14** (`app/src/trust.rs`, spec §9.5 "How a receipt travels"): "Rate them" appears once the thread holds a receipt, signs an `ATTESTATION` and sends it as `ducat:attest/`; the subject keeps it only if the sender signed it and it is about one of its personas; "Show my record" sends `ducat:record/<hex>.<hex>…`; a reader keeps what opens and is about the sender and weighs it itself — one voice per signer, rated by the latest, counted only when that signer's burn was verified here. Tests cover the travel, the one-voice rule and the refusals. **Proven live 2026-09-14** desk↔desk (`app/examples/mailbox.rs` `rated` / `rater`): the receipt landed in the rated desk's thread and was kept by the inbox on its own, the record came back, and the rater read it (1 receipt, 0 weighted — no burn verified for the rater, as it should be) in 47 s. Phone pending. | kiosk and rental walks produce receipts both sides can show |
| 4.4 | Vouching along in-person card edges, computed locally, never published. | "2 of your contacts know this person" on a three-phone walk |

### Phase 5 — later

Bonds through §17.2's float once §9.3's arbiter market exists; private
receipt tokens (blind or linkable-ring signatures); zero-knowledge "one of
the bonded set vouches" proofs; per-listing pseudonymous personas for public
cards (W4/N3).

### Decisions taken

- **Burn floor**: start at the price of a coffee — 0.01 XMR at today's rate,
  a named constant to tune — and let the gate, not the floor, do the work.
- **Receipts name the reviewer** to the reader in the first version; the
  private form is Phase 5.
- **Phase 1 before Phase 2**: the review's open money items are money lost
  today; the trust work starts when they are closed.
- **No second chain, no token**, for the reasons in §1 and §5.4.
