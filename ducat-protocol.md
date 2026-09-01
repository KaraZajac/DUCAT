# DUCAT — A Peer-to-Peer Proximity Commerce Protocol
**Draft 1.1.0-dev6 — Publications track (branch; 1.0.0-rc1 is the frozen line)**
*A ducat was a gold coin accepted from Venice to Vienna to the Levant for six centuries. It had no issuer relationship, no account behind it, and no permission attached — it was worth something because you were holding it, and it crossed borders the way a bearer instrument should.*
Canonical home: **ducatproject.org**

Status: Release candidate. The feature line is frozen as of 2026-08-30: no new wire objects, kinds, or fields before 1.0.0. The word "draft" comes off when three gates close, none of them features — the hardware field day (NFC tap, real GPS, two radios), the external adversarial review §2.5 still records the absence of, and an implementer building from this document alone (§18, O21). Until the review lands, treat §17.9's ceremonies and the board/mailbox/beacon surfaces as unreviewed. §14 remains the honest agenda.

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
- **1.1.0-dev6** — **A publication is discovered like a kayak (§16.18.2, `PUB_NOTICE`, fields 265–275).** The digital good takes the same boards: a claim-once publish card, a title, an optional blurb and an optional per-period price ride a notice whose stamp block is §16.18.1 verbatim in its own namespace — and whose board name, inside the signature like a slot, carries the where: `topic:<category>[.<lang>]` worldwide (six pinned categories, `other` the valve), `local:<cell>` for the town paper, cross-posting two honest stamps. Absent price means free and an explicit zero is `MALFORMED` — one meaning, one encoding. Claiming the card IS subscribing; everything after is §16.20 unchanged. Eleven new vectors pin both edges including the family's sealed form on a topic board and its unknown-field edge (276) — which caught the second implementation answering `MALFORMED` where core answers `UNKNOWN_FIELD` on every strict reader's leftover path, a divergence no vector had ever exercised; the checker now agrees with core in core's own words. **And the backup dresses every hat (§4.3):** each roster entry now carries its persona's own §16.9 profile — public name, avatar, reach-me identifiers, pronouns, the car, and the share switch (written only when off; absence decodes as on, the stored default) — as optional per-persona keys 4–13, because the presentation follows the hat and a restore that returns three keys wearing one face has quietly merged what the compartments kept apart. The primary's copy still also rides the legacy top-level fields, so a reader from the single-profile era restores what it always did; per-persona wins where both exist. One vector pins the enumeration at its edge, both sides. 367 cases, both implementations agreeing.
- **1.1.0-dev5** — **The answer takes the open door (§16.21 control frames).** A media frame with sequence `0xFFFFFFFF` carries control: type byte (1 ANSWER, 2 DECLINE, 3 BYE), the call id, and for ANSWER the callee's route blob. The offer already delivered a live route; using it for the reply turns connect and decline from two cold mailbox trips (~25–45 s each, measured) into one route trip (~200 ms). The sealed kind-15/Retract remains canonical and MUST follow; frames are honored only against a live call's id on its own route — possession-of-the-sealed-offer trust, identical to the media's. BYE gives hang-up a word; RENEW re-aims a call around a bad route draw mid-flight; the silence watchdog stays for crashes. No wire objects, kinds, or fields changed; vector count stays 355.
- **1.1.0-dev4** — **Call media is Opus, and constant-rate on principle (§16.21).** Client format v1 replaces the provisional PCM: 8-byte header then one Opus packet — 16 kHz mono, 20 ms, hard CBR at 24 kbit/s, 60 bytes every frame, DTX forbidden. CBR is stated as a privacy rule: encrypted VBR voice leaks speech through packet sizes (phrase spotting, phoneme reconstruction), so a frame leaves the same size whether the speaker talks or holds their breath. The call-route cap moved 1200 → 4096: route blobs embed the full peer-info of their hops, and a live phone allocated one past 1200 the day after the cap was pinned — both edge vectors re-pinned at the new cap. Vector count stays 355.
- **1.1.0-dev3** — **A call is a door handed down the thread (§16.21, kinds 14–15, fields 263–264).** `CALL_OFFER` carries a fresh private-route blob (1–4096 bytes) and an eight-byte call id; `CALL_ANSWER` carries the callee's own route and quotes the id. The pair travels whole or not at all, the door IS the kind (an offer with no route rings nothing; a route on any other kind is `MALFORMED`), and no amount rides either. Media never touches the mailbox: it flows as Veilid app-messages on the offered routes — client format v0 is an 8-byte header (`seq` u32be ‖ `ms` u32be) and a codec frame, PCM16 mono 16 kHz in 20 ms frames until an Opus dependency lands, provisional like the shelf's record layout. Declining is §16.13's Retract naming the offer; hanging up is stopping; a missed call is simply a message read later — ringing was never a separate channel. Grounded in measurement, not hope: p50 route RTT 187 ms and 500-of-500 frames delivered at the 50 Hz voice cadence with 65 ms jitter (research/post-1.0/CALLS.md). Ten new vectors pin the pair's edges and move the unknown-kind sentinel to the new edge (16); 355 cases, both implementations agreeing.
- **1.1.0-dev2** — **A heavy period ships by swarm (§16.20, fields 261–262).** The kind-13 manifest grows the shipment pair: the swarm share key a fetcher bootstraps from, and the index digest that authenticates what answers — together or not at all (a key without its digest bootstraps into whatever replies, which is not a fetch, it is an ask), and only aboard a publication key (away from the period pair the shipment describes nothing). The engine underneath is vendored from cmars's stigmerge with credit and three upstream-candidate patches (mobile/vendor/STIGMERGE-NOTICE.md), converted to BLAKE3 pieces, riding the same node as the mailbox with inbound calls demultiplexed by route; proven live twice — 100 MiB between two nodes at ~3 Mbit/s process-to-process, then 25 MiB desk-to-desk at ~6.7 Mbit/s through the clients' own Kotlin path — payload BLAKE3 identical at both ends in every run. Five new vectors pin the pair's edges; 345 cases, both implementations agreeing.
- **1.1.0-dev1** — **A publication period's key rides the paid thread (§16.20, kind 13, fields 257–260).** The post-1.0 track's first wire object, developed on its own branch while 1.0.0-rc1 stands frozen. A publisher seals content into DHT records — the shelf — and sells periods of it; what a paying reader receives is never the content but the **capability**: `PUBLICATION_KEY` carries the period's id and its 32-byte content key (together or not at all — a key with no name cannot be filed, a name with no key opens nothing), plus, on first delivery, the shelf itself: the publication's root record and the standing head key that opens its index (likewise together or not at all). The closed world holds in both directions — a kind-13 with nothing to hand over is `MALFORMED`, and a period key on any other kind is a capability smuggled where no reader is looking for one; an amount on it is refused the way Text refuses one. The period id is a label (≤64 chars), pinned at both edges — 64 accepted, 65 refused, and the empty spelling refused below the field layer as a second encoding of omission (§18.1). Twelve new vectors; 340 cases, both implementations agreeing. Client-side, the key is derived, not stored: one master secret and `derive_key`/keyed-BLAKE3 per period, so a back-catalogue sale is a re-derivation and a restore restores every key ever issued (core::publish, pinned by unit test pending its own vector kind).
- **1.0.0-rc1** — **The feature line freezes.** No wire change; 328 vectors unchanged, both implementations agreeing. This candidate names what 1.0.0 means and what still stands between this document and that number. Frozen in: everything below — the tap, the mailbox, cards and profiles, bills/receipts/settlement, the escrow ladder from bond to ruling, boards with stamps and generations, listings across five kinds, groups, references, live position. Declared limitations rather than gaps: **refunds** — there is no path to return money after settlement (`cancel` withdraws an unpaid bill, `markPaidOutside` records another rail; neither is a refund), and building one starts with a design question about what the payer's record should show, deferred past 1.0 deliberately; **the co-signer's blind fee** — a FROST co-signer sees the fee it reads from the bytes but not the payment list, until monero-wallet exposes a payments accessor on `SignableTransaction`. Three gates before the number: the field day (the NFC tap has never met hardware; real GPS; the OEM restore picker), the adversarial review this document has said since §2.5 it has never had — scope: the §17.9 ceremonies, the §16.12 mailbox, the board and §16.18.1 beacon surfaces, newest least-reviewed first — and O21's reader, a second implementer working from this text alone. 1.0.0 is this document with those three receipts attached and the word "draft" removed, not a feature away.
- **0.90** — **Small groups over pairwise threads (§16.19), and messages that name what they answer (§16.14).** Two changes, one field family. First the reference: every message has carried `re_seq`/`re_own` since reactions, and the closed-world rule around them widens — a `TEXT` naming a message is a reply, a `PAYMENT_SENT` names the request it settles, a `RECEIPT` names the request it receipts, and everything outside the allow-list stays `MALFORMED`. The money half is the point: with no back-reference the only thread from a payment to its bill was the amount, so two identical requests answered by one payment both read as paid; the reference replaces that inference with a statement, still advisory — §17.5 verifies by finding the output; the reference settles *which* request the sender says it was for, a question the chain cannot answer. Nothing of the target travels: a reference is a sequence number, so a withdrawn message cannot be brought back by the reply that followed it, and a reader resolves against the thread it holds — where it cannot, it says so and renders the reply anyway. Then groups, as §17.9's roster pattern carrying words instead of DKG rounds: fan-out into existing pairwise threads, no group key, no shared record, no new network object naming the N — every thread property (forward secrecy per pair, prekey partitioning, deniability) unchanged because sealing is unchanged, at the stated cost of N−1 writes per message, which bounds the feature at *small* and is the shape rather than a limitation. Four fields (253–256): a group message is named by `(sender, group_seq)` because the fanned copies land at different pairwise seqs, so in-group targeting uses the group reference and a pairwise `re_seq` there is `MALFORMED`. The roster (kind 12, list in payload) is a **grow-only set** — anyone in the group adds, nobody is ever removed, and that trade is deliberate: removal needs a consensus a peer-to-peer group cannot have, while a grow-only set converges by union in any order with no governance at all; admission has the one tooth — a roster for a known group is accepted only from an existing member, so learning the id does not admit you. The mesh requirement (everyone holds everyone) is checked with no coordinator: contact edges are mutual, each end checks its own, and every local check passing *is* the mesh being complete — so receiving is never gated, sending is refused while the sender's own mesh is incomplete with the missing names said aloud, and partial delivery is structurally impossible rather than detected. Money stays pairwise. A client MUST disclose the shape plainly: trusted people, add-only, no history for newcomers (not policy — there is no store to have one), unforgeable member-to-member, and leaving is local. 328 vectors, both implementations agreeing.
- **0.89** — **Live position after the accept, built (§15.12).** The disclosure ladder's last rung — spec'd ahead of code in 0.88 — now exists. A `POSITION_REF` (kind 11, fields 218–219) is an ordinary sealed message handing over a DHT record and a fresh 32-byte stream key; both halves travel together or not at all, and the reference is a `PositionRef`'s whole content — refused on any other kind, and a position kind with no reference is refused too. The stream itself is not messages: it is one record overwritten in place, a *now* with no past, so a chat history cannot double as a movement log (§5.2.3's surveillance database rebuilt inside the E2EE is exactly what that shape refuses). Each update is a fixed-length frame — a monotonic counter, position in §15.12's 1e-7-degree integers, an optional heading, the capture time — padded to a constant 64 bytes and sealed XChaCha20-Poly1305 under the stream key with the **record key as associated data**, so a value lifted from one ride's record cannot authenticate in another's, which is what stops a fresh key from silently linking rides. Every frame is the same length with a heading or without, so the ciphertext sequence leaks its cadence and nothing else; the padding MUST be zero, closing a covert channel under the same key. A receiver drops a counter *lower* than the highest it has accepted (in-ride replay) but treats an *equal* one as the same frame read twice — the slot holds one value between writes, and calling that "no position" reports a working stream as dead; the frame's own capture time is what ages. It renders staleness as staleness rather than a guessed dot, lets go when the slot reads empty after at least one frame (the sender's own stop), and MUST NOT retain the counterparty's track past the ride. Two new vector kinds — `position.frame` pins the sealed frame's fixed length, its record binding and its range/padding refusals; the message shape rides `message.payment`. The gate that a reference MUST NOT precede a `RideAccept`, and the stop rules (receipt / `RETRACT re_own` / expiry), are the sender's and reader's — the decoder sees one message and cannot hold thread state. 314 vectors, both implementations agreeing.
- **0.89** — **A board stamp perishes with a Monero block, and costs memory instead of hashes (§16.18.1).** §16.18.1 priced a board write at 20 bits of SHA-256 and said a hundred cells cost an attacker a couple of hours. **Both halves of that were wrong in the attacker's favour.** SHA-256 is what commodity hardware is built to do: a GPU runs it some three orders of magnitude faster than one core, turning those hours into seconds, and rented mining hardware is beneath its own noise floor at 2²⁰ — a price that collapses by 10³ in the attacker's hands is not a price. And *nothing in the preimage was unpredictable*: cell, slot, body and signature are the poster's own and a board's generation is `floor(unix_seconds / 604800)`, so every stamp of the coming year was mineable in a single afternoon, and 0.88's weekly rotation — which rests on re-poisoning being paid for again each week — was not costing anybody anything. The work is now **Argon2id** (`m = 4096 KiB, t = 1, p = 1`, nonce as password, the notice folded to a 16-byte salt) at **8** bits, which leaves the honest cost where it was — measured at 0.7 s per notice and 91 s for a full cell on one core — while bounding the attacker by memory bandwidth rather than hash throughput. That is one or two orders back, not equality, and the document says so. The memory is 4 MiB rather than more because **the reader pays one evaluation per notice, honest or not**, and the reader is a phone; an implementation MUST check the signature *before* the work, so a slot of random bytes is refused for the price of an Ed25519 verify rather than a memory-hard one. A notice also now names the **Monero block it was stamped against** — `beacon_height` and `beacon_hash`, fields 249–252, both inside the signature — and a reader with a chain view refuses one more than **720 blocks** below its own tip or **2** above it. A day rather than an hour, because the binding constraint is a phone that has been in a drawer rather than the attacker; that still collapses precomputation from fifty-two weeks to one day. **Confirming that the height carries that hash is a MUST before display, not a SHOULD** — two-minute blocks make a height months away predictable to within a few hundred, so an attacker pre-mines a spread of future heights against hashes they invented and every reader running only the cheap comparison takes the lot, which is the whole of the precomputation back. It is also cheap and does not scale with the attack: `on_get_block_hash` is 119 bytes, an answer is good for ever including "that is not its hash", and a reader tracking the tip banks that block for free on every poll. Bulk-fetching the window is the wrong trade at 624 KB against an 86 KB ceiling nobody but an attacker approaches. So **three answers rather than two** — confirmed, refused, and not yet knowable — with the third held for the minutes until the tip catches up rather than shown, which is what stops the two blocks of forward slack becoming an exception to the MUST. Occupancy is the deliberate exception in the other direction: a poster deciding which slots are spoken for applies the window and not the confirmation, because overwriting a notice it merely cannot confirm *yet* would do the damage the rule exists to prevent. A reader with **no** chain view skips the test and judges the notice on its signature and its work, because reading a board has never required a Monero node and a marketplace that goes dark when a daemon is unreachable is a worse answer than the spam it was avoiding. Decoding consults neither chain nor clock: freshness is the caller's judgement with the caller's own view, pinned by `board.beacon_window` at both edges and from both sides. **Boards written before this draft are abandoned** — an old reader refuses the two new fields as unknown and a new one refuses their absence, both failing closed, which for a mechanism whose whole argument is that there is no unstamped path is the correct direction. 314 vectors, both implementations agreeing.
- **0.88** — **A board name names a generation, so a griefed cell is abandoned rather than lost (§15.12).** §15.12 has always said a stand is public to write and therefore grief-able, and treated that as the price of no operator: notices are tiny and expiring, value flees into sealed threads, and the answer to a wiped notice is to post again. **That answer quietly stopped being available.** A Veilid subkey accepts an inbound write whenever its sequence is merely *greater* than the stored one rather than exactly one past it, and `ValueSeqNum::next()` fails at `u32::MAX - 1` — so one write per slot at the maximum leaves a board unwritable by anyone, for ever, and the record key is a pure function of the name, so the cell has nowhere to go. 128 writes would end a neighbourhood's boards permanently, from anywhere, with no repair short of abandoning every board globally. Boards now rotate weekly: `<name>@<epoch>-<shard>`, `epoch = floor(unix_seconds / 604800)`, stamped before the shard suffix, pinned by vector (`stand.epoch`), and re-stamping an already-stamped name is `MALFORMED` because it would compute a board nobody else does and move a poster off its own notice. Writers and readers stamp at the moment of use and keep raw cells in state, so a reader follows a rollover with no rollover logic; a poster on a stale board re-posts at once instead of holding a tenancy nobody reads; and an implementation refuses a name that carries no generation, because a site that forgot to stamp would read a board nobody else computes and look like a quiet network. **This is not a defence and must not be read as one** — re-poisoning costs the same 128 writes a week, and a determined attacker keeps a cell dark as long as they pay. What it removes is the ratchet: damage now lasts while somebody maintains it instead of accumulating for ever. Costs accepted: clock skew splits a rollover for minutes out of a week, a live notice loses one poll of visibility at the boundary, and boards deployed before this are abandoned — which also heals any already poisoned. 262 vectors, both implementations agreeing.
- **0.88** — **A claim is once because a reader counts the writes, not because the schema forbids a second one (§16.12, §15.12).** This section has said since 0.65 that single use is "enforced by the shape of the record — the inbox has exactly one reply subkey, so a second claimant has nowhere to write". **That argument was false, and the implementations inherited it.** `SMPL(1, [writer])` bounds how many *subkeys* a member may write, not how many *times*; a subkey is a mutable slot and a later write replaces an earlier one. Handed over privately the capability goes only to the person you handed it to, so nothing was lost there — but a **hail or a listing publishes the card URI with its writer secret in it**, which means every reader of the board could overwrite whoever answered and be adopted as the counterparty, `payto` included. Two reader rules replace the missing schema guarantee: a claimant MUST read the reply subkey first and refuse a card that already holds one (this is what makes a single write the only honest history the slot has), and an issuer MUST treat a subkey whose **sequence shows more than one write** as contested, adopt nobody, and discard the card. Discard rather than resolve: nothing in the record says which writer was the person in front of you, and a contested card costs only a fresh card — an attacker who could reach the board could always have claimed first, and it is *overwriting* that had to go. Verified against the live network rather than reasoned about: one write leaves sequence 0 and an overwrite leaves 1 (`mobile/examples/seqtest.rs`), and a real card contested from its own public URI (`mobile/examples/cardattack.rs`) was discarded unclaimed by the phone that issued it. No wire change; 257 vectors unchanged.
- **0.88** — **The listing (§16.18), and renting as an operating mode (§15.11).** Discovery for the Airbnb/Turo shape, built on the boards the hail already proved: a `RENTAL_NOTICE` (fields 220–239) carries a claim-once card, what the thing is, roughly where, what it costs and what each side stakes. Three rules do the work. The board is **coarser than a hail's** — precision 5 (~5 km), not 6 — because a listing outlives the day it was posted and a home does not move, and because nobody searches a square kilometre for a car to rent. **A place has no gearbox and a car has no bedrooms**: the mismatched half is `MALFORMED` rather than reconciled, so a reader never guesses which fields it may believe. And what is on the board is what a stranger needs to *decide*, never what they need to *arrive* — no address, no plate, no photographs; those pass through the sealed thread once both sides have agreed, because an advertisement everyone can read must not double as a burglary brief. Claiming the card opens an ordinary thread and the deal is §15.12's reservation escrow unchanged. 255 vectors, both implementations agreeing.
- **0.88** — **The two-sided ride, proven between two real clients (§15.12).** The 2-of-2 rung had never run end to end between two independent clients — only its release primitive had. It has now, twice over, from a script that can be restarted at any point in it: two headless desks over live Veilid and live stagenet derived one escrow (56RCwMGC…), the rider paid fare plus stake (0.0006, txid e364341c…), the driver staked (0.0002, txid f559e4d8…), each confirmed the pot by its *own* scan, and the co-signed release (txid 8615c2b2…, mined 2187858) spent **two inputs to two outputs**. The money landed where the promise says: the rider received 0.000100 — their own stake exactly — and the driver 0.000518, the fare plus their own stake less the 0.000182 fee, summing to the 0.000800 the escrow held. Four defects surfaced that no reading had: a client that polled mail but never scanned its wallet, an escrow that could be paid into twice, a payer waiting on a field written only on the *other* device (it waits on the chain now, as "secured" already did), and a restarted proposer whose in-memory FROST state was gone — which is why a fresh proposal supersedes, and why both sides now simply ask again until the money moves.
- **0.88** — **Stakes get a number, a reason, and words a person can act on (§15.12).** The 2-of-2 rung is symmetric by default now: each side stakes a percentage of the price and gets it back on completion, which is one sentence a user can hold — *you both put up a stake, and finishing gives it back*. Suggested: 10% a ride, 20% a stay, 30% a vehicle, scaled by how far an asset's value exceeds its rental price. The reasoning is written down rather than asserted: Bisq is the closest working precedent (2-of-2, no custodian, both sides deposit, 15% floor and 50% ceiling, chosen to make cooperation likely without a reputation system's privacy cost), the dual-deposit literature proves cheat-proofness at equilibrium but derives no optimum, and the ceiling comes from the rental industry's decade-long retreat from deposits that price people out. Two bounds are normative-shaped: a stake below about twice the release fee should be zero rather than decoration, and none should exceed half the price. The previous draft's argument for a driver staking nothing — that it gatekeeps those who start with nothing — is preserved as the reason zero must remain buildable, not deleted because the default changed. Onboarding explains it before the first deal, because "what stops the other person from cheating me" is the question a marketplace without a company has to answer out loud.
- **0.88** — **The reservation (§15.12): rent and two deposits in one escrow, acceptance as funding, checkout as a split.** The guest initiates with three numbers — rent, their deposit, the host's — and the ceremony frame names all three, so the host's phone states exactly what accepting costs; the host's acceptance IS funding their deposit, because consent lives where money moves and a host who never funds has simply declined. "Secured" is rent plus both deposits by each side's own scan; the default checkout sends the guest's deposit home and the rent plus host deposit to the host's published address, so an under-funded host shorts only themselves; settlement, counters and rulings are the ride's machinery verbatim. Proven on stagenet: a 2-of-2 funded by TWO transactions (guest 0.0007, host 0.0002 — txids 4d6de9d8…, bef0d57c…) and released in one two-input FROST split (txid 8ccf79ab…) — 0.0002 to the deposit's refund address, the residual to the payee — the first multi-input multisig release on the record. The honest sequencing cost was stated when this shipped: whoever funds first is exposed until the other side funds. **Answered in the same draft** — the exposed side funds second now (§15.12), so the host's stake lands before the guest pays, and at reservation sums an arbiter is still the better rung when one is available.
- **0.88** — **The ruling, on-chain (§9.3, §15.12): the arbiter's co-signature moved money past an absent principal.** The dispute path is the settlement path with a different audience — the stranded principal sends the identical release proposal (FROST round 0, claimed split aboard) to the arbiter instead of the vanished counterparty, and the arbiter's co-signature is the ruling; declining is never signing. Proven on stagenet through the shipping engine: a 2-of-3 built by all three parties (rider/driver/arbiter, one address each derived), funded 0.001 XMR, and released by **driver + arbiter with the rider absent** — txid ec401a91…, 0.0008 to the driver. The destinations cannot change with the audience: an asking rider still routes the driver's slice only to the driver's published address, so a captured arbiter can at worst pick a split between the named parties, never a beneficiary. The phone side is one button — "Ask the arbiter to rule", offered to a stranded proposer and to a funded rider whose driver never completes — and the desk arbiter grew a ruling console: parked proposals print as requests, approval is a line a human writes, the judgment deliberately unautomated. The engine's ten-block maturity message paced the whole proof — "needs 9… 8… 2… RULED".
- **0.88** — **Settlement: proposals until a signature (§15.12, §17.9, §16.13).** Either principal may propose a split of a ride escrow — one number, what the funder gets back — as a fresh FROST round 0 that supersedes whatever stood, including the proposer's own; the counterparty's screen states the claimed split and offers two moves, sign or counter; whoever signs ends it, and the burn is only what remains if nobody ever does. A rider proposing routes the payee's slice exclusively to the address the driver published in the handshake — no proposer aims the other side's money. To carry the statement beside the signature, `FROST_ROUND` round 0 MAY now bear an amount (the claimed funder-return) — the one ceremony kind that may; the second implementation caught the rule drift exactly as O21 intends (it rejected the new vector until the spec said what the reference did), and both implementations now agree on 255 vectors.
- **0.88** — **The split release, and the escrow ladder it unlocks (§15.12, §17.9).** One FROST-signed transaction, several destinations: fixed slices to named addresses, the residual minus the true fee to the payee. Proven on stagenet — a 2-of-2 escrow funded 0.0012 XMR released in one transaction paying 0.0002 to a distinct refund subaddress and 0.00087 to the payee (txid de818596…), both outputs verified by scan. This is the primitive under every rung of the ladder: with no arbiter shared, the accept now builds a **2-of-2 on mutual stakes** — rider funds fare + margin, the release splits the margin home, sulking beats nobody and extortion burns the extorter's own fare; the driver stakes no capital because their skin is the fare, and requiring a deposit would gatekeep the unbanked. The ceremony frame carries a fresh per-ride refund subaddress from birth. Found and fixed along the way: Monero's ten-block maturity surfacing as an unexplained daemon `invalid_input` — five rejected releases that were nothing but a 7-of-10-confirmations wait; the engine now refuses with "needs N more confirmation(s)" instead; a repeat thread's stale kind-7 confirming a fresh offer (answers must postdate the offer now); and the broadcast error path finally names the daemon's reason.
- **0.88** — **The bonded hail (§15.12): escrow at the accept, and the ride that proved it.** §15.12's "unbonded hail is a mutual promise" now has its other half — with an arbiter contact configured, the rider's accept opens a §17.9 2-of-3 DKG with the driver and arbiter, and the fare goes into the address all three independently derive. The ceremony's round-0 frame grew a self-description (kind, funder index, fare) so the escrow names its own amount and the joiner checks it against the accept's echo; ride ceremonies gate the FROST release behind the funder's explicit yes — §15.5's confirm surviving into escrow — while plain 2-of-2 deposit returns keep the proven auto-co-sign. "Fare secured" is each side's own scan of the escrow from the build height (§17.5), not a message. The arbiter builds and then holds silence; disputes are §9.3's machinery, unchanged. Proven end to end on stagenet with two phones and a headless desk arbiter: hail posted to a board, claimed, offered, accepted; three parties derived one escrow (58MGveRH…ipB9g) in ~2 minutes over sealed threads; the rider funded 0.002 XMR; the driver's scan flipped its banner to "fare secured". Two live finds along the way: concurrent mail polls double-joining a ceremony (rounds are serialized now — the racing sends cost a stranded escrow, 0.006 XMR of stagenet tuition), and a repeat thread's stale kind-6 being accepted over the fresh one (newest wins now).
- **0.88** — **Live position after the accept (§15.12), specified before it is built.** §5.2.3's disclosure ladder always ended at *"during service: live position, over E2EE"*; this writes that rung's mechanics, spec ahead of code by intent. The gate is the accept ceremony: no reference before a `RIDE_ACCEPT`, consent per ride per direction, off by default, and MUST NOT be a standing setting — a toggle that shares on every future ride converts one moment's consent into a policy nobody remembers choosing. The stream is a record, not messages (kind 11 `POSITION_REF` carries record key 218 + stream key 219, sealed into the thread once): each update overwrites one subkey — a *now* with no past by construction — sealed XChaCha20-Poly1305 under the stream key with the record key as AAD, monotonic counter against in-ride replay, fixed-size padding and fixed cadence so the ciphertext sequence leaks liveness and nothing else. Bounded twice: clients stop at receipt, `RETRACT re_own`, or expiry; the record's TTL forgets even if a client does not. The receiver MUST NOT retain the counterparty's track — archiving a peer's movements rebuilds pairwise what §5.2.3 refused publicly. Position stays display-only input: it MAY prompt, it MUST NOT transact.
- **0.88** — **The handshake says what it is for (§16.9): `purpose` on `CONTACT_ACCEPT`, and the profile scoped to it.** §15.12 already ruled that a plate travels only on a driving claim; the audit behind this change found email, phone and signal — the same class of reach-me identifier — riding *every* handshake in both directions: a till issuing a `sale` card published its owner's Signal handle to every customer, and each customer's claim sent theirs straight back. One optional text field (217) closes it: the issuer stamps what the handshake is for (`profile`, `sale`, `hail`, `intro`, …), the claimant reads it and scopes its reply, and reach-me identifiers SHOULD travel only on `profile` — a deliberate contact exchange. An absent purpose is read as the private default, so older cards fail closed. The field carries no authority and changes no protocol behaviour; a claimant that ignores it overshares only its own data. Name, avatar and pronouns keep riding wherever the share switch allows (recognition locates nobody off the app); the payout address keeps its own opt-in (182); the car keeps the driving gate (210–212). Verified end-to-end through the production path: a `profile` handshake carries the identifiers, a `sale` carries none, a hail claim carries the car and nothing else, and the purpose survives the wire both ways.
- **0.88** — **The bond ceremony DUCAT owns (§17.9): interactive DKG and FROST over the sealed thread.** §8.2's audit found the seam — `dkg` 0.6.1 ships the threshold *rounds* but no way to run them between distrusting parties — and this fills it with the channel already built for two committed parties: §16.12's mailbox. Three message kinds (`DKG_ROUND` 8, `FROST_ROUND` 9, `CEREMONY_ABORT` 10) carry opaque threshold-library bytes (field 214) tagged by round (215) and bound to a per-escrow `ceremony_id` (216); DUCAT does not parse the payload, the library validates it, a malformed one aborts rather than corrupts. The 2-of-2 key builds in four sealed messages (commit, share, both ways), off any critical path, retryable — §17.1's calm onboarding made real; the release is a three-step FROST signature whose destination a signer MUST verify before co-signing, which is how §15.5's WYSIWYS survives into escrow (a co-signature is consent to a *destination*, and a `RULING` is itself that co-signature). The load-bearing crypto is proven live: `mobile/examples/escrowtest.rs` builds a real 2-of-2 wallet on stagenet and releases it by FROST, both signers exchanging only the serialized wire bytes this section seals and asserting one identical transaction. What remains unsolved is named: the arbiter set must be online to build and to rule (§17.8 governance), and O22's lost-device hole stands. No vectors — the ceremony is opaque bytes over the existing message, whose encoding is already covered.
- **0.87** — **The offer/accept ceremony (§15.12), and the retract that bills needed too (§16.13).** Claiming a hail used to *be* the deal — commitment at the wrong moment for both parties. Three new message kinds move it to where people actually decide: the claim opens a sealed channel (applying, not winning); the driver's first word is a `RIDE_OFFER` (6) carrying the fare (MUST) and optionally `eta_secs` (213, bounded at a day, meaningful nowhere else); the rider answers with `RIDE_ACCEPT` (7), which MUST name the offer and MUST echo its fare — acceptance bound to a number both parties said, verified by the reader, so two offers in a thread cannot leave the price ambiguous. `RETRACT` (5) is the ceremony's no and turned out to be the missing half of §16.13: with `re_own` a sender withdraws their own earlier message — a vendor cancelling a bill kills the live "Review payment" button on the other phone, previously an open invitation to pay into a sale nobody was watching — and without it, declines the counterparty's. A retract carries no amount and no bill; none of the three itemise (the ride's bill still arrives through §15.11's meter, answerable to the accepted fare). All three are ordinary sealed messages — chained, sequenced, advisory, no authority and no new delivery machinery. 240 vectors, both implementations agreeing.
- **0.86** — **A one-time prekey id is offered to at most one counterparty (§16.11).** The review that closed 0.85 left one protocol defect standing, and this drafts it away: bundles travel in per-thread log heads, but one *global* bundle published to every head let two counterparties cache the same copy and seal to the same one-time key — the first message in burned it, and the second arrived permanently unreadable, with nobody misbehaving. A one-contact field test can never surface this; the second chatty contact is the trigger. The rule joins §16.11's table: partition the *offering*, never the secrets — ids stay globally unique on the device, each thread's head advertises its own disjoint batch, and a fresh batch replaces a thread's offer wholesale (senders on stale heads still open, because the old ids' secrets survive until consumed). No wire change: the bundle format, the burn, the pen and the sweep are untouched; what changed is which keys a head is allowed to offer.
- **0.85** — **The overflow ladder (§15.12): capacity that costs what it uses.** A stand is 8 slots, and 8 is a neighbourhood, not a Friday night. Instead of bigger boards — which every quiet cell would pay for — a stand grows by **shards**: shard 0 is the bare name (deployed boards stay valid), overflow shards are `<name>-<n>`, decimal, no padding, capped at 16 — because a padded and an unpadded spelling are two different record keys for one name, and past 128 concurrent notices density has outgrown the cell and the answer is a finer geohash. The two rules that make cost track demand: **writers backfill low** (the lowest shard with a free slot takes the notice), and **readers sweep from shard 0, stopping at the first shard holding nothing live** — so a quiet cell costs one read, a busy one its actual height, and the ladder's height is itself a live congestion signal no operator publishes. The derivation machinery is untouched: a shard is just a name. Also recorded here because the field taught it twice in one night: **a subkey write is silently refused unless the writer's store knows the slot's current value sequence** — so read-before-write on any slot that may have a tenant is a correctness rule of §16.12 and §15.12 both, "delivered" is a claim about the network to be confirmed by reading the bytes back, and inspect-scope sequence comparisons that pass vacuously on unset local state are not confirmation. 226 vectors, both implementations agreeing.
- **0.84** — **The driver has a car (§16.9 fields 210–212), because dispatch matches strangers.** A profile grows `car_model` (≤24 chars), `car_color` (≤16) and `plate` (≤12) — short plain text, control characters `MALFORMED`, validated on the wire like every §16.9 field because they render as identity beside a name on a stranger's screen. They ride `CONTACT_ACCEPT`, which means the machinery already delivers them at exactly the right moment: a driver *claiming* a hail writes their details into the card's inbox, so the rider holds name, car, colour and plate the instant the match exists — before anyone has spoken. They are claims like the rest of the profile, and the spec says where the verification lives: the plate on the screen against the plate on the bumper, a check only the rider standing at the curb can run. 221 vectors, both implementations agreeing.
- **0.83** — **Geocells (§15.12, §16.17): the map becomes the name space.** A geohash's defining property — every prefix is a containing cell — makes `geo:<geohash>` a stand name at a stated coarseness, and the whole dispatch machinery from 0.82 carries over untouched: same derivation, same board, same claim race. What is pinned: truncate-never-round, integer encoding with floor midpoints (both implementations agree on boundaries, validated against the classic published geohash answers), drivers watch the 3×3 neighbourhood because a rider fifty metres over a border is otherwise invisible, and **precision 6 (~1.2 km) is the cap on any public surface** — `MALFORMED` above it, so "no precise location on a board" is construction, not manners. `HAIL_NOTICE` grows optional `origin_cell`/`dest_cell` (208–209): the Uber-shaped triage fields, a driver reading the job before claiming. The fare estimate is deliberately ordinary — base + per-distance + per-time, great-circle × 1.3 circuity, snapshotted to piconero at post exactly as §15.11 snapshots a meter — and seeds an *offer*, because there is no surge without someone to decree it: the thread's counter-quote is price discovery, not price setting. 218 vectors, both implementations agreeing.
- **0.82** — **Hail (§15.12, §16.17): dispatch without a dispatcher, with the crux proven before it was specified.** A rider and a driver who have never met converge on a DHT record with nothing in common but where they are, because Veilid computes record keys locally from owner public keys — so a keypair derived from a public string (`SHA-256("DUCAT-STAND-v0" ‖ cell)`) makes the DHT a map from *names* to *bulletin boards*, and a geohash or "the taxi rank at the airport" is a name. Both halves of the derivation are pinned, because the second was learned by running rather than reading: veilid encrypts record values under a key that rides the record-key handle and `create` always draws a random one, so a public board derives its encryption key from the cell name too, or readers compute the right record and cannot decrypt it. Demonstrated cold over the live network, cross-process (`research/dispatch/REPORT.md`). The board is stated as what it is — public seed, public secret, **anyone can write or wipe it**, a bulletin board in the literal sense — and survives the way real ones do: notices tiny and expiring, value fleeing immediately to sealed threads, the claim race settled by the DHT's claim-once inbox rather than any matchmaker. The `HAIL_NOTICE` (fields 203–207) is the first object on a public surface and is sized for hostility: a card, ≤64 bytes of destination, an optional fare offer, an expiry — no coordinates *by construction*. Two rules with teeth: **position MAY trigger the request and MUST NOT trigger the payment** (coordinates are spoofable input; §15.5's confirm survives dispatch), and an **unbonded hail is a mutual promise, like flagging a cab** — stated in the UI, deposits as plain first payments, driver bonds via Part IV named as the future answer, and any dispute mechanism that reintroduces an operator refused. Hail imports no route blobs from strangers, which is O17's containment intact. 214 vectors.
- **0.81** — **The burn gets a grace window (§16.11), because the bundle travels through an eventually-consistent store.** Delete-on-use assumed the sender and receiver agree on which keys are live. They cannot: the published bundle rides the log head, a DHT record, and a sender's fetch was observed trailing the receiver's republish — so a sender seals to a key the receiver burned seconds ago through no fault of its own, and immediate deletion converts that race into a message unreadable by anyone, ever. Worse, a reader that refuses to step past an unreadable message (the chain rule taken literally) freezes the whole thread on it — observed in the field: one lost prekey, forty minutes of a live conversation stuck behind one dead ciphertext. Two amendments. **Consumption still withdraws the key from the published bundle immediately**, but the secret moves to a holding pen and MUST be deleted after a bounded grace window (RECOMMENDED: 30 minutes) rather than at once — within the window the forward-secrecy delete has not yet landed for those messages, stated rather than hidden, and the window buys tolerance of exactly the propagation lag the transport actually has. And **a message that cannot be opened MUST NOT block the log**: the reader records the loss in place — the same honesty §16.10 requires for a ring that lapped a reader — and the chain restarts at the next message, `prev` unverifiable across the gap and said so. A permanently silent thread is a worse outcome than a thread with one honest hole. 207 vectors, unchanged.
- **0.80** — **Stewardship of the transport (§18.7), stated as obligation rather than vibe.** DUCAT runs on a network built by people who explicitly refused to monetize one, and that is a dependency, not a coincidence: the transport's neutrality is part of this protocol's threat model. So it is now normative. DUCAT MUST NOT introduce protocol fees, node payment, or any mechanism that monetizes carriage; a client MUST run as a full participant, giving routing and storage back to the network it takes them from; records are created for live purposes only, an implementation MUST NOT rewrite records merely to extend their lifetime past use, and SHOULD delete its local record state once a purpose is spent — an answered handshake inbox, a fetched attachment, an expired sale card — stated honestly: deletion is local, the network reclaims its own copies by TTL, and the obligation is to stop being a long-lived origin for dead purposes, not to pretend a distributed store can be recalled. The implementation now does what the section says: a registry sweep drops spent cards and forgets their records, and a fetched attachment's record is forgotten the moment its bytes are safe. 207 vectors, unchanged.
- **0.79** — **Reactions (§16.14), attachments (§16.15), read receipts (§16.16) — and the measured limits that shaped all three.** Veilid's caps, from source rather than folklore: 32 KiB per subkey, 1 MiB per record, 1024 subkeys per schema — so pictures are genuinely feasible and video is a different design. **Reactions** are messages, not a side channel: kind `REACTION`, body is the emoji (≤16 chars), target named by sequence — the recipient's log by default, the sender's own under a presence flag — sealed, chained and sequenced like everything else, because a second delivery path is a second set of bugs. **Attachments** ride ordinary text messages by reference: the bytes sit in their own record as XChaCha20-Poly1305-sealed chunks, and the key, nonce, length, ciphertext hash and mime travel *inside the sealed message* — the record on the network is noise to everyone but the thread. Fetch, hash, then decrypt: network bytes never reach the AEAD without matching the promised hash. All six reference fields travel together or not at all; every subset is a trap. One record is the unit (≤1 MiB); a larger file is a different design, not a larger number. **Read receipts** cost nothing and default off: the watermark rides the log head, which is rewritten constantly anyway — no ring slot, no prekey, no chain entry — and §16.16's stance is that when a message was read is behavioural data, leaving the device by explicit opt-in, never by installing a chat app. A published watermark is the publisher's claim, rendered as one. And because receipts and reactions multiply message count, **the ring size now travels on the head** (2..=1024, eight encoded by omission): readers MUST take it from the head, never a constant — the mismatch failure is reading the wrong slot and refusing a valid thread. 207 vectors, both implementations agreeing; the reference's own vector runner had to learn that head cases can refuse, which is its own small lesson about who tests the tests. New field ids 192–202 registered.
- **0.78** — **The till may say *seen* two minutes before it may say *settled*.** §15.11's settlement section gains the mempool: an implementation MAY scan the transaction pool for a payment matching an outstanding bill and surface the sighting — and MUST present it as §17.5's *seen*, never as settlement, MUST NOT issue the receipt or release goods on sight alone (that is §8.6's bonded mode, which this is not), and SHOULD stop offering to cancel a bill once its payment is sighted, since money in flight has nowhere else to go. The library keeps single-transaction scanning private, so the implementation wraps each pool transaction in a synthetic block — minimal miner transaction, one hash, dummy RingCT index — and feeds the ordinary scanner, the construction O14 recorded as viable in 0.48; the dummy index poisons nothing because a pool hit is never spent, only pointed at, and the real output arrives through the block scanner with a real index when mined. What the customer experiences: their payment leaves, the till flips to "settling" in seconds, and the receipt follows the block — a two-minute wait narrated instead of a two-minute stare. 193 vectors, unchanged.
- **0.77** — **The backup carries the people (§4.3), and states what that costs forward secrecy.** The bundle restored money, identity and profile — and not one relationship: no contacts, no outbox owner keys, no prekey secrets, no chain counters. A restored user held their whole balance and could reach nobody, and the failure was measured in the field before it was specified — a reinstall stranded a live thread as readable-but-unwritable, and the payment sent to its address is in a wallet nobody can open. The bundle now carries **typed contacts** — persona, both outbox keys, our outbox's *owner keypair* (without which the log is exactly that stranding), their cached bundle, per-direction chain counters (off by one, the next message in that direction is refused as out of order — these are not metadata, they are whether the thread works) — plus the prekey store and next-id counter, and one **opaque `app_state`** blob for same-client continuity, deliberately untyped so the typed fields stay an honest list of what another implementation needs. **The forward-secrecy trade is stated rather than implied:** §16.11's property *is* delete-on-use, and a backup holding one-time secrets rewinds the delete to the moment the file was made, for anyone holding it. They are carried anyway — the alternative strands every message sealed to them precisely at the moment of device loss, and §4.3.4 already names the bundle a complete spending credential, so the marginal exposure rides a file that must already be guarded absolutely. Restore order is opaque-first, typed-overlay-second, so a foreign bundle still restores every relationship. 193 vectors, unchanged.
- **0.76** — **The tap exists (§18.7): a card over the antenna, and nothing else.** The AID has been pinned since 0.48; what was missing was the exchange behind it. It is two plain ISO 7816 verbs, chosen so an iOS reader can speak them with the system APIs as they stand (O19: iPhones read HCE, never emulate): `SELECT` the AID → response data is the payload length as two big-endian bytes, `6985` when the phone is present but offering nothing; then `READ BINARY` by offset until done, `6B00` past the end. The payload is the UTF-8 `ducat:` card URI and deliberately nothing more — tap is **presence-only**, proving two phones touched, and the profile, the bill and the receipt all ride the mailbox the card opens (§16.12), which is exactly why the counterparty being absent later costs nothing. What a phone serves is whatever its visible screen is offering — a till's sale card, a tab's handshake, the standing profile code as the fallback — snapshotted at `SELECT` so a reader walks offsets into one consistent value rather than whatever the screen swaps to mid-tap. Readers MUST also accept NDEF tags carrying `ducat:` or `monero:` URIs, which is §15.9's static world on the same antenna. Serving a card to a second reader is harmless by construction: cards are claim-once and the DHT refuses the second claim, not the tap. 193 vectors, unchanged.
- **0.75** — **A bill can be withdrawn, and cash is a settlement rather than an exception.** §15.11 gains the two exits every billed-but-unpaid state needs, found by holding a real unpayable tab: a payee MUST be able to **cancel a bill** and MUST tell the counterparty in the thread when it does — their client still holds an actionable request pointing at money nobody is watching for, and a cancellation they never hear about is a payment into the void. And a payee MUST be able to mark a bill **settled outside DUCAT** — cash across the bar, a card — because the fallback rails existing is half the design; the customer SHOULD still get a `RECEIPT`, which the wire already permits without a transaction, since their record ought not depend on which rail the money took. One consequence for implementations of §15.11's unified settlement: abandoning a billed sale MUST withdraw the bill, not just leave the screen — a settled record left watching is one a later unrelated payment of the same amount will match, firing a receipt into a dead sale's thread. 193 vectors, unchanged.
- **0.74** — **The tab talks, the tip fits, and a claim answers a card rather than "whoever showed up".** Three findings from running §15.11 against a real implementation, folded back in. **Interim notices become a SHOULD for the bar tab:** each line added goes to the customer as an ordinary message with the running total, because a tab that is silent until close is a bill that arrives as a surprise — the MUST NOT on per-line *requests* stands, five confirm screens for one evening being five where one was owed. **Tips:** a client presenting a request SHOULD show its amount as fixed with an optional *additive* tip, and MUST NOT offer to edit the amount itself — §16.13's no-authority rule is unchanged (declining is declining), but an edited amount is a payment nothing on the payee's side can match, and the honest way to pay a different amount is a different payment. A tipped payment arrives larger than the bill, so §15.11's settlement matching gains a first preference: **match the amounts nominated by the payer's own `PAYMENT_SENT` notices** in the thread (≥ the billed amount, after the bill went out), with exact-amount as fallback — still §17.5, the notice nominates and the chain confirms, and a notice with no matching output settles nothing. The receipt then covers what actually arrived, with the tip as a visible line item so the sum rule holds. And one correctness rule §16.9 needed stating: **a claim answers a specific card.** An issuer with several cards outstanding — a standing profile code and a till's handshake — MUST bind each claim to the card whose inbox it was written into, never to "the next contact to appear"; the implementation that watched for any new contact would have billed a bystander who scanned the profile code mid-sale. Implementation defects fixed under this rule's pressure: prekey batches now draw ids from a device-wide counter instead of every batch starting at 1, the signed prekey is reused on top-up instead of rotated as a side effect, and new material *merges* into the store — each of the old behaviours silently invalidated keys that peers' cached bundles still sealed to. 193 vectors, both implementations agreeing.
- **0.73** — **Operating modes (§15.11): the device takes a stance, and no new wire objects are needed to take it.** A till, a bar tab, a taxi meter and a donation box are not four features; they are four answers to §15.2's two questions — who presents, and who has authority over the amount — plus, for the tab, state that spans events. The section's central claim is negative and load-bearing: **every mode rides objects this protocol already has.** A tab is a conversation (§16.12) whose settlement is one itemised `PAYMENT_REQUEST`; a meter's legibility is carried by §16.13's line items, under the rule that **the bill must show the arithmetic** — "23 min × 0.0005 XMR/min", not a total to take on faith, and a metered rate MUST be disclosed in the thread *when the meter starts*, priced in piconero at start so the figure the rider agreed to is the figure the bill uses; a donation target is §15.9's `TapStatic` stance with its admitted limits restated, not a contact card, because it receives money and establishes no relationship. Settlement detection is stated with its seams showing: a payee matching an arriving output to a request by exact amount MUST match oldest-first among its own outstanding requests and MUST NOT acknowledge the same output twice — and the residual (two strangers paying identical totals in the same window) is named rather than waved off. A receipt (`RECEIPT`, 0.71) SHOULD follow observed settlement automatically in vendor modes: the party who benefits from the record arriving is the payer, and the payee is the only one who can issue it. 193 vectors, unchanged — the point.
- **0.72** — **A profile, and the rule that keeps it off the card.** A contact list of hex strings is a contact list nobody uses, so `ContactDetails` gains an optional profile: a small picture, an email, a phone number, a Signal username, and pronouns. **None of it rides the card**, and that is the design rather than an implementation detail — the card is a QR code held up across a counter, and a picture makes it unscannable. The card only has to get two people connected; everything else arrives on the record afterwards, which is also why a profile can change without reissuing anything. Every field is **validated on the wire, not at the screen**: these render as identity on a stranger's device, and a field nobody checks says whatever the sender wants — an email with control characters in it, a "phone number" that is a sentence. A phone is digits only, because one number has a dozen spellings and accepting all of them means two clients render it two ways and neither matches when somebody searches. An avatar is bounded and its format is checked by magic number, because these are attacker-supplied bytes handed to an image decoder on someone else's phone and a decoder should never have to guess what it was given. **Pronouns are a closed set**, since this is drawn beside a name on a stranger's screen and free text there is a place to put a message; the cost is stated rather than hidden — a closed list cannot express every pronoun anyone uses, and a client MUST render someone with none set exactly as it renders anyone else rather than substituting a guess. All of it travels in the encrypted backup, because a persona restored with the right money and no face is not the same person to anyone who knew them. 193 vectors, both implementations agreeing, and both new validators mutation-tested — removing either makes the second implementation disagree.
- **0.71** — **A bill says what the money is for, and a receipt is a claim only the payee can make.** §16.13 gave a payment message an amount and nothing else, which is enough to move money and not enough to sell anything: "0.352 XMR" is not a transaction anyone can check against what they bought. A payment message MAY now carry **line items** and an optional **tax**, under one rule that is the whole point — `sum(items) + tax` MUST equal the amount, or the message is `MALFORMED`. An itemisation nobody can check is worse than none, because a breakdown printed beside a total it disagrees with looks like a check that was performed. Absent means not itemised; a present-but-empty list is the same claim spelled twice and is refused, and tax with no items is refused because a split of a total the message never breaks down is a number the reader must take on faith. **There is deliberately no field for the network fee.** A Monero fee is paid by the sender to the network, not by the payer to the vendor, so a fee line inside a bill charges it twice — once in the total requested and again when the payer's wallet builds the transaction. What the transfer cost is known to the payer's own wallet, which is the only party that can state it truthfully. And a fourth kind: `RECEIPT`, issued by whoever **received** the money. It is a distinct claim and neither existing kind can make it — a vendor sending `PAYMENT_SENT` would be stating they sent money, and a second `PAYMENT_REQUEST` would be asking again. It carries the amount settled, the breakdown, and the transaction it acknowledges. Advisory like everything else here: §17.5 verifies a payment by finding the output, and a receipt is the vendor's account of what it was *for* — the chain records amounts and never reasons. 184 vectors, both implementations agreeing.
- **0.70** — **A wallet stores outputs; a person reads a history, and those are not the same list.** Scanning finds outputs, so the obvious screen shows outputs — and it is wrong three ways at once. Two outputs of one transaction are one event. **Change is not income:** spending a whole output returns the remainder to your own wallet, and shown unlabelled it is money arriving, usually the largest figure on the screen. And a send leaves *no local trace at all* — the only on-chain evidence is that one of your outputs stops being unspent. A real wallet rendered a receipt and its change as two deposits totalling more than it had ever held, with the payment between them missing. So §16.13 gains what turns the list back into a history: an implementation SHOULD record, for each output, **the transaction that created it**, and MAY then identify its own spends without any stored record — fetch that transaction, and if any key image it consumed is one of yours, you sent it, with `paid = inputs consumed − outputs returned − fee`. That reconstructs payments made before the wallet kept records, and it reconciles: the events sum to the balance, which is the only way a person can check that the history screen and the balance screen describe the same money. **A received payment still has no sender** — Monero does not carry one — so a `PAYMENT_SENT` notice naming its transaction is the *only* thing that can put a name on an arriving output, and an implementation MUST present that name as the sender's claim rather than as a finding: the money is verified by §17.5, the sender never is. Until a transaction has been read, an implementation MUST NOT state a direction it cannot support; "still checking" is the honest row, because the case it gets wrong is exactly the change output that looks like income. 170 vectors, both implementations agreeing.
- **0.69** — **Fees, limits, and the two settings a restore was silently changing.** The pay screen offered the *balance* as the maximum, which is how a wallet lets someone type a number it will then refuse — after they have decided. It now prices the fee first and offers what can actually be sent, in whichever unit the entry field is using, with a breakdown: amount, estimated fee, total, what is left, how many notes it consumes and roughly how long confirmation takes. The estimate is built from the real structure of a CLSAG/Bulletproof+ transaction — 576 bytes of signature per input plus key image and offsets, 72 per output, a logarithmic range proof, quantised up to the daemon's mask because a fee rounded *down* is a transaction that does not relay — and checked against a live daemon at 1470 bytes for one input and two outputs, which is what a real one measures. It is labelled an estimate everywhere it appears, because the exact figure is only known once decoys are chosen. **The backup now carries the profile name and the publish-address choice.** Neither is a credential and both change how a restored persona behaves: one hands out cards nobody recognises, and the other is a *privacy* setting where restoring the wrong way is a silent disclosure rather than a lost preference. Absence decodes as off — the safe direction and the original default — so a bundle written before these existed cannot turn publishing on. Onboarding asks for both, once, before there is anything at stake, rather than at the moment somebody is trying to pay, which is the worst time to be reading about linkability.
- **0.68** — **A contact may publish an address, and the cost is stated rather than hidden.** §16.13's per-request destination is the private answer and stays the preferred one: an address that exists only inside one request is never reused, so it links nothing. But it makes the ordinary case — sending a friend twenty — begin with "ask them to send a request first", which is a reasonable sentence to a protocol designer and an absurd one to everybody else. **A protocol nobody can use protects nobody.** So `ContactDetails` gains an optional `payto`, with two rules that keep it a choice rather than a default: publishing is optional and an implementation MUST let a contact decline, with absent as the way to decline and an empty string `MALFORMED`; and a newer per-request destination supersedes a stored one, so a contact who rotates addresses is not undone by a copy someone kept. An implementation SHOULD say once and plainly what publishing costs — a stored address is a reused address, and a reused address is a public ledger entry naming everyone who ever paid that person — and then let the person decide. Someone choosing convenience for themselves is entitled to; someone doing it without being told is not choosing. 170 vectors, both implementations agreeing.
- **0.67** — **A payment request names where to pay (§16.13), which is what makes paying a contact possible at all.** §16.12's contact record carries a persona, an outbox and prekeys — deliberately no Monero address, so until now there was nothing to pay *to*. Putting it in the request rather than the record is the better answer twice over: the payer needs nothing from a record that may be stale, and an address stored against a contact gets reused, which turns a public ledger into a list of everyone who has ever paid that person. A fresh address per request costs nothing and removes that. **Stated rather than glossed: this does not make the address trustworthy.** Nothing in DUCAT binds a Monero address to a persona, so a contact whose device is compromised can ask you to pay a stranger and the request will look genuine. The confirm screen MUST show the destination, and an implementation MUST NOT offer a one-tap pay from an arriving message — §15.5's screen is the only thing between a message and a spend, and a request that shortens the path to it is a request malware sends on your behalf. The destination sits inside the message, so §16.10's chain covers it and altering where a request points breaks the link. 168 vectors, both implementations agreeing.
- **0.66** — **Money in a conversation (§16.13), which is the case §16.12 was built for.** Asking a contact for twenty is not a tap: §15's flow assumes both parties are standing together, and the point of asking is that they might be asleep. So a request rides the same log as the text around it — sealed, chained, and waiting in a record until the other side looks. A message now carries a kind, and **text is encoded by omitting it**: an explicit zero is `MALFORMED`, because one meaning with two encodings is the thing §18.1 exists to prevent, and this is the third place that rule has earned its keep. **A request carries no authority whatsoever** and that is not a limitation to relax later — the payer still decides at §15.5's confirm screen, and a request that could move money is a request malware sends on your behalf. A `PAYMENT_SENT` notice is advisory for the same reason §17.5 already gives: a payment is verified by finding the output, never by believing a note from the party who benefits from being believed. Amounts are piconero and required for both payment kinds, refused on text — a payment with no amount is a screen with a blank where the number goes, and an amount nothing will honour is worse than none, because both render. The amount sits inside the message, so §16.10's chain covers it and altering a request breaks the link. What this buys: subscriptions, an invoice to someone offline, a receipt after the fact, and splitting a bill among people who have already left — each needing a counterparty who is not there. 165 vectors, both implementations agreeing.
- **0.65** — **The contact card carries a DHT record key instead of a route blob, and the claim mechanism is gone because Veilid enforces it better.** §16.12 established the delivery model; this is the card catching up. A card now names an **inbox** — `SMPL(1, [writer])`, the issuer writing subkey 0 and the card holder writing subkey 1 — and both sides come away holding the other's outbox key and prekeys, having never been online at the same instant. **`claim_commit`/`claim_secret` are deleted, not ported.** They existed to prove a claimant held the card; a writer keypair proves the same thing and the proof is enforced by the DHT rather than by a check this document defines and an implementation could get wrong. Single use likewise stops being bookkeeping: the inbox has exactly one reply subkey, so a second claimant has nowhere to write. Fields **147–156 are burned rather than reused** — an old card decoding as a new one under different meanings is precisely the divergence §18.4.2 exists to prevent, and reuse would make it happen silently. The log is a **ring**: subkey 0 is a head carrying `next_seq`, messages occupy the rest modulo the slot count, and `still_in_ring` lets a reader who was away too long *know* it lost messages rather than display a thread with a hole in it (§16.10's "conversation that did not happen"). A ring rather than an archive is also what §16.11 wants — a message is meant to stop being readable, not accumulate. 160 vectors across five contact kinds, both implementations agreeing, and the ring maths is covered for the case that matters most: an off-by-one that lands a message on subkey 0 overwrites the head and loses the entire log rather than one entry.
- **0.64** — **Delivery moved from calls to records (§16.12), after the first build proved the point the hard way.** `app_call` is a remote procedure call and it was being used as a mailbox. Everything that went wrong followed from that: a private route dies with the process, so a contact card went stale the moment the app restarted, both parties had to be present in the same instant, and "they are offline" was indistinguishable from "the route died" and from "the card was already used" — all three arrive as a timeout. **VeilidChat does not use `app_call` at all**; their update handler logs and discards it, and nothing in the application ever allocates or exchanges a private route. Messages travel through DHT records, which is what makes their contacts survive a restart. DUCAT now does the same: a card carries a **record key**, which is permanent, rather than a route blob, which is a snapshot of a process. A sender appends to their own record whenever they are online and the reader collects whenever *they* are. **The reason this matters beyond chat** is that §15's tap is a presence protocol — correctly so — and asking a contact for money is not: the request must survive the recipient being asleep and the answer must survive the asker being asleep. Subscriptions and after-the-fact receipts need the same. A vendor is expected to be continuously reachable, but nothing may *depend* on it, because then a brief absence becomes indistinguishable from refusing to pay. Proven between two independent nodes — a record written by one whose node then shut down, opened and read by another in 702 ms and 330 ms. Named as unproven: **how long a record survives without its owner online**, which is a `veilid-core` replication property and the real ceiling on offline delivery, and until it is known a queued message MUST NOT be shown as delivered on the strength of a successful write.
- **0.63** — **Message encryption (§16.11), and the correction that HPKE alone is the wrong half of forward secrecy.** 0.62 named HPKE as the fix because that is what VeilidChat migrated to. HPKE **base mode** uses an ephemeral *sender* key against a **static receiver** key: it protects against compromise of the sender and not at all against compromise of the receiver, so seizing a phone and recovering one long-term X25519 key decrypts every message ever sent to it. For a document whose threat model is §2.2's endpoint compromise that is the direction that matters least, and adopting HPKE and stopping would have let this spec **claim forward secrecy while providing the wrong half of it**. The property needs the *receiver's* key to be gone, so receivers publish **one-time prekeys** and delete each on successful decryption — after which the ciphertext is undecryptable by anyone, its recipient included — with a rotating signed prekey as the exhaustion fallback. X3DH's structure, named rather than reinvented, and its known weakness inherited with it. Consumption **only on success**, because burning a key on a failed open lets anyone who can reach the rendezvous exhaust a recipient's supply with garbage and force them onto the weaker fallback, which is precisely the state an attacker wants them in. Duplicate prekey ids and a one-time key claiming reserved id 0 are `MALFORMED`, since "delete after use" must be unambiguous when it is the only thing the property rests on. **The suite was chosen for checkability**: RFC 9180's A.2 configuration — DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20Poly1305 — because A.2 publishes test vectors and **the vector set and the second implementation both share an author with the reference, which is why O21 is still open**. `core/tests/hpke.rs` reproduces the RFC's encapsulated key and ciphertext byte for byte, making this the **first externally-validated component in the project**. Stated rather than glossed: there is **no post-compromise security**, because there is no ratchet, and a Double Ratchet is not attempted here because it would turn §16.10's per-sender sequences into ratchet state and doing it badly is worse than not doing it. The harness demonstrates the claim from outside — a delivered ciphertext replayed after its prekey was consumed comes back `STATE_VIOLATION`, and the run exhausts its three one-time keys and falls back to the signed prekey with the downgrade printed rather than swallowed.
- **0.62** — **Chat folded into the protocol (§16.9, §16.10), and doing it exposed that §7.5's memos had no cross-implementation coverage at all.** DUCAT's contact machinery assumed §16.3's ordering — identity only *after* a receipt, bound by `H(RECEIPT) ‖ session_pk`. Paying a friend is a different relationship from paying a stall, so §16.9 adds the contact-first path: a card carried by NFC across a table, or as a `ducat:` URI through Signal. It is specified by what it **cannot** prove — key possession, yes; who handed it to you, no, because a card that arrived in a chat app was authenticated by *that app*. §15.9's lesson a third time. Invitations are **claim-once and expiring**, with the issuer storing only `H_chain(secret)`, so a screenshot in a group chat is not a standing offer to everyone who saw it and a stolen invitation list is not a set of usable claims; self-claim is refused so an interceptor cannot burn a card by claiming it back and leaving the intended recipient with one that silently fails. Donation QR codes on a website are **not** this — that is `TapStatic` (§15.9), reusable and public, and it keeps §15.9's admitted limit that a swapped tag verifies. Messages are 1:1 with **per-sender** sequences (a shared counter needs a round trip an offline sender does not have) and a `prev` link, so a message *removed and replaced* is caught where the sequence number still fits. Groups are out of scope and need no primitive: §15.2's `amount_authority: open` already splits a bill, one tap per person, which is how a table splits one anyway. **The finding:** `run_object` in the second implementation is generic over fields, so no vector had ever put text on the wire at the field level — §7.5's memo, added one draft earlier, was covered by *nothing*, and shipped accepting a present-but-empty string alongside an absent one. Two encodings of "no memo" makes `H(FullOffer)` depend on whether a client wrote `""` or nothing into a blank field, and the reference's own test asserted the wart was a feature. Empty text is now `MALFORMED` everywhere, and 18 vectors across three new kinds pin the text rules, the claim refusals, and the chain. 156/156, both implementations agreeing. Also fixed: `manifest_is_self_consistent` summed a **hardcoded list of vector files**, so `contact.json` was briefly uncounted — the precise staleness that test exists to catch, now derived from the manifest instead. Named honestly in §16.10: these messages are not forward-secret, and VeilidChat moved 1:1 conversation crypto to HPKE for exactly that reason.
- **0.61** — **§7.5 added: memos and petnames — and putting text on the wire exposed a divergence between the two implementations.** A receipt list reading `£4.20` twelve times records nothing; `£4.20 — coffee, Tuesday` does. `FullOffer` and `ACCEPT` each gain an optional 128-character memo, **both** rather than one, because a payee's *"consulting, March"* and a payer's *"reimbursed by work"* are different claims and neither may overwrite the other. Advisory only, signed and therefore covered by `offer_commit`, bounded in **characters rather than bytes** — a byte bound silently shortens every language that does not fit one character per byte — and never written to the chain, since a memo in `tx_extra` publishes to everyone what the protocol keeps between two people. Names are **petnames**: §15.9's lesson applies unchanged, in that a signature over a display name proves only that a keyholder chose that string, so names are self-asserted in the contact card, exchanged in §16.3's post-receipt coda, and stored and renameable locally by the receiver. No global registry, because a registry is a directory. **Nobody can be addressed by name** — reaching a person requires a prior exchange, which is also how VeilidChat works, by invitation rather than lookup. The divergence: §18.1 has required NFC-normalized text since 0.14, the second implementation enforced it, and **the reference decoder did not** — invisible because no object carried a string until now. Two encodings of "café" are two canonical objects and two hashes, which is §18.3's transcript-divergence bug arriving through a display field nobody thought was load-bearing. Fixed, and pinned by vectors so the suite can catch it next time.
- **0.60** — **Second audit pass; the checks grew and each new one caught something.** §18.12's first version verified that references *resolve* — it could not see a claim that had quietly stopped being true. Three checks added, three findings: O21's live text still said "104 vectors" against a set of 136 (changelog entries stay exempt, being history); `bond_proof` occupied fields 140–144 while §18.4.2 still read "140+ Unallocated", reintroducing the exact collision hazard the registry exists to prevent; and **§18.9.1's normative table listed ten vector kinds while the schema accepted sixteen** — every escrow and bond case was executable, published, and undiscoverable from the document. That last one names the pattern: **an artifact and its description drift toward the artifact**, because the artifact is what gets run, and only a mechanical comparison notices. Verified still true rather than assumed: §2.5's "no adversarial review whatsoever" (the user has deliberately deferred it), and suite 2's key agreement genuinely unimplemented — a suite named in the type system is not a suite that exists.
- **0.59** — **Arbitration, mandates, and static tags exercised — the last three objects nothing had ever run.** §9.3: a ruling from a named arbiter is accepted and one from an outsider refused (`UNTRUSTED_ARBITER_SET`, which is §2.5 in a single check), an award larger than the claim refused, an award attached to a ruling for the *losing* side refused, a ruling naming another dispute refused — and §9.3.4's expiry confirmed to emit a **real, co-signable ruling** rather than nothing, since "return to the pre-dispute allocation" *was* the deadlock it claimed to prevent. §7.3: mandate caps bind **cumulatively** rather than per draw, only the named payee may draw, expiry is enforced, and a fresh period resets the allowance — otherwise a monthly mandate is a one-off with extra steps. §15.9: static tags report `Anonymous` or `SignedBy`, refuse a bad signature, and — **demonstrated rather than described** — a swapped tag carries the attacker's persona with a perfectly valid signature over the attacker's own address and verifies cleanly. A signature there proves who owns an address; it never proves the tag is the one the venue put there, and a first-time donor has nothing to compare against.
- **0.58** — **The paths where nobody co-signs, exercised: abandonment, meters, refunds.** §6.2 calls post-`FUND`/pre-`RECEIPT` the dangerous window and no harness had ever entered it. All three behave: a payee that vanishes after funding leaves the payer holding **payment evidence** — a 166-byte receipt flagged `unilateral`, proving what was signed and paid and claiming nothing about delivery; an abandoned meter leaves the payee holding **debt evidence**, the opposite assertion, which is why §6.2 keeps them distinct and lets the state machine choose rather than the UI. Confirmed alongside: `METERING` survives an hour of wall clock (§18.4.1(8) — a tab that died after sixty seconds was a real bug), a payer cannot abort a live meter while an operator can void one cleanly (§18.4.1(7)), and a refund is refused when redirected, when larger than the payment, when the payer signed no address, and when late. **One finding from the fixture rather than the code:** the default refund window is **zero**, so a client shipping default terms has silently made every sale final. Defensible as a default and easy to ship unknowingly, so §7.3 now requires the window be shown on the confirm screen — "no refunds" is a term of the sale, not the absence of one.
- **0.57** — **Ten attacks fired down a live route, and one of them found a denial of service.** Every refusal in this protocol was unit-tested and none had ever been *sent* — a gap that is not academic, since the `dest` bug was a check that existed, was tested, and rejected every real payment because the fixtures agreed with the mistake. A hostile payer now runs underpayment, overpayment, offer substitution, a forged signature, a **cross-context signature** offered as an ACCEPT, an unknown field, a TXID out of state, a TXID for another transaction, an announced underpayment, and a TXID naming a transaction that does not exist. All ten are refused with the right codes by the same code path a real counterparty runs. **The last one exposed a real defect**: the payee scanned synchronously inside the request, so one message naming a nonexistent transaction froze the terminal for five minutes — a denial of service costing the attacker 40 bytes. §6.2 now requires that `TXID` and `RECEIPT` **not** be a single request and response: structural checks are cheap and synchronous, the payee acknowledges immediately, the scan runs off the session, and the receipt is collected separately. Generalised, because this is the third instance in two drafts: **nothing that waits on the world may hold a session open** — the same failure as a server that dies on malformed input, and invisible until somebody hostile sends one.
- **0.56** — **Both tap directions run end to end; the inverted one had never been built.** The payee-presented case — merchant presents, customer taps — is a POS terminal and is what every earlier harness exercised. The payer-presented case, where the customer shows a code and the till scans it, existed only as an enum variant, **despite §15.3.2 relying on it as the escape hatch for iOS merchants who cannot present over NFC (O19)**. An escape hatch the document depends on for a whole platform had no test and no implementation. Building it exposed an asymmetry `presenter_role` hides: the presenter supplies *reachability*, so the reader drives, and in the inverted direction the till must **poll** because the customer holds the route and cannot call out. Three rules follow — a presenter's message loop MUST stay responsive while settling (the first implementation paid inline through forty seconds of propagation retries and the till abandoned a sale for a transaction already broadcast); `amount_authority` MUST be `open`, since a customer's phone does not know the price and `offer_commit` is necessarily empty because the offer does not exist yet; and **the human checkpoint does not move** — `ACCEPT` remains the payer's alone, whoever held out a phone. A UI built on one direction will assume symmetry and be wrong.
- **0.55** — **The two remaining "unproven" claims tested, and the `dest` bug that had made a real payment impossible.** Escrow and `fast/1` contract logic is now **language-neutral**: 24 vectors covering ceremony ordering (§2.5's out-of-order and duplicate cases), participant agreement on a formed wallet, release destination constraints, bond freshness and ladder membership, and slash-claim cure windows — executed by `core` *and* by a second implementation written from this document, 136/136 in both. Encoding agreement was never the hard part; two clients can serialise identically and still decide differently, and these are the decisions money depends on. **Tap latency measured** over live routes: 34 / 221 / 297 ms to a confirm screen across three consecutive runs, well inside §15.3's three seconds — and the honest reading is the order-of-magnitude spread rather than the best case, on identical hardware with an attached node. It says the protocol is not the problem; it does not say the tap fits on a handset. **And the integration harness found what unit tests could not**: `TapPresent` and `Accept` both length-checked `dest` as 16 bytes, copied from the `nonce` read above them, so any object naming a real 95-character Monero address decoded as `MALFORMED`. Every test passed `None` or a placeholder — the fixtures agreed with the bug — and it surfaced the first time two processes exchanged a genuine address over a live route. Also recorded: Veilid delivery is not reliable, a lost `app_call` on a working route must be retried rather than read as a counterparty refusal, and the three-second budget must therefore cover at least one retry.
- **0.54** — **The document is now audited against the implementation (§18.12), and the first run found a normative section that did not exist.** §15.5.1 — payer verification — was announced in 0.41's changelog, implemented in `core/src/verify.rs`, tested, and cited from §4.3 and O10, but **never written**. An implementer following the reference found nothing. Nothing was inconsistent, no test failed, and the absence was invisible precisely because everything around it was correct; §15.5.1 is written now, with the tier ladder, the four rules a user may not relax, and the reason none of it touches the wire. `conformance/audit_spec.py` checks section and open-problem references, reject codes against `core::reject`, field-number collisions **within a namespace**, object type codes and labels, vector `kind` agreement across the schema and both runners, the draft version against the newest changelog entry, and the transport identifiers against `core::transport`. Its own first run is recorded too: eight reports, of which **three were checker bugs** — an RFC citation read as a DUCAT section, the field registry's table matched by a pattern meant for §18.5, and `Terms`' nested key space read as five collisions. One real defect against three false alarms is the normal shape of a first audit, and the discipline is fixing the checker rather than loosening it until it agrees.
- **0.53** — **Veilid #395 checked upstream, and the answer changes how it should be treated.** O17 said the fix was "still required"; the issue has in fact been **open since July 2024**, sits on milestone *Release 0.13.0 — Private Routing 2.0* **due 1 March 2027**, while `veilid-core` ships 0.5.7. A milestone named 2.0 two years out is a redesign, not a patch — so §5.2's inversion is not a stopgap pending an upstream fix, it is the answer, and **nothing further should be designed on the assumption that #395 lands.** Also recorded: O16's throughput figures were measured against `veilid-core` 0.5.3 and four releases have shipped since, so they are stale as a budget even though nothing suggests they are wrong.
- **0.52** — **O9 quantified and O13 stated as a trade rather than a worry.** O9 said hot-wallet exposure was "mitigated by keeping it small", which is true and hides the important half: **the float is bounded from below by usage.** §17.2 makes consecutive capacity a count of unlocked outputs and `sim --drain` measured ~1.5 consumed per payment, so *k* payments before a top-up costs about `1.5k × typical payment` of exposure and no risk preference can reduce it. `core::float` computes the number and reconciles a risk cap against a usage pattern, because the two are set in different places by different reasoning and otherwise contradict each other silently until the user is at a counter. O13 gains the resolution its flag implied: **the accountable relay and the private relay are not the same relay.** A staked `relay/1` announces "DUCAT user" to a few-hundred-person set; a public Monero node announces "Monero user" and is accountable to nobody. 0.51 measured both halves — a public node accepted two transactions and propagated neither, and the client caught it by asking *other public nodes* rather than by holding anyone accountable. So: submit through several public nodes by default, since redundancy is free and defeats a silent drop without accountability, and buy accountability only when routing around the failure is impossible or a dispute needs behaviour attributable to a stake. Neither problem closes — the float stays reachable, and using the relay set remains the signal however narrow the audience.
- **0.51** — **A 3-of-5 FROSTLASS group funded and spent on stagenet, and 0.50's DKG claim retracted.** Three of five signers produced a valid CLSAG in 0.088 s, mined at height 2,183,934 — settling the signing half of O1 for a threshold Monero's native multisig cannot express. 0.50 recorded that "`dkg` 0.6.1 ships no interactive DKG" and called it §8.2's largest gap; **that was wrong.** The DKG was split into its own crate: `dkg-pedpop` 0.6.0 implements PedPoP, three rounds with blame assignment, which is what mutually distrusting parties need. The real problem is narrower and more awkward — **it does not link with the wallet**: `dkg-pedpop` declares `multiexp` without the `batch` feature its own source uses, so it does not build standalone, and it pins `multiexp 0.4` while `modular-frost 0.11.1` (required by `monero-wallet` for multisig) pins 0.5, making their `BatchVerifier` types incompatible. The correction matters because the two situations demand opposite responses: designing and auditing a DKG is a research project; pinning a source and verifying a build is an afternoon. Also from the same run: **the send side of §8.7.2**, where one relay accepted two transactions with `Ok(())` and propagated neither, and where a propagation check built on a confirmed-only lookup reported a live mempool transaction as lost — a false negative indistinguishable from the true one without an independent query.
- **0.50** — **FROSTLASS measured, and §9.3 corrected by it.** §8.2 had been carrying FROSTLASS's claims on the strength of a README while every empirical result in the project was wallet2's. The decisive finding is not performance: **Monero's native multisig admits only N-of-N and (N−1)-of-N**, so 2-of-3 exists and **3-of-5 does not** — which means §9.3's "multiple arbiters can co-sign for higher-value escrows" was unbuildable on the tested path, and one of O22's candidate directions never existed. FROSTLASS forms 2-of-3, 3-of-5, 2-of-5 and 7-of-11 in under a second each, so the limitation is an implementation choice rather than a property of Monero. Shares are **linear in *n*, not fixed** — the spike's own first draft claimed fixed and its output refuted it — at 151 B for 2-of-3 against wallet2's 2,286-byte key file, which also removes §4.3.3's file-copy workaround. Two gaps surfaced from building it rather than reading about it: **`dkg` 0.6.1 ships no interactive DKG**, so a deployment must bring its own ceremony between distrusting parties and the audit does not cover that; and **the view key is separate shared material that MUST be fresh per group**, because the FROST group key has no private half to derive it from, and a client reusing one view key lets every member of one escrow watch every other.
- **0.49** — **O15's detection built, O10's side channel closed — and O15's own recommendation turned out to understate the fix.** The document said `pos/1` merchants should "detect duplicate one-time keys", which reads as a lookup. **The burning bug is arithmetic**: a merchant expecting 1 XMR receives two outputs of 0.5 to the same one-time key, `sum()` reports 1.0, the goods leave, and exactly one coin is spendable. So the rule is that **outputs sharing a one-time key count once at the maximum and are never summed** — `sum()` is the bug, and no amount of detecting-then-summing fixes it. `core::burning` also surfaces a burn that still covers the price, because a duplicate one-time key is not an accident and the merchant is entitled to know who they are dealing with. On the privacy side, `bond_proof`'s exact `capacity_remaining` was a running meter on a rider's spending readable by every provider they tapped; it becomes `capacity_bucket`, the floor of a fixed ladder, **rounding down always** — rounding to nearest would let a bond overstate solvency, and the beneficiary of that overstatement is the party publishing it. Ladder membership is enforced on receipt, since an arbitrary integer there would let a rider publish their balance exactly and call it a bucket. 64 bits of leak become under 4.1, paid for with an honest false negative: a rider just under a rung can pay and cannot prove it.
- **0.48** — **O20 closed and O14 halved, both by checking a claim instead of carrying it.** §18.7 said its NFC AID was "a placeholder pending real RID registration". **There was no registration to wait for**: ISO/IEC 7816-5 assigns the first nibble by category and reserves `0xF…` for proprietary identifiers requiring none, which is what Android HCE documents for precisely this case. The AID is now `F0 44 55 43 41 54` (`0xF0` ‖ `"DUCAT"`) — the four-character `"DCAT"` contraction had been chosen to fit a 5-byte minimum that was never a maximum, and with no registry to guarantee uniqueness the distinctive name is worth its one byte. BLE takes four random 128-bit UUIDs, which need no registration for a cleaner reason: the SIG registers only 16-bit UUIDs. The L2CAP PSM stays unassigned on purpose, since LE CoC PSMs are allocated by the local stack and a pinned value would be one the spec does not control. **O14's unconfirmed-scanning gap turned out to be smaller than recorded**: `scan_transaction` is indeed private, but `ScannableBlock` is re-exported with public fields, so a caller wraps a mempool transaction in a synthetic block and calls the public scanner — verified by compiling against the crate. Correctness on real mempool data is untested and now stated as the open part, which is a far more tractable problem than "the API does not permit this". The proof gap stands, narrowed to clients that embed a wallet rather than drive `monero-wallet-rpc`.
- **0.47** — **`fast/1` and escrow implemented — the two paths the manifest admitted had no coverage — and three defects surfaced doing it.** First, **`TXID` and `TXPROOF` had been conflated since 0.17**: that draft established the payee *is* the recipient and scans with its own view key, so acceptance needs a mempool pointer rather than a proof — but §6's message table, §6.2's deadlines, and §18.4's transition table all kept saying `TXPROOF` for thirty drafts. They are now two objects with two jobs: `TXID` on the happy path carrying no evidence at all, and `TXPROOF` only inside a slash claim, for an arbiter who cannot scan. Second, **five objects had improvised type codes and discarded the type field on decode** — `DISPUTE`, `RULING`, `HAIL`, `HAIL_REPLY`, `TapStatic`, every one added after the original four, each written from the last rather than from the correct originals. Two byte strings differing only in declared type decoded to the same object: §18.3's transcript-divergence bug exactly. Third, **the field registry's `96+ Unallocated` row was stale from 0.14**, with 96–103 long in use, so a second implementer allocating from 96 would have collided head-on. Escrow's ceremony is built as a `RoundTracker` that accepts only the round it expects and only one contribution per participant per round, because **§2.5's exploit was a forged out-of-order message overwriting settled state** — and `check_escrow_ready` takes the trusted arbiter set as an argument so an arbiter can never arrive in a message. Verified on stagenet: a **third wallet, neither payer nor payee, checked a real `OutProofV2`** (`good: true, received: 600000000`) and rejected the same proof under a different message (`good: false`), confirming both §17.5's premise and the new requirement that **a proof be bound to its transcript** — without which any proof the payer ever generated replays into an unrelated claim.
- **0.46** — **Vector schema published (§18.9.1), and the harness normalised rather than documented.** §18.11 recorded that most of the second implementation's effort went into the *test harness*, not the protocol: `signing.json` had two undeclared shapes, `negotiate.json` held two cases that were not negotiations, `transcript.json` held one that was not a transcript, and the state event grammar had **five spellings of one concept**. Writing a schema that documented five spellings would have formalised the mess, so the format was normalised first — **every case now carries a `kind`, and `kind` is the only discriminator; file names carry no meaning to a consumer** — and then specified in a normative `vectors/v1/schema.json`. **The schema is hand-written, not emitted by the generator**, because a schema produced by the same program that produces the vectors agrees with that program's mistakes, and it earned the distinction immediately: it caught a negotiation case that never declared which versions the local client supported (forcing consumers to invent a default, which is how two implementations diverge) and a `state.sequence` shape that let a case assert a transition without asserting the effect — which passes while a client emits the wrong evidence, and §6.2's two unilateral receipts assert opposite things. `why` is now required on every case and `hint` explicitly unparseable. Both implementations and the validator agree at 104/104. O21 remains open, and what is left cannot be engineered away: an implementer who has never read `core/`. Everything accidental is now out of their path.
- **0.45** — **A second implementation ran the vectors, and found three defects in this document (§18.11, O21 advanced).** `conformance/ducat_check.py` is written from Part V rather than from `core/`; it agreed on 101 of 104 cases. Two disagreements were plain omissions where the reference was right and the text was silent: **§18.1 had no nesting bound** — the word did not appear in the document, while the vector's own hint cited "the 16-level nesting bound" as though specified — and **§18.4's self-described exhaustive table listed `CLOSED`'s 120 s contact window only as a guard, never as a deadline**, which changes no state and therefore leaves a client holding session keys open forever. The third is why the exercise was worth doing: **negative integers were unspecified**, so the reference accepted CBOR major type 1 and the second implementation refused it and *both were conformant*. No vector set could have caught that, because there was no correct answer to test against — it is only visible when two implementations read the same text and reach different conclusions. Resolved toward refusal, with a rule for every future §18.1 addition: **later accepting a value type extends the format, later refusing one breaks every peer already relying on it, so strict first is the only reversible choice.** 104/104 after correction. O21 remains open for an honest reason — same author, not clean-room — and the cheapest step toward closing it is publishing a schema for the vector files, whose shape a second implementer currently reverse-engineers from examples.
- **0.44** — **O22 closed: an escrow share is recoverable after all, and the earlier reasoning was wrong in an instructive way.** 0.42 concluded shares could not be backed up because `monero-wallet-rpc` cannot restore one. Sound reasoning, wrong conclusion — it assumed restoring a share means *reconstructing* it. Measured both halves: reconstruction genuinely fails, since two wallets with byte-identical key material produce `prepare_multisig` outputs agreeing for 101 characters and then diverging for 88 of fresh randomness. But reconstruction is unnecessary. A share is already a **2,286-byte `.keys` file**, and copying it into a virgin directory yielded `multisig: true, ready: true, threshold 2, total 3` at the correct group address, producing valid `export_multisig_info` — the missing RPC method is simply not on the path. The multi-megabyte companion file is scan cache and must not be backed up. **What actually changed is the trust ask**: recovery used to require the *counterparty's signature*, an adversary being asked to sign away their own claim, which nothing can compel; it now requires the other participants to re-share multisig info, which endorses no outcome and is a routine mechanical step. Four rules from observation — capture at `ready` so a half-formed ceremony is not backed up as one, never back up the cache, re-export when an escrow opens since this is the bundle's only freshness-sensitive content, and note that restoring membership is not yet the ability to spend. Residue recorded rather than hidden: a stale bundle still misses escrows opened after it, and an end-to-end spend from a restored share is undemonstrated — the restored wallet refused to build one, and so did the original, for the same unrelated reason.
- **0.43** — **Added §4.4, custody modes — external hardware wallets, and the two limits that make them narrower than they sound.** "Hardware keys" names two opposite things: a secure element dies with the phone, which is what made §4.1 unrecoverable, while an external hardware wallet *is* its own backup. Supporting the second is worth a first-run choice. But a Monero hardware wallet **cannot hold the persona key** — it signs Monero transactions, not DUCAT's domain-separated objects — so no mode moves identity off the phone and §4.3 remains the only answer for it in every mode; and it **cannot hold a multisig share**, since Monero multisig on hardware is unshipped, which bars escrow and bonds. Three modes result, and the recommended one is the hybrid: float on the phone, reserve on the device. That is not generic defence in depth — it closes something §4.3 could not, because "a backup is a complete spending credential" becomes false when the reserve sits behind a device seed DUCAT never sees, making **the amount at risk a number the user chose** and bounding a stolen phone, a leaked backup, and a compromised client with one number. It needs no new machinery either, since §17.2 already models the float. Two rules earn their place from failure modes: the capability check happens **before an offer is presented**, not at FUND, because a hardware-only user discovering at a counter that escrow cannot fund has failed in front of a queue; and **role matters for `fast/1`**, where the bond is the provider's, so a hardware-only consumer can pay a bonded merchant and only the provider side is barred.
- **0.42** — **Added §4.3, encrypted persona backup, closing O12 — and reversed §4.1's persona storage rule to make it possible.** §4.1 recommended hardware-backing persona keys, and hardware backing means a key cannot be extracted; the recommendation therefore made every persona permanently unrecoverable on device loss, silently, and no backup mechanism could have rescued it. Keys are now split by **replaceability**: device keys stay hardware-backed because losing one costs a device (§4.2), persona keys become software and exportable because losing one costs an identity. Backup is one Argon2id + XChaCha20-Poly1305 file the user exports and keeps — not social recovery, not custody, nothing uploaded. Three contents earn their place by being invisible when missing: the **Monero restore height**, without which a restore rescans from genesis at a measured ~106 hours instead of 35 seconds and the user concludes their money is gone; **rendezvous keys** (§16.4), without which a restored persona can be paid but nobody it knows can reach it; and **attestation record writer keys** (§9.2), which are not derived from the persona key — omit them and reputation is readable but frozen at the moment the device died. KDF parameters and CBOR field numbers are pinned by format version and covered by a known-answer vector, because changing either derives a different key and turns every existing backup into a "wrong passphrase" error for people typing the correct one. Records are explicitly *not* in the bundle: receipts keep §7.4's separate export, since a credential backup from a year ago is still valid and a receipt archive from a year ago is not. **Multisig shares are not in it either, and that turned out to be the interesting part.** Measured: a multisig wallet has a seed (592 hex chars for a 2-of-3, not 25 words) and `monero-wallet-cli --restore-multisig-wallet` rebuilds the exact group address from it — but `monero-wallet-rpc`, the integration surface a phone client actually has, exposes no restore method at all (`-32601`). Bundling a share would advertise a recovery that does not exist. For bonds this costs nothing, since §17.2 already puts both non-user keys in the arbiter set. For escrow it is a hole with no clean answer: §8.2's 2-of-3 makes every buyer-favourable outcome need the *seller's* signature once the buyer's key is gone — including a `RULING` for the buyer, since a ruling **is** a co-signature — and §9.3.4's expiry rule cannot help, because it guarantees a ruling exists rather than two live keys to execute one. Raised as **O22**: the only place in DUCAT where a lost device loses money rather than convenience.
- **0.41** — **Added §15.5.1, payer verification — the question WYSIWYS never asked.** WYSIWYS proves a payer sees what they sign; nothing established that the person holding the device should be signing. A stolen unlocked phone was a valid payer. Adopts EMV's CVM *shape*: three tiers with user-set, value-scaled thresholds, where the gap between "device happens to be unlocked" (passive, a thief has it) and "secret entered in-app just now" (a knowledge factor they do not) is the load-bearing distinction. Four rules are not the user's to relax — thresholds must ascend, in-app secrets expire, velocity is checked alongside per-payment value, and **a stale exchange rate escalates rather than relaxes**, since failing the other way would let anyone who can stall a rate feed lower the security requirement. None of it touches the wire: a payee that could influence verification would ask for the weakest.
- **0.40** — **A refund had nowhere to go.** Nothing in the transcript carried the payer's address, so a merchant willing to refund had to obtain one out of band — the exact ambiguity a published attack on BIP-70 exploited by substituting the destination. `ACCEPT` now carries an optional signed `refund_to` and a refund must have gone there. Optional deliberately: supplying it lets the payee learn a payer address even when no refund happens, so omitting it is a legitimate choice of unlinkability over refundability, and clients must present it as a trade rather than filling it silently.
- **0.39** — **Timeout audit: `FUNDED` and `PROVISIONAL` were dead ends holding the payer's money.** Applying cycle 9's rule — *nothing happens is not a safe default* — across §6.2 found that neither state had a deadline in any settlement mode. "Profile-defined" is a deferral, not an action, so a payer who funded and never received `PROOF` waited forever with the money gone and no evidence. Added `DeliveryWindowExpired` as a backstop closing to payment evidence, and the tested invariant that **no state holding committed funds may be a dead end** — every such state now has an exit reachable without the counterparty's cooperation, since a counterparty holding your money is precisely the one who may not cooperate. Also recorded: the 60-second `FUND` deadline is shorter than a Monero block and is therefore meaningful only against mempool visibility, and `TXPROOF` expiry named two actions where the machine can only take one.
- **0.38** — **The single-sided receipt was one effect covering two opposite claims.** A `DELIVERED` timeout means *the payer paid and holds no co-signature*; an abandoned meter means *the payee is owed and the customer left*. Both emitted the same instruction, so a client had to infer direction from the state it had just left — putting the decision back where a state machine exists to remove it from. Now split into payment evidence and debt evidence. Conflating them would have a payer's client record a debt it does not owe, or a merchant file a payment it never received.
- **0.37** — **`CANCEL` and `TapStatic` implemented; §15.9's persona pinning was weaker than stated.** Pinning was offered as the mitigation for a swapped tag, but an attacker replacing the tag replaces the persona too — so the warning fires only for someone who already knows which persona to expect, and never for a first-time donor. A pinned persona without a signature is a *claim*: an attacker can print a charity's name over their own address. `TapStatic` now requires a signature by the pinned persona over the address, and an unsigned pin MUST be shown as unauthenticated rather than as an identity. The residual is stated rather than left to be found: a wholly replaced tag verifies under the attacker's own persona, and only a payer who knows what to expect is protected. `CANCEL` fixes the fee to the signed `terms`, so a cancelling party cannot invent a figure the confirm screen never showed.
- **0.36** — **Hail objects implemented; "no route" made structural rather than prescribed.** §5.2's inversion depends entirely on providers never supplying a route — a provider able to smuggle one into a reply would deanonymise every consumer who used it, reinstating the harvesting the section was rewritten to remove. Neither `Hail` nor its reply now *has* a route field, because a prohibition can go unimplemented while a missing field cannot be populated. Also enforced at parse time: replies must echo the hail's nonce, or a stale quote can be replayed against a fresh hail; and geocell precision is capped, since §5.2.3's ladder begins at a district and one generous client's users pay for its generosity.
- **0.35** — **`DISPUTE`/`RULING` implemented, and §9.3.4's expiry rule was a deadlock.** It said an abandoned dispute returns funds to "the pre-dispute allocation" — but under escrow that *is* funds locked in a 2-of-3 awaiting a `RELEASE` two disagreeing parties will never co-sign, so doing nothing freezes them permanently, the exact outcome the timeout claims to prevent. Expiry now emits a real ruling: for the respondent, award zero, co-signable against the escrow. Generalised as a rule for the whole protocol — **in a system with no operator, "nothing happens" is never a safe default**, so every deadline must name the action it triggers. Also enforced: a ruling from outside the market's signed arbiter set is refused (§2.5's lesson in one check), an award cannot exceed the claim, and only a ruling for the claimant may carry one.
- **0.34** — **`MANDATE` implemented, and §15.5 amended to admit it.** §15.5 said the confirm tap is *the one mandatory human checkpoint*; §7.3's mandates authorise payment without one. That was a flat contradiction, resolved by stating that the checkpoint **moves rather than disappearing** — the human confirms a cap and a period once, and every later draw is bounded by what they signed. Holds only because a capless or periodless mandate is now *unparseable* rather than merely refused, the cap is enforced by the payer's own client, and only the named persona may draw. Periods anchor to the first draw, keeping timezones out of the protocol.
- **0.33** — **`ABORT` made directional once a meter is running.** §6 lists it as available to either party with no penalty — correct before value accrues, and a free exit afterwards: a payer could open a tab, consume, abort, and owe nothing. From `METERING` only the operator may void cleanly; a payer leaving is abandonment via `MeterExpired`, which leaves evidence. Also recorded that `CANCEL` does not apply to a running meter, since stopping it and paying what accrued is the instrument that already exists.
- **0.32** — **`METERING` state added; §15.7 and §6.2 had been contradicting each other.** A metered session's `start` leg landed in `ACCEPTED`, whose 60-second deadline aborted it — so a bar tab died one minute after being opened. The two sections were written independently and never checked against each other. `METERING` is deliberately not wall-clock bounded, because its limit lives in `terms.meter_max_s` and the machine holds no terms; expiry arrives as an explicit `MeterExpired` event, following `ConfirmationsReached`'s pattern. Abandonment routes to `CLOSED` with a single-sided receipt, per §15.7.
- **0.31** — **Consecutive capacity corrected from an equality to a bound.** A drain test predicted six consecutive purchases from six unlocked outputs and achieved four: two of the payments consumed two outputs each. Input selection belongs to the wallet, not the client, so capacity is *at most* the output count and can be about half. The earlier seven-outputs-seven-payments result was over-fitted to a single run where one input happened to suffice each time. §17.2 now requires provisioning more finely than the naive calculation, checking capacity before presenting an offer, and never quoting an exact count to a user.
- **0.30** — `REFUND` implemented (§7.3, field keys 34–37) with the three checks a signature cannot make: it must name *this* receipt by chain-link commitment, must not exceed the original amount, and must fall inside `terms.refund_window_s`. Window boundary is inclusive; a refund timestamped before its receipt yields zero elapsed rather than underflowing into an apparently-expired window.
- **0.29** — §8.7.2's relay-rotation guidance promoted from SHOULD to **MUST**, after the failure was reproduced against the market simulator rather than merely reasoned about. A relay died mid-scan; one participant's wallet stopped four blocks short, kept answering `get_height` with a plausible number, and never saw funds that had already settled on chain. Nothing surfaced as an error. Added the detection rule — compare the wallet's height against the relay's own, since a stalled wallet and a synced one give identical answers alone — and the observation that silent divergence is worse for a payee, who ends up telling a customer "not received" about money that is already settled.
- **0.28** — **`FullOffer.terms` added; several requirements were previously unimplementable.** §7.3's cancellation fee and refund window, §15.7's mandatory meter cap and duration limit, and §8.8's minimum fee tier were all written as `terms.*` while no such field existed — rules no conforming client could obey. They now live in a nested map inside the signed offer, so altering them breaks `offer_commit` like any other tampering. The meter requirement is a *pairing* rule: whether a cap is required depends on `amount_authority`, which lives in `TapPresent`, so it cannot be enforced by parsing either object alone.
- **0.27** — **§18.4.1 rule 1 corrected: direction constrains the originator, not the evaluator.** "Only the payer may emit `ACCEPT`" was implemented as a check on the *local* role, so a payee refused every `ACCEPT` it received and no transaction could complete. Both parties run the same machine over the same message and must reach the same verdict, so the originator now travels with the event, established by signature. The bug survived a 75-test suite because every test drove the machine from one side only; a five-party market simulation caught it on the first run — which is the argument for simulating a market rather than testing a state machine.
- **0.26** — **Slashing demonstrated: a bond can be seized over its holder's objection.** A funded 2-of-3 bond was spent by `arbiter + recovery` with the user's wallet never contacted, for signing *or* for key images. The second half was the open question — Monero reconstructs key images from partial ones, and had all three exports been required, a bond could only have been taken with the cooperation of the party being taken from, collapsing §17.2's deposit model. It needs only the threshold count. O1 updated: mechanically validated end to end, with the caveat that this exercised wallet2's multisig rather than the FROSTLASS path §8.2 intends to ship — the mechanism is proven, the code path is not.
- **0.25** — **Pre-split confirmed, and capacity is a count.** Seven consecutive payments of 0.0005 XMR each consumed exactly 0.005 of unlocked balance; the eighth was refused with 0.05 XMR still in the wallet. **A payment costs a whole output regardless of its size**, because the change returns locked — so consecutive capacity is `count(unlocked outputs)`, not a balance, and a float holding one large output makes exactly one payment per lock interval however much it holds. §17.2 now requires clients to surface single-payment capacity and consecutive capacity as two different numbers, since only the second answers *"how many more times can I pay before waiting."*
- **0.24** — §8.7.2: relays fail non-adversarially, and clients must fail over silently. Observed directly — the public stagenet node under test dropped mid-session between two transactions, reporting only `no connection to daemon` while the wallet continued to serve a cached chain height. Two alternatives were live at the same height. A client with one configured relay has an unchosen availability dependency, and stale-but-plausible state is worse than a visible disconnection.
- **0.23** — Canonical home recorded as **ducatproject.org**. Added a release-integrity requirement to §11: a distributed client must be reproducibly built where possible, signed by a key published independently of the site, and hash-verifiable before running — because verifying a hash over HTTPS from the same host that served the binary proves very little, and a domain hijack serving a wallet-draining lookalike is §2.5's lesson one layer up, cheaper to mount than anything else in the threat model.
- **0.22** — **Review gaps closed.** §15.7: every meter is bounded at `start` — the payer confirms a rate, a **cap**, and a maximum duration, because an open-ended obligation cannot be consented to and §15.5 fails without it. Abandoned meters auto-stop, produce a single-sided receipt, and are **collectable only against collateral** — against an unbonded payer the provider bears the loss, as a bar bears a walked tab. §7.4: receipts as records — encrypted local storage of whole transcripts, opt-in backup presented as the privacy decision it is, role-differentiated retention (a consumer holding four years of coffee receipts has built the dossier the protocol avoided creating), and two export forms. §4.2: device delegation generalized from §7.1's staff terminals, covering identity but explicitly **not funds**. §7.3: refunds get a `terms.refund_window`. §18.4.2: a field-number registry reserving ranges for every object the document names, since unassigned numbers are how implementations collide silently. Open problems renumbered into order and all cross-references remapped.
- **0.21** — **The recovery-key contradiction resolved, and arbitration specified.** §17.2's third multisig key could not be filled as written: a user-held key defeats slashing, a stranger-held one recreates §2.5's structure, and a pre-signed timelocked refund is impossible because Monero's custom `unlock_time` is already blocked by relay rule and removed at FCMP++. Resolution is to name the structure honestly — **both non-user keys belong to the market's arbiter set**, making a bond *a deposit under the market's threshold control rather than self-custodied collateral*. A dead market forfeits its bonds; there is no recovery path, because any key that provided one would also defeat slashing. Scoped hard in compensation: zero-conf risk is bounded by transaction value, so most users should never form a bond at all. §9.3 expanded from four lines to a protocol — two dispute classes (mechanical claims are decidable from transcript plus chain and provably wrong when mis-ruled; judgment claims are neither), `DISPUTE`/`EVIDENCE`/`RULING`, the rule that a ruling *is* a co-signature rather than an instruction, staged timeouts, and the uncomfortable terminal case that an unresolvable dispute resolves against the claimant because there is no higher court.
- **0.20** — **Pricing model made explicit.** §6.1: fixed and metered are not redundant, they allocate route risk — fixed puts traffic on the provider, metered on the consumer. Fixed is the default for rides because WYSIWYS is strongest there. Added the condition `rated` mode always depended on and never stated: **a meter is verifiable only where the payer independently observes what is metered.** A ride qualifies — the rider's phone measures the journey itself. A bar tab does not, and profiles using `rated` must now say what the payer measures. §12: a market-published *fare* rate would be price coordination among independent providers and is excluded at every layer; §17.7's *currency* rate is a different object and stays. Price discovery already falls out of §5.2's sealed-bid offers. §11: the take-rate arithmetic stated plainly, with the honest half — the platform cut also buys insurance and trust-and-safety, which this protocol does not replace.
- **0.19** — **§5.2 replaced: providers listen instead of advertising.** Two observations drive it — reading a DHT record imports nothing, so matching can run with zero route imports; and the *publisher* learns the *importer's* address, so publishing is safe and importing is what exposes you. Hail is therefore inverted: providers watch a market record and publish nothing, consumers post a hail carrying a coarse cell and an ephemeral key but no route, providers answer sealed to that key, and only after mutual selection does one import occur — by the party that chose to initiate. **Providers become invisible**, which substantially retires O3 and removes hail's dependency on the Veilid 0.13.0 milestone (O17 contained rather than blocking). Also added §5.2.3: there is no map of nearby drivers, because a live driver map is a published surveillance database of workers' movements — strictly worse than the operator it replaces. The map lives *after* matching, over E2EE, where it is safe and expected. Optional provider visibility is excluded because in a competitive market it is not optional.
- **0.18** — **Added §2.5, the RetoSwap case study.** A Haveno-derived Monero DEX using 2-of-3 arbitrated multisig — §17.2's structure in production — was drained of ~7,000 XMR in May 2026 by a forged, out-of-order ACK that overwrote the arbitrator's address without any check against a known key. **Nothing about Monero failed**; the break was in the messaging layer, which here is Veilid. Route anonymity is not authentication. Four existing rules are the direct countermeasures (§18.3, §18.4, §18.8, §10.1), and §9.3 now states explicitly that arbiters come from the signed market descriptor and never from an address in a message. Second lesson recorded: Haveno was mature, had a prior exploit to learn from, and was breached again — this document's equivalent surface has had no adversarial review at all.
- **0.17** — **`fast/1` acceptance simplified: the recipient scans, the payer does not prove.** Source review of `monero-wallet` 0.2.0 found no transaction-proof support at all — and most of the requirement dissolved on inspection, because a tx proof exists to convince a non-recipient and the driver *is* the recipient. §17.3's Layer 1 and §17.4's flow now have the driver scan the mempool transaction with their own view key; `TXPROOF` becomes `TXID`. Proofs remain necessary for arbitration (§17.5) and are now DUCAT's to implement. Added O14 (scanning is block-oriented, so unconfirmed verification has no public API; plus the missing proof work) and O15 (burning-bug-immune outputs exist but are unspecified outside one implementation, so staying standard and detecting the attack beats adopting them).
- **0.16** — **Client architecture decided: embed a wallet, do not drive `monero-wallet-rpc`.** This dissolves the missing-API and halfway-stranding problems from 0.15 by construction, and permits FROSTLASS (`monero-oxide`, audited May 2025, O(1) per-signer vs native O(n!)) in place of wallet2's experimental multisig — the upstream warning in §8.2 describes wallet2's implementation, not threshold signing on Monero generally. Added the constraint this creates: **bond parties cannot mix schemes**, so `multisig_scheme` joins the market descriptor (§10.1). Also separated the `FLOAT`'s two halves in §17.2's user-facing guidance — "only load what you'll spend" describes `hot_wallet` and mislabels `bond_ms`, which is locked collateral backing fast-settle capacity. O1 reframed.
- **0.15** — **Monero multisig measured (O1).** A 2-of-3 ceremony converged in 2 rounds and 134 s on v0.18.5.1/stagenet — round-trip fragility was overstated. The real obstacles are that Monero ships multisig **disabled by default** with an upstream warning that funds may be unspendable or stealable (now quoted verbatim in §8.2), that **no RPC method enables it**, and that a wallet can be stranded halfway because `prepare` and `make` succeed while `exchange` refuses. O1 reframed from fragility to availability.
- **0.14** — **Field numbering fixed; transcripts exist.** Integer keys assigned for `TapPresent`, `FullOffer`, `ACCEPT`, and `RECEIPT`, with signed objects carried in an envelope `{1: body, 2: sig}` rather than a `sig` field inside the signed map. Encoded sizes measured and replacing estimates: token mode 217 B (was 190), inline 1-hop 915 B (was 886) — the earlier figures counted payload and omitted CBOR key and type headers. **Inline routes now fit no commodity tag at any hop count**, retiring the last argument against token-only tags. §18.9(4) closed: full `xfer/1`, `pos/1`, and `ride/1` transcripts ship in `vectors/v1/transcript.json`, chained and verified end to end, plus a tampered case. 92 vectors, 75 tests.
- **0.13** — **Conformance vectors exported (§18.9).** 88 language-neutral cases in `vectors/v1/`, each carrying a `why`, stated in §18.5 wire codes rather than any implementation's internal errors, deterministic on regeneration, with a runner that executes them against the reference client. O21 narrowed but explicitly *not* closed: vectors validated only by their own author encode that author's bugs as the spec. §18.9(4) full transcripts remain uncovered, blocked on fixing field numbering for `TapPresent`/`FullOffer`/`ACCEPT` — now the largest blocker in Part V.
- **0.12** — **P-256 suite implemented; two encoding-uniqueness bugs found and fixed.** §18.3 now states that uniqueness of encoding reaches past CBOR into values: ECDSA signature malleability (low-`s` mandatory, high-`s` refused rather than normalized) and SEC1 public-key encodings (compressed only, tag checked explicitly — a common parser accepts `0x05` and yields the same key as `0x03`). Both would have produced transcripts that hash differently while every signature still verified. Generalized as a rule: anywhere the protocol admits two byte representations of one value, it has a transcript-divergence bug. Core conformance's both-suites requirement (§4.1) is now satisfiable.
- **0.11** — **Negotiation implemented, and §18.6's suite rule corrected.** "Highest mutually supported" is right for versions and wrong for suites: identifiers are allocated in registration order and encode no preference, so highest-wins would have silently selected P-256 — a fallback forced by iOS hardware (§4.1) — over Ed25519 on every dual-capable pair. Suites are now chosen from an explicit preference list held by the payer. Also established that downgrade resistance needs no new machinery, since `offer_commit` already covers the advertised set — but the commitment MUST be checked *before* negotiating, not after. Extended §18.3's domain separation to commitments as well as signatures, since `offer_commit`, `H(RECEIPT)`, chain links, and `market_id` all hash canonical objects and a bare digest records which role it was for.
- **0.10** — **First implementation, and the gaps it found.** `ducat-core` implements §18.1–18.5: deterministic CBOR, domain-separated signing, the contract state machine, and reject codes, with 35 tests. Added §18.4.1 for six rules the transition table left open — message direction (only the payer may `ACCEPT`), `CANCEL`'s closing bound, the mode-dependent post-`ACCEPT` deadline (60 s direct/fast, 300 s escrow with fund recovery), the `FUNDED` deadline applying only under `fast/1`, terminal-state absorption with `CLOSED` deliberately excluded, and elapsed time in unbounded states being a no-op. Also established that no serde CBOR crate can satisfy §18.1, since none reject non-canonical input on decode.
- **0.9** — **Phase 0 completed; all three experiments have numbers.** 0a measured on veilid-core 0.5.7: `token` mode 190 B, `inline` mode 877–1669 B, **non-monotonic in hop count** with within-hop variance exceeding between-hop differences, because size tracks which peer was selected rather than how many hops were requested. This invalidated 0.5's claim that QR level H clears inline mode at every hop count — across two runs it passed in one and failed in the other, minutes apart. Tags now ship tokens; `token` mode reframed as the *privacy-preserving* choice since its size is constant regardless of hop count. 0b measured: 135–204 KB/s at 32 KB payloads, latency-dominated, adequate for incremental sync and not for a full chain — which is what §17.1's restore height already assumes. O11 and O16 closed. The earlier claim that Veilid needs inbound port forwarding was wrong and is retracted.
- **0.8** — **First empirical results (Phase 0).** 0c answered: Veilid #395 is open, milestoned to 0.13.0 against a current 0.5.7 — no near-term fix, and remote hail (§5.2) is gated on it (O17). **Corrected §15.10:** `token` mode does *not* mitigate hostile-route deanonymization, contrary to 0.5's claim — the exposure is in using the route, not in how the blob arrived. 0a dissolved rather than answered O11: `inline` blobs have no fixed size because the entry hop carries a third party's peer info, so §15.3.2 now mandates measure-and-degrade instead of a budget. 0b remains blocked on a host with inbound reachability. Harness and full results in `phase0/`.
- **0.7** — **Renamed SPECIE → DUCAT.** The old name was semantically exact and practically hostile: one letter from "species," pronounced *SPEE-shee* by almost nobody, and search-polluted by both biology and finance. Wire constants changed with it — domain-separation prefix `DUCAT-v1` (§18.3), NFC AID `F0 44 43 41 54` (§18.7), QR magic `DCAT`, URI scheme `ducat:`. Also: §9.4 scoped — the safety floor binds the high-exposure tier (rides, lodging, tasks) and not commerce as a whole, which narrows O5 and removes the apparent tension with §1's reframe. Four remaining gaps closed. §4.1: key storage, and the P-256 suite iOS's Secure Enclave forces into the registry. §6.2: clock skew — monotonic for elapsed time, ±120 s tolerance for absolute, both directions failing closed, skew detected but never applied. §8.8: transaction fees, `fee_policy`, the WYSIWYS requirement to display total outlay, and minimum-fee-tier refusal closing §17.8's cure-window abuse. §10.1: the market descriptor — self-certifying `market_id`, threshold-signed rotation chained from genesis. O8 narrowed from unspecified to mechanism-specified-accountability-open.
- **0.6** — **Added Part V (§18): wire format and conformance.** Canonical CBOR rules, integer-piconero money (correcting the float hazard in `amount_xmr`, now `amount_pxmr`), signature domain separation and the verify-received-bytes rule, the normative state transition table, reject codes, downgrade-resistant version negotiation, transport bindings (NFC AID, BLE UUIDs, QR envelopes with per-EC-level capacities), strict rejection of unknown fields, required test-vector coverage, and four conformance levels. §11's many-clients claim now points at something payable. Phase 1 ships the codec and identifiers rather than retrofitting them. O20–O21 added.
- **0.5** — **The tap is medium-agnostic; QR leads.** §15.1 restated: the bootstrap moves ~200 B–1 KB across a few feet so both phones can finish over Veilid, and only one direction needs to cross — which is why a one-way medium suffices. Added §15.3.1 (two reach modes, `inline` vs `token`, with concrete sizes) and §15.3.2 (the transport ladder, the NFC platform matrix, BLE channel security via Noise XX). NFC demoted to an Android-presenter optimization: iOS HCE is gated behind an EEA/organization/financial-regulatory entitlement incompatible with A4. Added QR's weaker relay resistance, hostile-route-blob deanonymization (Veilid #395), and mandatory length padding to §15.10. O11 downgraded, O19 added, Phase 0c added.
- **0.4** — **Reframe: the unit of ambition is the card terminal, not the rideshare app.** §1 restated with what DUCAT actually replaces; rides recast as the *hardest* profile rather than the representative one. Added `pos/1` (§7.1), `exchange/1` (§7.2 — the on-ramp as a profile), and `goods/1`. Added §7.3 cross-profile mechanics: refunds, cancellation/no-show, standing mandates, multi-payee splits, with `REFUND`/`CANCEL`/`MANDATE` in the §6 registry. Added §6.2 — timeouts on every state, plus the single-sided receipt for the post-FUND/pre-RECEIPT window. Fixed the Monero output-lock flaw in §17.2 (pre-split the float; capacity is over unlocked outputs, not balance). Build order resequenced: `pos/1` before `ride/1`. O18 added.
- **0.3** — **The settlement leg is a network path too.** Added §2.4 (two networks, two dependencies) and §8.7 (network path for settlement), specifying submission and scanning over Veilid, why Veilid does *not* replace Dandelion++, and why full monerod-over-Veilid is rejected. Added the `relay/1` profile (§8.7.2) — node access as a staked, verifiable service — answering "whose monerod?" without reintroducing a light-wallet server. T1 amended; O13–O16 added; Phase 0 split into 0a/0b and node access promoted into Phase 1.
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
| **Device** | Hardware-backed where available | Signs on a persona's behalf under §4.2; **disposable** — a lost device is revoked and replaced, so unextractability costs nothing |
| **Persona** | Software, exportable, encrypted at rest | **Irreplaceable.** See below |
| **Stake / bond** | Hardware-backed **and** biometric- or passcode-gated | Directly spendable collateral; the highest-value key on the device |

**Why the persona row does not say "hardware-backed", which it did until 0.42.** Hardware backing means the key cannot be extracted — that is the entire point of it — and a key that cannot be extracted cannot be backed up. Recommending it for personas therefore made every persona unrecoverable on device loss, silently and permanently, and no backup mechanism could have fixed it. That is not a defensible trade for the one key class the user cannot afford to lose: a destroyed persona takes every persistent contact (§16) with it, and ends the user's ability to accrue reputation at all (§9.2).

The rule that resolves it is **replaceability**:

- **Replaceable keys are hardware-backed.** Device keys are unextractable and never leave the device. Losing one costs a device: the persona revokes its delegation and issues another (§4.2).
- **Irreplaceable keys are exportable and protected by a passphrase** (§4.3). Persona keys are software keys because they must be able to outlive the hardware they were created on.

Hardware backing where it is disposable, exportability where it is not.

This governs the *persona* key specifically. Where the user's **money** lives is a separate choice with its own trade-offs, including an external hardware wallet — §4.4. Note that no custody mode moves the persona off the phone: a Monero hardware wallet signs Monero transactions, not DUCAT's domain-separated objects.

**The honest limit:** hardware backing prevents key *extraction*. It does not prevent malware from asking the enclave to sign while the user is looking at something else. It raises the cost of a stolen phone, not of a compromised one — and §2.2 already places endpoint compromise out of scope. This is defense in depth, not a new guarantee. The corollary is that moving personas out of hardware gives up less than it first appears: against the threat hardware actually addresses, a persona key's exposure now rests on the encrypted backup and the device's own storage protections rather than on an enclave.

**Cross-platform consequence:** a persona created under the P-256 suite is unverifiable by a client implementing only the Ed25519 suite, which would fragment personas by platform. Core conformance therefore requires *both* suites (§18.10).

### 4.2 Device Delegation — One Persona, Several Devices

§7.1 gives a merchant persona the ability to authorize staff terminal keys. That is not a merchant feature; it is the general answer to a problem the rest of the document left open — **a person with a phone and a tablet**, who otherwise needs a separate persona per device and thereby splits their own reputation.

A persona signs a **delegation** naming a device key, its permitted profiles, and an expiry. The device key signs on the persona's behalf within those limits; verifiers check the delegation chain back to the persona. Revoking a device is republishing the delegation set, so a lost tablet costs a device rather than an identity.

Three limits, all load-bearing:

- **Delegation covers identity, not funds.** A device key can present, negotiate, and co-sign receipts. It cannot spend the hot wallet or move the bond, because those are Monero keys and threshold membership, not delegable signatures. A second device is a second terminal, not a second purse.
- **Delegations are visible to counterparties**, since a verifier must see the chain. A payer learns that they are dealing with device 3 of a persona — which is a small linkability cost and the reason a delegation carries no device metadata beyond a key.
- **Revocation is only as fresh as the delegation set a counterparty has read.** A stolen device stays usable to anyone who has not refreshed. Short expiries bound this; nothing eliminates it, which is the same limit every revocation scheme has.

### 4.3 Backup — Export and Import (closes O12)

O12 asked what happens when a device is lost. Until 0.42 the answer was that the persona, its reputation, and every rendezvous keyed to it were gone. For a payment application that is disqualifying, and it is the failure users are least forgiving of, because it looks exactly like theft by the software.

**What this deliberately is not.** Not social recovery: guardians, shard distribution, and quorum rejoin are a large amount of protocol and user-facing ceremony for a system with no operator to coordinate them, and they introduce a social attack surface — collusion, coercion, guardians who stop answering — where none existed. Not custody: there is no service to hold anything. Not automatic: nothing is uploaded, and no counterparty, relay, or DHT record ever holds any part of a backup.

**What it is:** a single encrypted file the user exports and keeps, and can import on any device. The user chooses where it lives — password manager, cloud drive, offline media, printed. That choice is deliberately theirs; a protocol that also decided *where* would be back to needing a service.

#### 4.3.1 Contents

A backup MUST carry all four, and the reasons for the last three are not symmetric with the first:

| Field | Why it must be there |
|---|---|
| **Persona secret key + suite** | The identity itself. Without it the restored user is a stranger with a new reputation. |
| **Monero seed** (25-word Electrum-style) | The money. Restores spend and view keys. |
| **Monero restore height** | See below — this is load-bearing, not metadata. |
| **Rendezvous record keys** (§16.4) | Without them a restored persona keeps its identity and loses every contact. It can be paid and nobody it knows can reach it — a half-restore that is worse than a visible failure, because it looks like it worked. |
| **Attestation record writer keys** (§9.2) | Attestations live in DHT records the persona *controls*, and control is the record's own writer key — which is **not** derived from the persona key. Restoring the persona without it leaves the existing attestations readable but frozen forever: the user can never add another, so their standing stops at the moment the device died. Nobody would attribute that to a backup. |
| **Granted mandates** (§7.3) | A standing authorization the user can no longer see is one they cannot revoke. Dropping these leaves live drawing rights against the restored wallet, invisible to its owner. |
| **Verification thresholds** (§15.5.1) | Not a credential, and included anyway. Losing them fails *safe* — the defaults are stricter than most users' settings — but a merchant who raised their floor limit for a busy counter and restores to defaults finds the terminal demanding a secret on every sale, with nothing to explain why. Silently reverting a deliberate setting is its own kind of data loss. Restoring them weakens nothing exploitable, since §15.5.1 keeps verification entirely off the wire. |

**The restore height is not a convenience field.** A Monero wallet restored without one rescans from the genesis block. Phase 0b measured this directly against a remote stagenet node: **roughly 106 hours from genesis, against 35 seconds from a recent height.** Omitting an eight-byte integer therefore converts a restore into a four-day ordeal during which the user's balance reads zero — and they will conclude, reasonably, that the backup did not work and their money is gone. A backup format that stores the seed and not the height is technically complete and practically broken.

**And the height must be *right*, not merely present — it is wrong in both directions, asymmetrically.**

*Too late is silent and total.* The obvious implementation — stamp the current height at export — makes the restored wallet scan forward from after every output it owns. Demonstrated on stagenet: correct seed, correct address, **zero balance, and no error anywhere**. The money is untouched and the user is looking at an empty wallet. A restore height above the wallet's oldest unspent output does not degrade recovery; it silently cancels it.

*Too early is merely expensive, and the price is measurable.* Backdating 500 blocks cost roughly two minutes of scanning (measured; ~4 blocks/second against a remote stagenet node). Extrapolated, "just use the genesis block to be safe" is the ~106-hour figure above, and even backdating a year of chain is on the order of a day.

The rule is therefore exact: **`restore_height` MUST be at or below the block containing the wallet's oldest unspent output, and SHOULD be as close to it as possible.** Outputs older than that are by definition already spent — a restored wallet that never sees them loses nothing but the scanning time it would have cost to find them.

Implementations SHOULD recompute this on each export rather than stamping a creation height once. A long-lived wallet that spends its early outputs can move its restore height forward over time, and a backup that keeps rescanning from a persona's first day gets slower every year for no benefit.

#### 4.3.2 Construction

- **KDF: Argon2id**, memory-hard, with parameters **pinned by the format version**, not inherited from a library default. A backup is an *offline* target: whoever holds the file can grind at it forever, on hardware of their choosing, with no rate limit anyone can impose. Memory hardness is what denies them cheap GPU and ASIC parallelism. The reference implementation uses 64 MiB, 3 passes, 1 lane — above OWASP's floor, and a cost the user pays once at import.
- **AEAD: XChaCha20-Poly1305.** The 24-byte nonce removes any nonce-reuse concern across exports without a counter the format would have to maintain.
- **Salt and nonce are fresh per export** and stored in the clear. They are not secrets; reusing either would be one. A consequence worth stating: two exports of an unchanged backup MUST differ, so possession of two files reveals nothing about whether anything changed between them.
- **Plaintext is canonical CBOR** under §18.2's codec, so the artifact is self-describing and deterministic.
- **An import is a trust boundary.** Fields that carry their own construction rules MUST be re-validated on the way in, not installed because the bundle decrypted cleanly. A verification policy whose ladder inverts is refused at import exactly as it would be at construction — decryption proves the file was not tampered with, not that its contents were ever sane.
- **The format identifier is authenticated as AAD**, so a file of another format cannot be coerced into decrypting as this one, and so the version selecting the KDF parameters is readable *before* any key is derived.

**Parameters and field numbering are frozen once shipped.** A changed KDF constant or a reordered CBOR key derives a different key or a different plaintext, and every backup any user has ever exported becomes permanently unopenable — while reporting nothing but "wrong passphrase" to people typing the correct one. The failure is indistinguishable from user error and would be diagnosed late, if at all. Implementations MUST hold a known-answer vector over the complete artifact so this breaks a test rather than a person's wallet. A format change is a new version identifier, with the old version still decryptable.

#### 4.3.3 Multisig Shares — Not From the Seed, But From the Key File (closes O22)

A bond (§17.2) and an escrow (§8.2) are Monero **multisig** wallets. This section said until 0.44 that their shares were deliberately omitted, on the grounds that `monero-wallet-rpc` cannot restore one. That reasoning was sound and the conclusion was wrong: it assumed restoring a share means *reconstructing* it, and it does not.

**First, what does not work.** A share is not derivable from the wallet seed. Measured on v0.18.5.1: two wallets given byte-identical key material produced `prepare_multisig` outputs agreeing for 101 characters and then diverging for 88 more. The ceremony draws fresh randomness, so restoring a seed and replaying a recorded ceremony reproduces a *different* share. That closes the cheap route.

**Second, what does.** Restoring a share does not require an RPC method, because it does not require reconstruction — the share is already a file. Verified against stagenet: copying a 2-of-3 wallet's **2,286-byte `.keys` file** into a virgin directory and calling `open_wallet` produced a wallet reporting `multisig: true, ready: true, threshold 2, total 3` at the correct group address, which then produced valid `export_multisig_info` and scanned its balance. The missing `restore_multisig_wallet` method is simply not on the path.

Note the size, because it decides feasibility: **2,286 bytes.** The multi-megabyte file beside it (52 MB in the measured case) is scan cache — it rebuilds itself and MUST NOT be backed up.

So the bundle carries escrow shares as opaque key-file bytes. Four rules, each from something observed:

- **Capture at `ready`, never before.** A ceremony interrupted between `make` and `exchange` restores exactly as interrupted, which is the stranded half-formed wallet §8.2 already warns about. A backup of a broken state is a broken backup.
- **Do not back up the cache.** Restore height replaces it, under §4.3.1's rule.
- **Re-export when an escrow opens.** This is the only part of the bundle with a *freshness* requirement. A persona key from last year is still the persona; a bundle exported before an escrow existed cannot contain it. Clients MUST prompt for re-export when a ceremony reports ready, because the user has no way to know the bundle went stale.
- **Restoring membership is not restoring the ability to spend.** A restored share must re-exchange multisig info with the other participants before it can sign. This is a real dependency and a much weaker one than it sounds — see below.

**Why this resolves O22 rather than merely improving it.** The problem was never that funds were locked; it was *who held the key to unlock them*. Under the old reading, a buyer who lost their device needed the **seller's signature** to receive a ruling in their own favour — an adversary being asked to sign away their own claim, which nothing can compel. Under this one, the buyer restores their own share and needs the other participants to re-share multisig info. That is not an approval: importing multisig info endorses no outcome, authorises no transfer, and is the same non-discretionary step every multisig participant performs routinely. The arbiter can supply it as part of ordinary duty.

**The trust ask moved from an adversary's consent to a participant's cooperation in a mechanical step**, and that is the difference between an unsolvable position and an operational one.

**Bonds need none of this.** §17.2 places both non-user keys in the market's arbiter set, so a bond never required the user's signature to move. Backing up a bond share is optional convenience, not recovery.

**Honest limits.**

- A stale bundle does not recover an escrow opened after it. The freshness rule above is a prompt, and a user who dismisses it is unprotected. This is now a UX failure rather than a protocol impossibility, which is progress and not a guarantee.
- Recovery requires the other participants to still exist and respond. A counterparty who has vanished and an arbiter set that is dead leave the escrow stuck — the same dependency §17.2 already accepts when it says a dead market forfeits its bonds.
- **An end-to-end multisig spend from a restored share was not demonstrated.** The restored wallet refused to build one — and so did the *original*, with the same `-16`, because both needed a fresh multisig-info exchange after an earlier spend. The copy was indistinguishable from the original at every point measured, which is the claim being made here; it is not the same as having watched a restored share co-sign a real transaction, and that test remains to be run.

#### 4.3.4 What this does not solve, stated plainly

- **A forgotten passphrase is unrecoverable.** There is no operator to appeal to; that is the same property that makes the system uncustodied. Clients SHOULD say this at export, in those terms, rather than in the language of a password reset the user might expect to exist.
- **The passphrase is the whole of the protection.** Argon2id raises the cost per guess; it does not rescue a passphrase drawn from a small space. Clients SHOULD refuse trivially short passphrases at export rather than producing an artifact whose protection is nominal.
- **A backup is a spending credential — in software custody, for the whole balance.** Anyone with the file and the passphrase becomes the user completely. It is not a receipt or an archive, and should not be treated as safe to store casually. Under §4.4's hardware-reserve mode this bound shrinks to the float, since the reserve sits behind a device seed DUCAT never sees; a client MUST NOT show the same warning in both modes, because it is false in one of them.
- **The relationships are credentials too, and the bundle carries them.** A contact is not an address-book entry: it is the writer keypair for our half of a conversation (lose it and the log is readable but unwritable — measured, not hypothesised), the peer's outbox and cached prekeys, and two chain counters that decide whether the next message in either direction is accepted at all. These travel as typed fields another client can restore. One consequence is stated in §16.11's terms rather than hidden: **a backup containing one-time prekey secrets can rewind forward secrecy to the moment it was made** — delete-on-use is the entire property, and a copy predating the use undoes the delete for whoever holds the file. They are included anyway, because excluding them strands every message sealed to them at exactly the moment of device loss, and this file is already a complete spending credential; the marginal exposure rides an artifact that must already be guarded absolutely. Same-client continuity (threads, tabs, presentation) rides one opaque `app_state` field with no interop promise, which is what keeps the typed list honest.
- **A credential backup is not a record backup.** This bundle answers *who the user is, what they can spend, who can reach them, and what they have authorised*. It does not carry transaction history; receipts are a separate growing archive with its own export (§7.4), and §7.4's warning that losing receipts loses evidentiary reputation still stands in full. The split is not only about size — the two have opposite refresh needs. A credential bundle exported a year ago is still entirely valid; a receipt archive from a year ago is a year out of date. Merging them would force constant re-export of a bundle whose important half never changes.
- **Restore does not recover in-flight state, and for escrow that is permanent.** Open sessions and undelivered receipts are merely absent; a restored user re-establishes them. An escrow is different in kind — §4.3.3 — because the multisig share cannot be reconstructed and funds favouring the lost party need a signature only their counterparty can give. A restore recovers identity, funds, contacts, and standing authorizations. It does not recover an escrow that was open when the device died.
- **The old device is not disabled by restoring.** Import copies a persona; it does not revoke one. A user restoring because a device was *stolen* rather than lost MUST also revoke that device's delegation (§4.2) — and until every counterparty refreshes, the thief's device still verifies. Clients SHOULD prompt for revocation as part of the import flow, because a user who has just recovered their money will not think of it unprompted.


### 4.4 Custody Modes — Where the Spend Key Lives

§4.1 and §4.3 settled identity: the persona is software, exportable, and protected by a passphrase. They said nothing about the *money*, and the two are not the same question.

**Two different things are called "hardware keys", and they behave oppositely.**

| | Secure element (Secure Enclave, StrongBox) | External hardware wallet (Ledger, Trezor) |
|---|---|---|
| Key can be extracted | No | No |
| **Survives losing the phone** | **No** | **Yes** |
| Has its own backup | No | Yes — its own seed phrase |

The first is what made §4.1's original rule unrecoverable. The second has no backup problem at all, because **the device is the backup**. That distinction is the whole reason a custody choice is worth offering.

#### 4.4.1 What a Monero hardware wallet cannot do

Two hard limits, both external to DUCAT, and both narrower than "use a hardware wallet" suggests:

- **It cannot hold the persona key.** A Ledger or Trezor running the Monero app signs Monero transactions. It does not produce Ed25519 or P-256 signatures over DUCAT's domain-separated objects (§18.3). **No custody mode moves the persona off the phone**, so §4.3's export/import remains the only answer for identity in every mode. This choice is about money, not about who you are.
- **It cannot hold a multisig share.** Monero multisig on hardware is roadmap, not shipped — consistent with §4.3.3's finding that even `monero-wallet-rpc` cannot restore one. Escrow (§8.2) and bonds (§17.2) are multisig, so a device-held spend key can enter neither.

A third limit is practical rather than cryptographic: a hardware wallet needs a physical connection and a button press. That is not a slow tap, it is a different interaction, and it does not fit §15's budget.

#### 4.4.2 The three modes

A client SHOULD offer this choice at first run, and MUST make the consequences visible rather than presenting three security levels.

| Mode | Spend key | Tap (§15) | Escrow / bond | A leaked backup costs |
|---|---|---|---|---|
| **Software** | On the phone | Yes | Yes | **The whole balance** |
| **Hardware reserve** *(recommended)* | Float on the phone, reserve on the device | Yes | Yes | The float |
| **Hardware only** | On the device | No | No | Nothing |

**Hardware reserve is the recommended shape, and not as generic defence in depth.** It closes something §4.3 could not close on its own. §4.3.4 must warn that a backup file is a complete spending credential — anyone with the file and the passphrase becomes the user entirely. Behind a reserve that sentence is simply false: the bundle carries the hot wallet's seed, and the reserve sits behind a device seed phrase DUCAT never sees or stores. **The amount at risk becomes a number the user chose**, and the same bound covers a stolen phone, a leaked backup, and a compromised client at once.

It also needs no new machinery. §17.2 already models a float as a count of pre-split unlocked outputs sized for expected consecutive payments; a reserve is simply where that float is topped up from. Top-up is an ordinary on-chain transfer, so it costs a fee and ten blocks of lock time, which is the real argument for sizing the float generously rather than topping up per purchase.

**Clients MUST refuse an impossible combination before presenting an offer, not at FUND.** A hardware-only user who reaches a counter and discovers that escrow cannot be funded has failed in front of a queue with a customer already committed. The capability check belongs where the settlement mode is chosen.

**Role matters, for exactly one mode.** Under `fast/1` the bond is the *provider's* (§17), so a payer holds no multisig share: **a hardware-only consumer can pay a bonded merchant perfectly well**, and only the provider side is barred. Refusing both sides would be simpler and would lock hardware users out of the flow they are best suited to.

#### 4.4.3 What this means for O22

§4.3.3 raised O22 — an escrow participant who loses their device strands the escrow — and 0.44 closed it by carrying the share as a key file. That closure applies to **software** and **hardware-reserve** modes, whose multisig shares are software key files like any other.

Hardware-only mode sits outside it for a different reason: such a user can never enter escrow at all, since a device-held key cannot hold a multisig share. The exposure is unreachable rather than recovered. That is not a fix to recommend, because it is also the mode that cannot tap.

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

**"No route" is a property of the objects, not a rule about them.** Neither `Hail` nor its sealed reply has a route field to populate, so a provider cannot attach one and a consumer cannot helpfully import one. This matters because the entire inversion rests on it: a provider able to smuggle a route into a reply would deanonymise every consumer that used it (§15.10), reintroducing precisely the harvesting this section was rewritten to eliminate. A prohibition can go unimplemented; a missing field cannot be populated. An object carrying an unrecognised field is refused outright under §18.8 rather than parsed with the extra quietly ignored.

Two further properties the objects enforce rather than request:

- **A reply must echo the hail's nonce.** Without it a provider's stale reply could be replayed against a fresh hail, and a consumer would be selecting from quotes nobody currently stands behind.
- **Geocell precision is bounded at parse time.** §5.2.3's ladder begins at a district; an over-precise cell turns its first rung into a position fix. Precision is therefore capped by the parser rather than left to each client's discretion, since the cost of one client being generous is borne by its user.

#### 5.2.2 What this fixes, and what it does not

**Providers become invisible.** A harvester watching a market learns nothing about supply — there is no advert, no persistent presence, no standing dossier of who works where. It learns only that *someone* hailed from a coarse cell at a time, which is ephemeral and unattributed. This is the substance of O3's improvement, and it also retires geocell epoch rotation, per-cell advert encryption, and the rate-card distribution sub-problem, all of which existed only to make persistent adverts survivable.

**#395 is contained, not closed.** One exposure per real transaction, to a counterparty the exposed party selected, instead of one per browse to anyone watching. That is the same posture the proximity tap already has and §15.10 already accepts. The upstream fix is still needed before the final import is clean.

**Hail spam is unsolved.** Anyone can write to a market record. Per-subkey rate limits in the record schema are the cheap mitigation; requiring a stake or bond proof to post is the strong one, at the cost of pulling consumers toward collateral — the same pressure O18 describes.

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

**The map exists — it just lives after the match rather than before it.** Once two parties have selected each other, live position sharing over the E2EE session is both safe and expected: they have consented, and they are about to be physically co-present anyway (§2.3). Watching your driver approach works exactly as riders expect (mechanics in §15.12's live-position stream). What does not exist is browsing strangers' locations before any relationship exists.

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
TXID            payer → payee             mempool pointer for zero-conf accept (fast/1 only, §17.4)
TXPROOF         claimant → arbiter        transaction proof; arbitration evidence only (§17.5)
PROOF           either → either           delivery evidence (profile-defined)
RELEASE         consumer → escrow         escrow disbursal (escrow mode only)
RECEIPT         both                      co-signed closure; input to attestation
SETTLED         local                     finality observed; fast/1 obligation clears (§17.4)
ABORT           either                    pre-FUND cancellation, no penalty
DISPUTE         claimant → arbiter set    contest carrying the transcript (§9.3.2)
EVIDENCE        either → arbiter set      voluntary disclosure, judgment class only (§9.3.2)
RULING          arbiter → both            outcome; *is* a multisig co-signature (§9.3.2)
CANCEL          either                    post-ACCEPT cancellation; invokes terms.cancellation (§7.3)
REFUND          payee → payer             voluntary, receipt-bound reverse payment (§7.3)
MANDATE         payer → payee             capped standing authorization; unilaterally revocable (§7.3)
CONTACT_OFFER   either → either           optional post-RECEIPT identity coda (§16.3)
CONTACT_ACCEPT  either → either           completes the mutual contact (§16.3)
```

**The in-person tap collapses the first three.** `TapPresent` (§15.3) carries the advert commitment and the hail in one gesture; `FullOffer` (§15.4) *is* the QUOTE, delivered over the channel the tap just opened. The remote-hail path (§5.2) runs the same three roles over DHT records rather than a channel: the consumer's HAIL is a record write carrying no route, the provider's QUOTE is a sealed reply, and no ADVERT exists at all because providers no longer advertise. One state machine, two entry paths — this equivalence is normative, and a client that implements only the tap path must still produce transcripts a remote-hail client can verify.

### 6.1 Deterministic Pricing

A QUOTE must be reproducible: `price = f(rate_card, route_inputs)` where `f` is specified per profile and computed **locally** (on-device OSRM/Valhalla for rides; no external map or pricing API in the loop). The rate card is committed before the price is seen — in the ADVERT for remote hails, and via `offer_commit` in the `TapPresent` for in-person taps (§15.3), which binds the `FullOffer` carrying it — so a provider cannot quote off-card without detection. This makes pricing *more* auditable than any surge algorithm — transparency as a feature, not a compliance cost.

**Fixed and metered are not redundant; they allocate route risk.** A fixed price computed from the route means the provider absorbs traffic and detours. A meter means the consumer does. Both are legitimate commerce, and the choice belongs to the profile and the parties rather than to this document.

**Fixed is the default for rides, because WYSIWYS is strongest there.** The payer's app derives the fare independently from rate card and destination and refuses a mismatch (§15.5); nothing about the final number requires trusting the provider.

**A meter is verifiable only where the payer independently observes what is metered.** This is the condition `rated` mode (§15.2) depends on and it is not universal:

- **A ride qualifies.** The rider's own phone was in the vehicle. It measures elapsed time and distance itself, recomputes the total from the rate confirmed at `start`, and flags a discrepancy exactly as it would for a fixed fare.
- **A bar tab does not.** Only the vendor knows what was served. The payer confirms a rate at `start` and is handed a total at `stop` that they have no way to check — which is precisely the hostile-terminal scenario §15.5 exists to prevent.

Profiles using `rated` MUST therefore state what the payer measures. Where the payer cannot measure it, the total is an unverifiable claim, and the profile should use `fixed` per unit with a tap per item, or accept that it has stepped outside the protocol's security model and say so in the UX.

### 6.2 Timeouts and Failure Transitions

A state machine without deadlines is unimplementable. **Every await has a deadline and a defined transition**, because a dead Veilid route is indistinguishable from a silent counterparty and both must resolve the same way. Defaults, overridable per profile:

| Awaiting | Default | On expiry |
|---|---|---|
| `FullOffer` after tap | 10 s | Discard silently; no screen ever shown to the human |
| ACCEPT (offer displayed) | `TapPresent.expiry` (≤ 30 s) | ABORT, no penalty |
| FUND after ACCEPT | 60 s | ABORT, no penalty |
| TXID (`fast/1`) | 30 s | Provider falls back to `direct` (wait for confirmations) or ABORTs |
| PROOF of delivery | Profile-defined, **bounded** | `CLOSED` + payment evidence — see the audit below |
| RECEIPT co-signature | 120 s | **Single-sided receipt** (below) |
| Multisig setup (escrow) | 300 s | ABORT + fund recovery path (§8.2) |
| RELEASE (escrow) | Profile-defined | DISPUTE becomes eligible (§9.3) |
| Confirmation (`fast/1`) | 20 blocks | CLAIMED — the cure window (§17.5) |
| Contact window post-RECEIPT | 120 s | Session teardown; session keys destroyed (§4) |

**`TXID` and `RECEIPT` MUST NOT be one request and its response.** The payee's answer waits on a chain scan, and a transport's patience has nothing to do with how long Monero takes. Holding a call open until the scan finishes delivers *a slow confirmation* and *a fabricated TXID* to the payer as the same timeout — indistinguishable, and pointing at the network rather than at the payment.

It is also a denial of service, and a cheap one. An implementation that scans synchronously blocks for the whole scan window on a value the counterparty chose, so **one message naming a transaction that does not exist freezes a terminal**. Measured in the harness before the fix: five minutes, for the cost of sending 40 bytes.

So the structural checks — does this TXID name the right ACCEPT, for the right amount — are synchronous and cheap, and the payee **acknowledges immediately**. The scan runs off the session, and the receipt is collected separately. Two rules follow, and the second is the general one:

- **Bound the scan by this section's window.** Mempool visibility is near-immediate when a payment is real; a long wait is evidence of absence, not of slowness.
- **Nothing that waits on the world may hold a session open.** This is the same failure as a server that dies on malformed input: both convert a counterparty's message into an outage, and both are invisible until someone hostile sends one.

**The dangerous window is post-FUND, pre-RECEIPT** — the payer's money is gone and the co-signed record does not yet exist. A counterparty that vanishes here must not be able to erase the transaction, so the payer's client emits a **single-sided receipt**: its own signed record of `{ ACCEPT, TXID, timestamp }`, valid as dispute evidence (§9.3) and as an attestation input (§9.2), and explicitly flagged as unilateral. It proves what the payer signed and paid; it cannot prove delivery, and it does not claim to.

**There are two unilateral receipts and they assert opposite things.** This one is *payment evidence* — the payer saying "I paid and hold no co-signature". An abandoned meter (§15.7) produces *debt evidence* — the payee saying "you owe me and never stopped the meter". A client is told which to write, because inferring the direction from the state it just left would put the decision back in the place a state machine exists to remove it from. Conflating them would have a payer's client recording a debt it does not owe, or a merchant filing a payment it never received.

#### The audit: every deadline must name an action

Cycle 9 of this document's development produced a rule while fixing dispute expiry: **in a system with no operator, "nothing happens" is not a safe default.** Auditing the rest of §6.2 against it found the same class of failure in a worse place.

**`FUNDED` and `PROVISIONAL` had no deadline at all, under any settlement mode.** The row above previously read "profile-defined", which is a deferral rather than an action — and an undefined profile meant unbounded. A payer who had funded and never received `PROOF` waited **forever**: the money already gone, no exit, and no record to show for it. That is strictly worse than the post-`FUND` window §6.2 already worried about, because there at least a deadline existed.

The backstop is `DeliveryWindowExpired`, which closes the transaction and emits **payment evidence**. A profile sets the window; the protocol requires that one exist. The event is signalled by the caller rather than fired by a wall-clock deadline, because the window belongs to the profile and the machine holds no profile — the same reasoning as `MeterExpired`.

The general invariant, now tested: **no state that can hold committed funds may be a dead end.** Every such state has an exit reachable without the counterparty's cooperation, because a counterparty who has your money is exactly the one who may not cooperate.

Two smaller findings from the same audit:

- **The 60-second `FUND` deadline is shorter than a Monero block.** It therefore cannot be satisfied by waiting for confirmation, and a payee that watches only confirmed blocks will abort every transaction it ever receives. The deadline is meaningful only against **mempool** visibility (§17.3), which is why the payer sends a `TXID` and why the payee scans rather than waits.
- **The `TXID` expiry named two actions** — "falls back to `direct` or aborts" — where the state machine can only do one. Falling back is a *policy choice made before the deadline*, not an outcome of it: a provider willing to wait for confirmations should not have set a 30-second window in the first place. Expiry aborts.

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
- **Offline lane.** A stall or festival vendor with no connectivity is a common case, not an edge case. Until §8.4 resolves, `pos/1` degrades honestly rather than silently: the merchant holds the signed ACCEPT and TXID and confirms when connectivity returns — bounded by the customer's bond where one exists (§17), and declined where it does not.

### 7.2 `exchange/1` — The On-Ramp Is a Profile

To transact, a consumer needs XMR; to transact *fast*, they also need a bond (§17). That is two acquisition steps before a first tap, and it is the real shape of §10's cold-start wall — not a shortage of drivers, but a shortage of anyone holding the settlement asset.

`exchange/1` makes the on-ramp a DUCAT service: two people meet, one hands over cash, the other sends XMR, both co-sign. It is the same move §9.3 makes for arbitration and §8.7.2 makes for node access — **the network bootstraps itself with itself**, and every piece of infrastructure the protocol depends on becomes a market its own participants can serve.

- **Both directions matter equally.** A driver earning XMR who pays rent in currency needs the off-ramp exactly as much as a rider needs the on-ramp, and a market with only on-ramps starves its supply side within a week. `direction` is a first-class field, not a mode.
- **The rate is negotiated, not oracled.** Unlike a fare (§17.7), an exchange *is* a price negotiation — the counterparty's rate is the product. The payer's app shows it against the market's cached reference rate as a **spread**, not as an error.
- **Physical handoff is the PROOF.** Cash changed hands or it didn't, and both parties co-sign that it did. There is no escrow because there is nothing to escrow. This is the most cash-like profile in the protocol and the most exposed to §9.4's safety floor, which applies here with full force.
- **Regulatory surface is highest here** (§12). Person-to-person cash-for-crypto is the activity most likely to be regulated as money transmission or exchange in a given jurisdiction, and that is true whether or not an operator exists. The protocol takes no fee and routes no funds through anyone, but a *participant* running `exchange/1` as a business is in a materially different position from one taking a ride. The UX must say so, and a real legal opinion is required before this profile is promoted anywhere.

### 7.3 Cross-Profile Mechanics

Four things every commercial profile needs, specified once instead of per-profile.

**Refunds.** *Implemented and tested; see `core/src/wire.rs`.* A2's finality is a property of the *ledger*, not a prohibition on commerce. A merchant issuing a refund is not reversing a transaction — they are making a new, voluntary one. `REFUND` (§6) is an `xfer` bound to a prior receipt: `{ prior_receipt_hash, amount, txid, out_proof, sig }`, partial or full, producing its own co-signed receipt. It is payee-initiated only and can never be compelled; a customer refused a refund has exactly the recourse they have at a market stall today, which is reputation (§9.2).

  **A refund has a window, declared in the offer the payer already signed.** `terms.refund_window` bounds how long a prior receipt can be referenced — without it, "can I refund a two-year-old receipt?" has no answer and a merchant carries an unbounded open liability. Referencing a receipt outside its window is refused with `POLICY_REFUSED`. A window of zero is legitimate and means final sale, provided it was on the confirm screen — which it was, because `terms` lives inside the signed offer.

  **The default window is zero, and that is a decision a client can make by accident.** Default terms grant no refund window at all, so a client shipping them has quietly made every sale final — correct as a default, since a merchant should opt into accepting returns, and silent in a way that is discovered from a customer rather than from a specification. A client MUST surface the refund window on the confirm screen (§15.5) when it is non-zero, and SHOULD say plainly that there is none when it is zero. "No refunds" is a term of the sale, not an absence of one.

  **A refund needs somewhere to go, and nothing carried that address.** A merchant willing to refund had to obtain the payer's address out of band — which is precisely the ambiguity a published attack on Bitcoin's BIP-70 exploited, by substituting the destination a refund was sent to. `ACCEPT` therefore carries an optional `refund_to`, signed by the payer, and a refund must have gone there.

  **It is optional because supplying it costs privacy.** The payee learns a payer address even when no refund ever happens, which §15.10's fresh-subaddress rule otherwise avoids. A payer who omits it has chosen unlinkability over refundability, and no refund is payable to them; a client MUST present that as the trade it is rather than filling the field silently. A merchant advertising a refund window should expect some customers to be unrefundable by their own choice.

  Four checks a signature alone cannot make, since a refund object can be perfectly valid while referring to the wrong thing, too much of it, too late, or to the wrong address: it must have gone to the address the payer signed, it must name **this** receipt by chain-link commitment, it must not exceed the original amount, and it must fall inside the window. The window boundary is **inclusive**, and a refund timestamped *before* its receipt is treated as zero elapsed rather than being allowed to underflow into an apparently-expired window. Building the clawback would mean building the arbiter that can seize funds — that is precisely the party DUCAT deletes.

**Cancellation and no-show.** ABORT is free pre-FUND, which is correct and insufficient — a rider who cancels after the driver has driven ten minutes has imposed a real cost. `CANCEL` covers the post-ACCEPT window and invokes `terms.cancellation` from the offer the payer already signed: a fee schedule, typically time-graded, that was visible on the confirm screen. The fee settles from the canceling party's bond (§17) where one exists. **A cancellation fee is only enforceable against collateral** — against an unbonded counterparty it is uncollectable, and the spec does not pretend otherwise. Providers price that risk through `accept_unbonded` policy (§17.6).

**Standing mandates.** *Implemented and tested.* Rent, dues, a weekly delivery, a subscription. A `MANDATE` is a payer-signed authorization for a named payee to request up to `cap` per `period` until revoked, bound to a persona pair (§16) rather than to a session. Two properties are non-negotiable, and together they are the entire difference from a card-network subscription: the cap is enforced by the payer's *own* client, and **revocation is unilateral, instant, and requires no cooperation from the payee.** You stop honoring requests and that is the end of it — no cancellation flow to navigate, no retention offer, no one to email.

**Multi-payee splits.** §15.8 splits one bill across N payers. The mirror — one payer, N payees — is a band splitting a door take, a courier relay, a driver and a vehicle owner dividing a fare. Monero settles multiple outputs in a single transaction, so this is an offer field rather than new machinery: `payout_split : [ { payto, share } ]`, committed under `offer_commit` and verified by the payer's app before signing. Every payee's share is visible to the payer, which is the honest default — **a split you cannot see is a fee.**
### 7.4 Receipts as Records — Storage, Backup, Export

A co-signed `RECEIPT` is the only record a transaction produces (§1), and §12 argues that is *better* for a merchant than a processor statement. That argument is unpayable unless receipts survive a broken phone and can be handed to an accountant. Neither was specified.

**Storage.** Receipts live in an encrypted local store alongside their transcripts — the chain of `TapPresent`, `FullOffer`, `ACCEPT`, `RECEIPT` that makes each one self-verifying. Storing the receipt alone loses the ability to prove anything about it, so the transcript is the unit of retention, not the receipt.

**Backup is a privacy decision, and MUST be presented as one.** A backup is by definition a copy of the record somewhere else, which is the exact thing §1 claims the protocol removes. So:

- Backup is **opt-in, never automatic**, and never silently to a vendor's cloud.
- Backups are encrypted under a key the user holds, and a client MUST NOT hold a recovery key that could decrypt them.
- The UX states the trade in one line: *a backup means these transactions exist somewhere other than this device.*

**Retention differs by role, and the default should too.** A consumer holding four years of coffee receipts has built a dossier on themselves that the protocol went to some trouble to avoid creating. A merchant needs exactly that dossier for tax. Clients therefore expire consumer-side transcripts on a configurable default and retain merchant-side ones until the operator deletes them, and the difference is a profile default rather than a user setting nobody changes.

**Export.** Two forms, for two audiences:

- **Verifiable bundle** — the canonical transcripts, concatenated and signed. Another DUCAT client can re-verify the whole chain. This is the form that has evidentiary weight (§9.3.2 disputes, §9.2 attestations).
- **Accounting export** — a flat table of date, counterparty persona (or "one-time"), amount in reference currency at `rate_ts`, fee, and profile. This form is *not* self-verifying and is explicitly for humans and bookkeeping software.

**Losing your receipts loses your reputation.** Attestations (§9.2) reference receipts, so a device wiped without backup takes the user's accumulated standing with it — the same failure as O12's persona loss and worth stating in the same breath. This is a real cost of record-absence and the protocol does not soften it.


### 7.5 Memos and Names — knowing what a payment was, and who it was with

A list of twelve entries reading `£4.20` is not a record of anything. `£4.20 —
coffee, Tuesday` is. §7.4 makes receipts the user's own records, and a record
without a human handle on it is a number they will not recognise a month later.
This is the smallest thing that fixes it, and it is deliberately small.

#### Memos

`FullOffer` and `ACCEPT` each carry an optional `memo`: bounded UTF-8, at most
**128 characters**.

**Both objects, not one**, because they are different claims by different
parties. A payee writing *"consulting, March"* and a payer recording
*"reimbursed by work"* are both true, and neither is entitled to overwrite the
other.

Four rules:

- **Advisory only.** §18.1 confines text to fields no decision depends on, and
  this is the archetype. A client MUST NOT route, price, filter, or authorise
  anything on a memo's contents. It is for a person to read.
- **Signed, therefore agreed.** The memo is inside the offer, so it is inside
  `offer_commit`, so a payee cannot edit it after the payer saw it. §15.5's
  confirm screen shows what is signed, and that now includes what the payment
  says it is for.
- **Bounded.** An unbounded text field inside a signed object is a covert
  channel with a signature on it. The bound counts **characters, not bytes** —
  counting bytes silently shortens every language that does not fit one
  character per byte.
- **Never on the chain.** A memo lives in the transcript and stays there. Writing
  one into Monero's `tx_extra` would publish to everyone what the protocol took
  considerable trouble to keep between two people.

#### Names are petnames, and this is not a preference

§15.9 already established the shape: a signature over a static tag proves who
owns an address and never that the tag is the one the venue put there. **A
display name has exactly that property.** A signature over a name proves only
that the holder of a key chose that string, and anyone can choose any string.

So a persona MAY carry a self-asserted display name in its contact card (§16.3),
exchanged in the post-receipt coda — after the transaction has already completed
anonymously, which is the existing ordering and the right one. **The receiver
stores it locally and may rename it.** What a user sees is their own label for a
key they have transacted with, not a claim the network vouches for. Two contacts
may share a display name; they can never share a key.

Two designs are excluded:

- **A global name registry**, because that is a directory, and a directory is the
  thing this protocol deletes. It would also become the chokepoint everything
  else was arranged to avoid.
- **Showing a name on first contact**, because a name displayed before any
  relationship exists is a claim with nothing behind it, and putting it beside an
  amount lends it authority the protocol cannot supply.

This is the Zooko trade taken deliberately: names secure and meaningful **to
you**, given up as globally unique — which is how people already reason about the
contact list on their phone.

**Nobody can be addressed by name.** Reaching a person requires a prior exchange,
and that is true of VeilidChat as well, which uses invitations rather than lookup
for the same reason. Messaging between contacts is a natural extension of the
same rendezvous machinery (§16.4) and is deliberately not required for a payment.


---

## 8. L4 — Settlement (Monero)

Monero has no scripting layer. This is the single hardest constraint in the protocol and it shapes L4 entirely. Four settlement modes ship — direct (§8.1), escrow-multisig (§8.2), escrow-bond (§8.3), and fast (§8.6). Two further tracks (§8.4, §8.5) are research, not modes.

### 8.1 Direct
Consumer sends XMR to a provider subaddress conveyed in QUOTE. No recourse. Correct **only** when delivery is immediate and concurrent with payment (rides, transfers, live file send). Simplest, most cash-like, ships first.

### 8.2 Escrow — 2-of-3 Multisig
Buyer, seller, and a mutually chosen arbiter (§9.3) form a Monero 2-of-3 multisig. Happy path: buyer + seller co-sign RELEASE, arbiter never touches it. Dispute: arbiter co-signs with the party it rules for.

**A participant who loses their device mid-escrow is recoverable, and only through §4.3.** A multisig share is not derivable from the wallet seed — measured — so it must be carried explicitly; it is carried as the wallet's own 2,286-byte key file, which a virgin `wallet-rpc` opens directly (§4.3.3). Without that, "the arbiter co-signs with the party it rules for" fails exactly when it is needed: the arbiter supplies one signature of two, and the other key belongs to the counterparty, who has no reason to sign away their own claim. **A restored participant must re-exchange multisig info before signing**, which is a mechanical step endorsing no outcome — not the adversary's consent. Clients MUST therefore prompt for backup re-export when a ceremony reports ready; an escrow opened after the last export is not in it.

**Known hazards (must be engineered around, not assumed away):**
- **Monero ships multisig disabled by default, and says why.** Quoted from v0.18.5.1's own refusal, because a protocol whose escrow depends on this feature should carry its maintainers' assessment verbatim rather than paraphrased: *"Multisig is an experimental feature and may have bugs. Things that could go wrong include: funds sent to a multisig wallet can't be spent at all, can only be spent with the participation of a malicious group member, or can be stolen by a malicious group member."* Enabling it requires a per-wallet flag set through `monero-wallet-cli`; **there is no RPC method**, so a client must drive the CLI out-of-band or link the wallet library and bypass the RPC. This constrains client architecture more than any fragility does.
- **A wallet can be stranded halfway.** `prepare_multisig` and `make_multisig` both succeed with the flag off; only `exchange_multisig_keys` refuses. The wallet is then multisig-but-unfinalized, and `prepare` rejects it as already multisig. There is no rewind — recovery means discarding it and restarting with all three parties. **Check the flag before step 1**, because nothing checks it after.
- Multi-round key exchange is *not*, in measurement, the fragile part: 2 rounds, 134 s, deterministic (§O1). Earlier drafts treated wallet-sync and key-exchange failure as the primary hazard; that was inherited caution rather than an observation.

**The intended path avoids wallet2's multisig entirely.** DUCAT clients embed a wallet rather than driving `monero-wallet-rpc`, which removes the missing-API problem by construction and — more importantly — allows a different multisig implementation. `monero-oxide`'s `monero-wallet` implements **FROSTLASS**, a formalized threshold signing protocol for CLSAGs, audited by Cypher Stack in May 2025, with O(1) per-signer upload against Monero's native O(n!). The upstream warning quoted above describes *wallet2's* implementation; it is not a statement about threshold signing on Monero in general.

**Measured at 0.50** (`monero-rs/frostlass-spike`), because these claims had been carried on the strength of a README. A **3-of-5 group was funded and spent on stagenet** — a configuration native multisig cannot express — with three of five signers producing a valid CLSAG in **0.088 s**, mined at height 2,183,934. Key generation:

| Group | Share | Key generation |
|---|---|---|
| 2-of-3 | 151 B | 0.10 s |
| 3-of-5 | 215 B | 0.20 s |
| 7-of-11 | 407 B | 0.73 s |

**Arbitrary *t*-of-*n* forms**, which is the result that matters — it is why §9.3's multiple-arbiter escrows and one of O22's candidate directions were unbuildable on wallet2 and are not unbuildable in principle. A share is **linear in *n*** at 32 bytes per participant and independent of *t*; the spike's own first draft claimed fixed size and its output refuted it. Against wallet2's combinatorial key sets, the same 2-of-3 group is **151 bytes here versus a 2,286-byte wallet file** — 15× smaller, and serializable directly, which removes the file-copy workaround §4.3.3 exists for.

Two findings from building it, neither visible from the documentation:

- **Key generation and threshold signing cannot both be taken from crates.io today.** 0.50 recorded that "`dkg` 0.6.1 ships no interactive DKG"; **that is retracted at 0.51.** The DKG was split into its own crate — `dkg-pedpop` 0.6.0 implements PedPoP, the construction the FROST paper specifies, as three rounds with blame assignment. What is actually wrong is narrower and more awkward: **it does not link with the wallet.** `dkg-pedpop` 0.6.0 declares `multiexp` without the `batch` feature its own source requires, so it does not build standalone; and it pins `multiexp 0.4` while `modular-frost 0.11.1` — required by `monero-wallet` 0.2.0 for multisig — pins `multiexp 0.5`, so their `BatchVerifier` types are incompatible. A client must therefore pin a source (the serai workspace, where these are aligned — untested here) or vendor a patched crate. The distinction matters: designing and auditing a DKG is a research project, and verifying a build is an afternoon.
- **The view key is separate shared secret material, and MUST be fresh per group.** The group *spend* key is the FROST group key, whose private half nobody holds — so the view key cannot be derived from it in the usual Monero way and is instead distributed during setup. Every participant can therefore scan everything paid to that group, which is correct for an escrow and fine for a bond. What is **not** fine is the obvious implementation: a client that reuses one view key across groups lets every member of one escrow watch every other escrow it is party to. A fresh view key per group is not an optimisation.

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

Caveats, since these are two samples: measured against a self-route rather than a real `relay/1` peer, sequential and unpipelined, and `app_call` caps payloads near 32 KB so bulk transfer means many calls rather than a stream. O16 is answered well enough to proceed and not well enough to design against. Pulling full blocks over a Veilid route leaks no query pattern, discloses no view key, and exposes no address — the three things a light-wallet server would otherwise learn. **Clients MUST NOT disclose a view key to a remote node.** The restore-height property is what makes that prohibition practical rather than aspirational.

#### 8.7.2 `relay/1`

Node access is a service, so it is a profile — the same move §9.3 makes for arbitration.

A `relay/1` provider advertises that it runs a Monero node and will submit transactions and serve blocks. It is staked (§9.1), optionally paid, and **verifiable**: the payer can confirm the transaction reached the mempool or the chain, so a relay that silently drops traffic is detectable and its stake is slashable. Unlike an arbiter, a relay exercises no judgment — only liveness — which makes it the cheapest possible thing to hold accountable and the easiest dispute class in the protocol.

Relay selection SHOULD be per-transaction and drawn at random from a market's advertised set; a client that always uses one relay has built itself a single observer. Submitting the identical transaction to several relays is harmless — Monero nodes deduplicate — and defeats a single dropping relay outright.

**Relays fail, and not only adversarially.** The public stagenet node this project tested against went down mid-session, between one transaction and the next, with no warning and no error other than `no connection to daemon`. Two alternatives were reachable at the same chain height and the work resumed against one of them. That is the ordinary case, and it means a client MUST hold **several relays and fail over silently**, rather than surfacing an infrastructure outage to a user standing at a counter. A client with one configured relay has an availability dependency it did not choose and cannot see.

The failure is also *loud* in exactly the wrong way: a wallet whose relay has died reports a cached chain height and refuses to refresh, so a naive client shows stale-but-plausible state rather than "disconnected." Clients MUST treat a refresh failure as grounds to rotate relays immediately, not as a transient to retry against the same endpoint.

**This was subsequently reproduced against the market simulator, which is why the rule is now a MUST.** A relay died mid-scan during a funding round. One participant's wallet stopped four blocks short of the chain, kept answering `get_height` with a plausible number, and simply never saw the funds it had been sent — while four other wallets on healthier relays completed normally. Nothing surfaced as an error. The participant appeared, to itself and to the simulator, to be a wallet that had received nothing, and the run stalled waiting for a balance that had in fact already arrived on chain.

**And the same failure exists on the *send* side, which 0.50 hit head-on.** A funding transaction was accepted by a relay, returned a txid, and never propagated: fifteen minutes and four blocks later two independent nodes reported `NOT FOUND` while the sending wallet still displayed `pending`. Nothing had failed from the wallet's point of view — it had a transaction hash and a plausible status.

This is worse than the scan-side case, because a payer holds evidence that looks like a payment. §6.2's 60-second `FUND` deadline is measured against **mempool visibility** precisely so this is caught, but that only works if the payee is scanning; a payer watching only its own wallet sees `pending` indefinitely and has no way to distinguish "propagating" from "dropped on the floor".

Therefore: **a client MUST confirm its own transaction is visible on a relay it did not submit through**, and MUST resubmit rather than wait if it is not. Submitting to several relays at once (above) makes this cheap and is the reason that guidance exists — deduplication means the only cost of redundant submission is bandwidth, while the cost of trusting one relay's acceptance is a payment that never happened. Re-broadcasting through a different node put the same transaction in two independent pools immediately.

**The accountable relay and the private relay are not the same relay, and 0.51 made the tension concrete.** A `relay/1` provider is staked and slashable, which is the whole reason the profile exists — and using one announces that you are a DUCAT user to anyone watching the set, which in a seed market is a few hundred people (O13). A public Monero node announces nothing beyond "a Monero user", an anonymity set orders of magnitude larger, and is accountable to no one.

This session got both halves of that empirically. A public stagenet node accepted two transactions with a success return and propagated neither — precisely the silent-drop behaviour `relay/1`'s stake exists to punish. And the client that caught it did so by querying *other public nodes*, not by holding anyone accountable.

The resolution is that these answer different questions and a client SHOULD use both:

- **Submit through public nodes by default**, and to several. Membership privacy is preserved, and redundant submission — free, since nodes deduplicate — defeats the silent drop without needing anyone to be accountable for it.
- **Reach for `relay/1` when detection is not enough**: when public nodes are unreachable, when a market's policy requires a slashable path, or when a dispute needs a relay's behaviour to be attributable. Accountability is worth buying when you cannot simply route around the failure.

**What this does not do is let a client have both at once.** A transaction submitted to a staked relay is submitted to a staked relay, whoever else also sees it. §8.7.3's guidance to prefer Tor narrows *who* observes the set; it does not change that using the set is the signal. O13 stands.

**Round-trip latency measured, and the spread matters more than the median.** Three `direct` runs over live private routes, timing from tap-read to confirm screen — node startup excluded, since a phone keeps its node attached and §15.3's budget is the user's wait:

| Run | Route import | Round trip | To confirm screen |
|---|---|---|---|
| 1 | 0 ms | 34 ms | 0.03 s |
| 2 | 0 ms | 221 ms | 0.22 s |
| 3 | 0 ms | 297 ms | 0.30 s |

All comfortably inside three seconds, and **the honest reading is the variance, not the best case**: an order of magnitude between fastest and slowest across three consecutive runs on identical hardware. A budget justified by 34 ms would be a budget justified by a sample. Route import was free every time — importing is local parsing, not a network operation, which is worth knowing because it is easy to assume otherwise.

What these numbers do **not** cover: a phone rather than a desktop, a cold node rather than an attached one, cellular rather than wired, more hops than the default, and a route that has to be re-established. Each of those is additive, and the last one is bounded below by a full round trip. The measurement says the protocol is not the problem; it does not say the tap fits on a handset.

**Veilid's own delivery is not reliable either, and a client must treat it that way.** The integration harness ran the same `fast/1` flow twice with no change between attempts: the first lost a single `app_call` — the second round trip of five, on an already-working route — and the second completed all five. Route establishment succeeded both times; it was one message that went nowhere.

A private route is not a connection, and §6.2's deadlines exist partly for this. **A client MUST retry a lost round trip rather than failing the transaction**, and MUST treat a timeout as a transport event rather than as a counterparty refusal — the two are indistinguishable at the API and lead to opposite actions. This bears on the tap budget: §15.3's three seconds must cover at least one retry, or the first flaky message turns into a declined payment at a counter.

Two consequences worth writing down:

- **A height that looks plausible is the failure mode.** Detection requires comparing the wallet's height against the relay's own, because a stalled wallet and a synced one are indistinguishable from the wallet's answer alone.
- **Silent divergence is worse for a payee than a payer.** A payer discovers the problem when a payment fails. A payee shows a customer "not received" for money that is already settled, which is a dispute manufactured out of an infrastructure fault.

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

Arbiters are DUCAT participants running an `arbiter/1` profile. They are named by key in the market descriptor's signed arbiter set (§10.1) and selected from it before FUND — **never from an address supplied in a message, which is precisely how RetoSwap was drained (§2.5)**. They see only what disputants disclose (T5). **Multiple arbiters cannot co-sign on the path this project has actually tested**, and earlier drafts said they could. Monero's native multisig — wallet2's, the one measured throughout `monero-spike/` — supports only **N-of-N and (N−1)-of-N** schemes. 2-of-3 exists; 3-of-5 does not. A higher-value escrow wanting two arbiters plus buyer and seller is 3-of-4, which is (N−1)-of-N and therefore buildable, but anything needing a smaller threshold than N−1 is not. This also removes one of O22's candidate directions, since an arbiter-heavy composition that lets two arbiters execute a ruling without the counterparty is exactly the shape wallet2 cannot express. FROSTLASS (§8.2) does arbitrary *t*-of-*n*; until a market runs it, the arbiter-count knob is narrower than it reads. This dogfoods the protocol: the dispute layer is itself a P2P service market, with no central court, only a competitive field of stakers whose own bonds are slashable.

#### 9.3.1 Two classes of dispute, and only one needs judgment

The distinction governs everything else, because it decides whether an arbiter is checking arithmetic or weighing testimony:

- **Mechanical.** The claim is decidable from the signed transcript plus public chain state. *Did this transaction confirm? Does a conflicting key image exist?* Every `fast/1` slash claim (§17.5) is of this class. The transcript is self-verifying (§6, Part V), so the arbiter checks signatures and queries the chain — no discretion, no testimony, and a wrong ruling is **provably** wrong because the chain disagrees.
- **Judgment.** The claim turns on facts neither party can prove. *Was the room clean? Was the task complete?* Escrow disputes over `lodging/1` and `task/1` are of this class. An arbiter here is weighing disclosed evidence, and a wrong ruling is not provable — only unpopular.

**Only the mechanical class is in scope for Phase 3.** It is what bonded fast settlement needs, it is automatable, and its arbiters are accountable in a way judgment arbiters are not. Judgment arbitration ships with escrow or not at all, and §14's O6 — what counts as "done" — is the reason it cannot ship sooner.

#### 9.3.2 Messages

```
DISPUTE    claimant → arbiter set   { transcript, class, claim, evidence?, sig }
EVIDENCE   either → arbiter set     { dispute_hash, disclosed_material, sig }   (judgment class only)
RULING     arbiter → both parties   { dispute_hash, outcome, awarded_pxmr, sig }
```

- **`DISPUTE` carries the transcript, not a story.** Signed `ACCEPT`, `TXID`, `RECEIPT`, and the chain link between them. For a mechanical claim that is the entire case; the arbiter needs nothing the claimant asserts.
- **`EVIDENCE` is voluntary and one-way.** Disclosing it to an arbiter is a privacy decision the discloser makes (T5), and there is no compulsion, no discovery, and no penalty for withholding beyond losing the point it would have made.
- **A `RULING` is not an instruction — it is a co-signature.** The arbiter's authority is exactly its key in the bond multisig (§17.2). It does not order a transfer; it signs one, and the ruling object is the audit record of why. Nothing enforces a ruling that the arbiter did not itself sign, which means an arbiter cannot rule beyond the funds it can already move.

#### 9.3.3 Accountability, and its honest limit

An arbiter's own stake is slashable for **provable** misconduct — and per §9.3.1 that word only bites for mechanical claims, where the chain contradicts the ruling. A market's arbiter set can be slashed by its own peers on that evidence.

For judgment claims there is no such proof, so the only accountability is reputational: participants leave a market whose arbiters rule badly, and §10.1 makes leaving a matter of ceasing to read a keyspace. **That is weaker than a court and should not be described otherwise.** It is roughly the accountability a market stall has, which is the comparison §9.4 already draws.

Two further limits worth stating rather than discovering:

- **Arbiters are paid per dispute, and the fee comes from the disputed amount** before disbursal, with the schedule published in the market descriptor. An arbiter paid by one side is not neutral, and an arbiter paid nothing does not answer.
- **O8 remains open.** A captured arbiter set can rule dishonestly on mechanical claims and merely be caught afterwards; it can rule dishonestly on judgment claims and not be caught at all. §10.1's descriptor chain proves continuity, not honesty, and nothing here changes that.

#### 9.3.4 Timeouts

A dispute that never resolves is worse than one resolved badly, because the funds stay frozen. Each stage is bounded and expiry has a defined outcome, on the same principle as §6.2:

| Awaiting | Default | On expiry |
|---|---|---|
| Arbiter acknowledges `DISPUTE` | 24 h | Escalate to the next arbiter in the set |
| `EVIDENCE` from either party | 72 h | Rule on what was disclosed |
| `RULING` after evidence closes | 72 h | Escalate; the silent arbiter's stake is at risk |
| Whole dispute, all escalations | 14 d | **Arbiter rules for the respondent**, award zero — see below |

The final row is the uncomfortable one: **an unresolvable dispute resolves against the claimant**, because the alternative is indefinitely frozen funds and there is no higher court to appeal to. A protocol without an operator has no one to escalate to when its own dispute mechanism fails, and that has to be a stated outcome rather than a hang.

**It also has to be an *action*, and an earlier draft got this exactly backwards.** That row previously read "funds return to the pre-dispute allocation, claim abandoned." Under escrow the pre-dispute allocation *is* funds sitting in a 2-of-3 awaiting a `RELEASE` that two disagreeing parties will never co-sign — so returning to it and doing nothing freezes them permanently, which is precisely the outcome this timeout exists to prevent. Abandonment must therefore emit a **real ruling**: for the respondent, award zero, signed by an arbiter and co-signable against the escrow. A ruling moves funds; an absence of one does not.

The general form is worth stating, because it will recur wherever this protocol has a timeout: **in a system with no operator, "nothing happens" is not a safe default.** Every deadline must name the action it triggers, because there is nobody to sort out the mess afterwards.

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
- **Joining a market you have bonded into is a financial commitment, not just a subscription.** The arbiter set holds two of three keys on your deposit (§17.2), so a market that dies takes its bonds with it and a market that is captured can take them deliberately. Leaving is free only until you have posted a bond.
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

**The arithmetic is the pitch, and it is genuinely favourable.** A platform taking 25% of a $20 fare leaves the worker about $15. Price the same trip at $17 and the worker clears $17: **the consumer pays less and the worker earns more**, funded entirely by deleting the intermediary rather than by squeezing either side. There is no growth-subsidy phase to unwind and no investor expecting the take rate to rise later.

**The honest half.** That cut is not pure rent. It also buys insurance, trust-and-safety staff, dispute handling, background checks, and consumer acquisition. This protocol replaces matching (§5.2) and payment processing (§8) at near-zero marginal cost, and it explicitly does **not** replace the safety half (§9.4). So the comparison is sound on money and incomplete on services, and any client presenting it MUST present both halves — a fare comparison that omits what the platform was also buying is an advertisement, not an argument. The gap is narrowest for the no-exposure profiles (§9.4) where those services were buying little to begin with: a merchant taking a coffee payment was never getting trust-and-safety for their interchange fee.

Workers keep 100%; therefore the protocol earns 0% by default. But security-critical money-moving software that goes unmaintained is a wallet-drainer. Funding, in preference order:

1. **Removable maintenance tip** — default 0.5–1%, one tap to zero out, always visible. Precedent: Monero mining dev-fee conventions. Honest and opt-out beats hidden and mandatory.
2. **Grants / donations** — the Veilid and Tor model.
3. **Never**: a protocol-level mandatory cut, a token pre-mine, or venture capital that needs a rent-extraction exit. Any of these reintroduces the middleman the protocol exists to delete.

**A published client is a supply-chain target, and the project now has an address to attack.** Anything distributed from `ducatproject.org` MUST be reproducibly built where possible, signed by a key published independently of the site, and accompanied by hashes a user can verify before running it — the convention Monero follows, and which this project used when verifying its own toolchain. Verifying a hash over HTTPS from the same host that served the binary proves very little; the signature is the part that matters, and a project that tells users to check hashes should make the signing key easy to obtain from somewhere else. A domain hijack that serves a wallet-draining lookalike is §2.5's lesson moved one layer up, and it is cheaper to mount than any attack in the threat model.

Multiple independent client implementations are a design goal: a protocol with many clients can't be acquired or shut down like an app, and it keeps the spec honest. That goal is unreachable on behavioral description alone — it requires pinned bytes, a normative state table, strict rejection, and a vector set a second implementation can fail against. **Part V (§18) is what makes this claim payable rather than aspirational**, and a client count of one means the spec has not yet been tested.

---

## 12. Regulatory Posture (not legal advice)

- **Spec author ≠ operator.** Publishing a protocol and reference client is a different posture from running infrastructure or sitting in the flow of funds. DUCAT is built so no one *is* an operator.
- The `xfer` (person-to-person money movement) profile is the component most likely to read as money transmission if anyone facilitates it for a fee. The protocol takes no fee on it and routes no funds through any operator — decisions made deliberately to keep it self-hosted-software rather than a service.
- **`exchange/1` (§7.2) now carries the highest surface of any profile**, and it differs in kind from the others: cash-for-crypto is regulated activity in many jurisdictions *at the level of the participant*, independent of whether an operator exists or a fee is charged. Publishing the profile is a different act from running it as a business, and the client should say so where a user enables it.
- **A published fare rate would be price coordination; a published currency rate is not.** If a market published a suggested *fare* — "$2.40/km here" — and independent providers followed it, that is competitors aligning prices through a common mechanism, and a protocol whose participants are genuinely independent contractors is arguably *more* exposed to that reading than a single firm setting its own prices. This document therefore publishes no fare guidance at any layer. §17.7's market-published **exchange** rate (XMR against a reference currency) is a different object entirely and stays: it converts between units, it does not suggest what to charge. Price discovery is already a property of §5.2, where a consumer receives multiple sealed offers and picks one — a sealed-bid auction per transaction, which also resists tacit collusion better than any published number, since providers cannot see each other's bids.
- **Receipts cut the other way, and that is a feature.** `pos/1` produces a co-signed record the merchant holds and no one else does (§7.1). A merchant with tax obligations is better served by that than by a processor statement, and the protocol neither reports it nor prevents its disclosure. Record *absence* is a property of the network, not a constraint on what participants voluntarily keep about their own trade.
- These are design constraints chosen to reduce regulatory surface; they are not a legal opinion and a real one should be obtained before any launch.

---

## 13. Build Order

Grounded in what current primitives actually support (veilid-core 0.5.x is stable; Monero multisig is not):

**Phase 0 — Measure before building (cheap, blocking)**
0a. **Done** (O11). Measured on veilid-core 0.5.7: `token` mode 190 B, `inline` mode 877–1669 B and non-monotonic in hop count. §15.3.1 carries the numbers; §15.3.2 specifies measure-and-degrade instead of a budget.
0c. **Done, and it found the worst result available** (O17). Veilid #395 is open against milestone 0.13.0 while the current release is 0.5.7. Remote hail (§5.2) should not ship until that lands. Proximity profiles proceed with the residual documented in §15.10.
0b. **Done** (O16). 135–204 KB/s at 32 KB payloads over a private route, latency-dominated (§8.7.1). Adequate for incremental sync from a restore height of now; inadequate for a full chain, which the design already avoids. Re-measure against a real `relay/1` peer before Phase 3.

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

- **O1. wallet2 measured end to end; FROSTLASS now measured too, and the gap moved.** A native 2-of-3 ceremony converged in 2 rounds and 134 s, and a funded bond was seized by `arbiter + recovery` with the user's wallet never contacted — the mechanism is proven on wallet2. **What 0.50 established is that wallet2's threshold limitation is not Monero's**: native multisig admits only N-of-N and (N−1)-of-N, while FROSTLASS forms 2-of-3, 3-of-5, 2-of-5 and 7-of-11 in well under a second, with shares linear in *n* rather than combinatorial (151 B for 2-of-3, against a 2,286-byte wallet2 key file). **Signing is now measured too**: a 3-of-5 group was funded and spent on stagenet, three of five signers producing a valid CLSAG in 0.088 s, mined at height 2,183,934. **The remaining risk is key generation, and it is a packaging problem rather than a research one.** An interactive DKG exists — `dkg-pedpop`, PedPoP, three rounds with blame assignment — but it does not link with `monero-wallet` as published: it declares `multiexp` without the `batch` feature it uses, and pins `multiexp 0.4` against `modular-frost`'s 0.5. So a client must pin a source or vendor a patch, and the spike's keys still come from a dealer who holds every share. Also open: FROSTLASS and wallet2 groups cannot co-sign, so a market's declared scheme is load-bearing (§10.1), and `monero-oxide` remains pre-1.0.
- **O2.** Offline settlement (A5) has no trust-minimized answer yet.
- **O3.** **DHT harvesting — largely retired by §5.2's inversion.** Providers no longer advertise; they watch. A harvester learns nothing about supply, only that someone hailed from a coarse cell at a time — ephemeral and unattributed, rather than a standing dossier of who works where. What remains: hail traffic is observable in aggregate, and hail spam is unsolved (rate limits are cheap, stake is strong but pulls consumers toward collateral, per O15).
- **O4.** Reputation vs. unlinkability is a genuine trade with no free lunch (§4, §9.2).
- **O5.** The safety floor (§9.4) structurally caps the addressable market **for high-exposure profiles** — rides, lodging, open-ended tasks. Scoped in 0.7: it does not bind the no-exposure tier (`pos`, `xfer`, `goods`, `file`, `chat`), which is where most of §1's addressable surface now lives. The cap is no smaller where it applies; it simply applies to less of the protocol than first stated.
- **O6.** Open-ended PROOF for `task/1` (what counts as "done"?) resists specification.
- **O7.** Cold start still requires a real, motivated seed community; the protocol enables it but cannot manufacture it.
- **O8.** **Arbiter-set governance — mechanism specified, honesty still unenforceable.** §10.1 gives self-certifying `market_id` and threshold-signed rotation chained from genesis; §9.3 gives the dispute protocol, and §9.3.3 draws the line that matters: a **mechanical** ruling contradicted by the chain is provably wrong and slashable, a **judgment** ruling is neither. A captured set can therefore be caught late on mechanical claims and never on judgment ones. Since a market's arbiter set now also holds two of three keys on every bond it custodies (§17.2), capture is a custody risk and not only an adjudication one. Bounded by keeping bonds small; not closed.
- **O9. Hot-wallet exposure — now quantified, and bounded from *below*** (§17.2, §4.4). "Mitigated by keeping it small" was true and incomplete: **the float has a floor set by how the user wants to transact, not by how much risk they will accept.** §17.2 makes consecutive capacity a count of unlocked outputs, and `sim --drain` measured roughly 1.5 outputs consumed per payment, so wanting *k* payments before a top-up means exposing about `1.5k × typical payment` — there is no way to hold less and still spend that often. `core::float` computes it and refuses to let a stated risk cap and a stated usage pattern silently contradict each other, which they otherwise do until the user is at a counter. §4.4's hardware reserve is what makes the floor tolerable, since it applies to the float alone. **Not eliminated:** the float remains malware- and seizure-reachable, and the floor means it cannot be shrunk to nothing by anyone who intends to use the thing.
- **O10. Half closed: the capacity side channel is bucketed (§17.8); oracle integrity is not.** `capacity_remaining` published an exact balance to every provider a rider tapped, which is a running meter on their spending — two merchants comparing notes recover what was spent between taps. It is now `capacity_bucket`, the floor of a fixed 1–2–5 ladder, rounding down so a bond can never overstate solvency, with ladder membership enforced on receipt because an arbitrary integer in that field would let a rider publish their balance exactly and call it a bucket. The leak falls from 64 bits to under 4.1, at the cost of a real false negative: a rider just below a rung can pay and cannot prove it. **Price-oracle integrity remains open** and is the harder half — a manipulated rate moves every reference-denominated threshold at once, including §15.5.1's verification tiers, which is why a stale rate escalates rather than relaxes.
- **O11.** **Route-blob size — measured, and it resists being a constant.** Phase 0a: `token` mode is exactly 190 B; `inline` mode ranged 877–1669 B for a `TapPresent` across two runs, non-monotonic in hop count, with within-hop-count spread exceeding between-hop-count differences. Size tracks *which peer was selected*, not how many hops were asked for. §15.3.1 therefore specifies measure-and-degrade rather than a budget. Closed as a question; the variance itself is the finding.
- **O12. (closed in 0.42, §4.3.)** Persona loss and recovery. Resolved by encrypted export/import rather than social recovery, at the cost of making persona keys software-held (§4.1) — a trade justified in that section. **The residue is key rotation:** a user who believes a backup was *compromised* rather than lost has no way to tell existing contacts "this persona now has a different key". Backup answers device loss; it does not answer key exposure, and signed rotation announcements to existing contacts remain an unsolved sub-problem of L1.
- **O13. Relay anonymity set — the trade is now stated rather than merely flagged** (§8.7.2, §8.7.3). Using a `relay/1` provider announces "DUCAT user" to anyone watching the set, which in a seed market is a few hundred people; a public Monero node announces only "Monero user" and is accountable to nobody. 0.51 got both halves empirically: a public node accepted two transactions with a success return and propagated neither, and the client that caught it did so by querying *other public nodes* rather than by holding anyone accountable. So the guidance is now explicit — **submit through several public nodes by default**, since redundant submission is free and defeats a silent drop without accountability, and reach for `relay/1` only when routing around the failure is not possible or when a dispute needs behaviour attributable to a stake. **Not solved:** a client cannot have both at once, and preferring Tor narrows who observes the set without changing that using the set is the signal. Still compounds O7 — the smallest markets have the thinnest liquidity and the thinnest cover.
- **O14. Both wallet-layer gaps re-measured against 0.2.0; one is much smaller than recorded** (`monero-rs/REPORT.md`). **Proofs:** confirmed absent — the only proof-shaped symbol in the crate is `Bulletproof::prove_plus`, a range proof. But the obligation is narrower than "DUCAT must build this": `monero-wallet-rpc` exposes `get_tx_proof`/`check_tx_proof`, and §17.5's arbitration path was measured working through them, so the work is owed only by a client that *embeds* a wallet (§8.2's intended path). **Unconfirmed scanning:** the premise was that `scan_transaction` is private and `Scanner::scan` takes a block. The first half is true; the conclusion is not. `ScannableBlock` is re-exported with all three fields public, so a caller wraps the mempool transaction in a synthetic block and calls the public scanner — **verified by compiling against the real crate** (`monero-rs/mempool-probe`). What compiling does *not* establish is correctness: whether a synthetic block scans an unconfirmed transaction accurately, and what `Scanner::scan` reads out of a fabricated header, are both open and answerable only against real data. The gap moves from *blocked* to *a public path exists and needs an empirical test*.
- **O15. Detection implemented (§17.3); the hazard is narrowed, not closed.** Staying standard rather than adopting `monero-wallet`'s "guaranteed" outputs remains right — its own source says they are *"not officially specified by the Monero project … No support outside of monero-wallet is promised"*, and funds only one implementation understands are not bearer funds (A1, §11). What was missing was the detection half, and stating it as "detect duplicate one-time keys" understated the fix: **the bug is arithmetic.** A merchant expecting 1 XMR receives two outputs of 0.5 to the same one-time key; `sum()` reports 1.0, the goods leave, and one coin is spendable. The rule is therefore that **outputs sharing a one-time key count once, at the maximum, never summed** — `sum()` *is* the bug. Implemented in `core::burning`, which also surfaces a burn that still covers the price, since a duplicate one-time key does not occur by accident and is evidence about the counterparty. **Residue:** detection protects the recipient's accounting and cannot stop a sender constructing the transaction, and §15.10's fresh-subaddress rule still only narrows the window — the sender picks the transaction key, so it can drive two outputs to one key inside a single transaction however fresh the subaddress is.
- **O16.** **Veilid at sync volumes — measured, provisionally adequate.** Phase 0b: 135–204 KB/s at 32 KB payloads, latency-dominated so throughput scales with request size (§8.7.1). A day of blocks moves in minutes; a full chain would not, which is why §17.1's restore-height-of-now is load-bearing. Two samples, self-route, unpipelined — enough to proceed, not enough to design against. Re-measure against a real `relay/1` peer before Phase 3. **Version drift, noted at 0.52:** those figures were taken against `veilid-core` 0.5.3 and the crate is now 0.5.7 (four releases, 2026-03 to 2026-07). The measurement is not invalidated, but it is no longer current, and Phase 0b should be re-run before any figure from it is treated as a budget.
- **O17. Veilid #395: contained, and it should be designed around rather than waited for.** §5.2's inversion means matching runs entirely over DHT reads — which import nothing — and the single remaining import happens after mutual selection, by the party that chose to initiate. Exposure is one per real transaction to a chosen counterparty, the same posture the tap already carries (§15.10), rather than one per browse to anyone watching. **Checked upstream at 0.52 and the timeline is worse than "pending":** the issue has been **open since July 2024**, its milestone is *Release 0.13.0 — Private Routing 2.0*, **due 1 March 2027**, and `veilid-core` is at 0.5.7. A milestone named "2.0" two years out is a redesign, not a patch. **DUCAT should therefore treat the containment as the answer and not as a stopgap** — every design decision that would become sound only once #395 lands should be assumed unsound for the life of this document. That is already true of remote hail; it is worth stating so nothing new is built on the assumption.
- **O18.** **Cancellation fees erode the permissionless lane.** §7.3 makes no-show fees enforceable only against collateral. The pressure this creates — providers preferring bonded counterparties precisely because cancellation *costs* them something — pushes the network toward the collateralized lane and quietly hollows out the slow permissionless one A4 depends on (§17.6). Whether the unbonded lane survives contact with real no-show rates is an empirical question no amount of spec work answers.
- **O19.** **iOS cannot present over NFC, permanently.** Apple's HCE entitlement is conditioned on EEA establishment, organization enrollment, and financial-regulatory standing (§15.3.2) — structurally incompatible with A4, and not a hurdle an open protocol clears. The best-UX medium is therefore available to roughly half the supply side, and QR carries the rest. This is outside DUCAT's control and will not improve through protocol design; it is stated so no one plans around a tap that cannot exist.
- **O20. (closed in 0.48, §18.7.)** Transport identifiers assigned. The NFC AID is `F0 44 55 43 41 54` (`0xF0` ‖ `"DUCAT"`), and the "pending real RID registration" caveat was mistaken — ISO/IEC 7816-5 reserves the `0xF…` range for **proprietary identifiers requiring no registration at all**, which is what Android HCE documents for exactly this case. There was nothing to wait for. BLE takes one random 128-bit service UUID and three characteristics sharing a base; the Bluetooth SIG registers only 16-bit UUIDs, and the 128-bit space exists so anyone can allocate without asking. **Residue, and it is a real one:** no registry means no uniqueness guarantee, so nothing prevents another vendor choosing the same AID bytes — mitigated by using the full name rather than the four-character contraction, since AIDs may run to 16 bytes and the 5-byte minimum was never a maximum. The L2CAP PSM stays deliberately unassigned: LE CoC PSMs are allocated dynamically by the local stack, so a spec pinning one would pin a value it does not control; it is published in a characteristic and read.
- **O21. Conformance suite exists, schema published, second implementation runs it (§18.9.1, §18.11).** 367 vectors, every case carrying a `kind` that is the sole discriminator, validated against a **hand-written** `schema.json` — hand-written because a schema emitted by the generator would agree with the generator's mistakes, and it earned that by catching two defects on its first run. A second implementation written from Part V agreed on 101 cases and disagreed on 3, **all three defects in this document**, of which the important one was negative integers being *unspecified* — the reference accepted them, the second implementation refused them, and both were conformant, a divergence no vector set could detect because there was no correct answer to test against. 104/104 after correction. Most of the second implementation's effort went into the harness rather than the protocol; that friction is now removed (§18.11). **Still not closed, and what remains cannot be engineered away: an implementer who has never read `core/`.** Everything accidental has been cleared out of their way — a normative case schema, one event encoding instead of five, `why` required on every case, and two commands that validate any change. The gap is authorship.
- **O22. (closed in 0.44, §4.3.3.)** An escrow participant who loses their device. Resolved once the question was asked correctly: a share cannot be *reconstructed* — measured, `prepare_multisig` draws 88 characters of fresh randomness beyond what the wallet keys determine — but it does not need to be, because it is already a 2,286-byte file that a virgin `wallet-rpc` will open directly. The recovery ask therefore moved from **the counterparty's signature**, which no protocol can compel from an adversary, to **the other participants re-sharing multisig info**, which endorses no outcome and is a step every participant performs routinely. **Residue:** a stale bundle still cannot recover an escrow opened after it, so this now depends on a client prompting for re-export at ceremony completion — a UX obligation rather than a protocol impossibility. And an end-to-end spend from a restored share is still undemonstrated (§4.3.3's last limit).
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

**Both directions now run end to end (0.56), and the inversion was untested until then.** The payee-presented case — merchant holds out a terminal, customer taps — is the one every earlier harness exercised. The payer-presented case, where the customer shows a code and the till scans it, existed only as an enum variant: **nothing had ever built one, despite §15.3.2 depending on it as the escape hatch for an entire platform.**

Building it surfaced an asymmetry the field name hides. The presenter supplies *reachability*, so **the reader drives every round trip** — and in the inverted direction that means the till must **poll** the customer, because the customer holds the route and cannot call out. Three consequences, none of them obvious from the grid above:

- **A presenter's message loop must stay responsive while it settles.** The first implementation paid inline, blocking for up to forty seconds of propagation retries, so the till's polls went unanswered and it abandoned a sale for a transaction that had in fact been broadcast. Settlement belongs off the loop.
- **`amount_authority` MUST be `open`.** A customer's phone does not know the price of a coffee, and a payer-presented tap has no offer to commit to — `offer_commit` is necessarily empty, because the offer does not exist until the till makes one. A reader MUST refuse a payer-presented tap that claims otherwise.
- **The human checkpoint does not move.** §18.4.1(1) still admits `ACCEPT` only from the payer, and §15.5's confirm screen is still the payer's. **Whoever held out a phone, the party whose money is at risk decides** — and that invariant is the whole reason this inversion is safe to offer.

A UI built on the payee-presented flow will assume symmetry and be wrong: one direction needs no polling and the other cannot avoid it.

**This promotes `presenter_role` from convenience to load-bearing.** §15.2 observes that the presenter is almost always the payee — the driver, the merchant, the busker. That is the supply side, and on iOS it cannot present over NFC. Two escapes, both already in the protocol: invert the roles so the payer presents (works when the payer is on Android), or fall to QR (works always). An iPhone merchant serving iPhone customers is a QR deployment, and the protocol should say so rather than let an implementer discover it at a market stall.

**Static tag sizing, measured twice.** A bare `TapStatic` (§15.9) fits the commodity NTAG213's 144 bytes. A `TapPresent` does not: `token` mode is 217 B sealed and needs an NTAG215, and `inline` mode **no longer fits any commodity tag at any hop count** — the encoded 1-hop object is 915 B against an NTAG216's 888. The earlier 2-byte margin was an artifact of counting payload rather than encoding. **Tags ship tokens**, and there is no longer a case to argue.

**Securing the BLE channel.** When the session runs over BLE rather than Veilid — offline, or while a route is still building — the channel needs its own encryption, and the spec previously left this unstated. DUCAT adopts **`Noise_XX_25519_ChaChaPoly_SHA256`**: mutual authentication and forward secrecy, with the presenter's `session_pk` from the `TapPresent` as the expected static key, which binds the BLE session to the bootstrap that started it. This is the same construction bitchat uses for live BLE sessions, and reusing a Noise pattern with existing cross-platform implementations is the point. Note the negative lesson from the same source: bitchat's *offline* courier envelopes use the `X` pattern and knowingly give up forward secrecy — acceptable for undelivered chat, not for a payment authorization sitting in someone's outbox. DUCAT's store-and-forward work (§8.4) MUST NOT inherit that trade.

---

## 15.4 `FullOffer` — what flows over the channel

Delivered by the presenter immediately after the channel opens; the reader checks `H(FullOffer) == offer_commit` before showing anything to the human.

| Field | Meaning |
|---|---|
| `terms` | cancellation fee, refund window, meter cap and duration limit, minimum fee tier — see below |
| `payto` | **fresh** Monero subaddress, one per tap (unlinkable on-chain; never reuse) |
| `amount_pxmr` | the number, when `amount_authority = fixed` — **unsigned integer piconero**, never a decimal (§18.2) |
| `rate_card` | presenter's signed rate card (or hash + fetch ref) — lets the reader *reproduce* the price |
| `route_inputs` | for rides: origin, `dest` (must equal the tap's `dest`), distance, duration |
| `breakdown` | how `amount_pxmr` was derived: base + per-km + per-min, etc. |
| `persona` | optional long-lived key + its stake proof + attestation pointer (trust surface, §15.6) |
| `terms` | profile-specific (cancellation, escrow flag, meter rate) |
| `sig` | presenter session-key signature over the whole `FullOffer` |

**`terms` is a single nested map, and it exists because several requirements referenced it before anything carried it.** §7.3's cancellation fee and refund window, §15.7's mandatory meter cap and duration limit, and §8.8's minimum fee tier were all specified as `terms.*` while `FullOffer` had no such field — rules about a field that did not exist, which no conforming client could obey. Its inner keys are their own namespace:

| Field | Section | Meaning |
|---|---|---|
| `cancellation_pxmr` | §7.3 | Owed if the payer cancels after `ACCEPT`, before `FUND` |
| `refund_window_s` | §7.3 | How long this receipt may be referenced by a `REFUND`; zero means final sale |
| `meter_cap_pxmr` | §15.7 | **Required when `amount_authority = rated`** |
| `meter_max_s` | §15.7 | **Required when `amount_authority = rated`** |
| `min_fee_tier` | §8.8 | Below this the payee refuses with `POLICY_REFUSED` |

Because `terms` is inside the signed offer, altering it after the fact breaks `offer_commit` exactly as altering a price does — a presenter cannot quietly shorten a refund window between the tap and the confirm screen.

**The two meter fields are a pairing rule, not a field rule.** Whether they are required depends on `amount_authority`, which lives in `TapPresent` rather than `FullOffer`, so it cannot be enforced by parsing either object alone. A client MUST check the pair before rendering a confirm screen.

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

**One exception, and it is a moved checkpoint rather than a missing one.** §7.3's standing mandates authorise payment *without* a per-payment confirmation — that is their entire purpose, and it flatly contradicts the paragraph above as it was originally written. The resolution is that the human checkpoint happens **once, at mandate creation**, and what they confirm is not an amount but a **cap and a period**. Every later draw is bounded by what they saw and signed.

This holds only because of three things, and a mandate missing any of them is a blank cheque signed once:

- **A cap and period are structurally required.** A mandate declaring neither is not merely refused, it is unparseable — so one cannot exist in a client's store to be honoured by mistake.
- **The cap is enforced by the payer's own client** (§7.3). A cap the payee enforces is not a cap, it is a promise.
- **Only the named persona may draw**, or the mandate is bearer paper that anyone holding it can spend.

Periods anchor to the first draw rather than to a calendar, which keeps timezones out of the protocol and means no global midnight at which every cap in a market resets at once.

---

### 15.5.1 Payer Verification — the question WYSIWYS never asked

**This section was referenced from 0.41 and never written.** The rule was implemented and tested (`core/src/verify.rs`); the normative text was missing, so an implementer following the reference found nothing. Found by §18.12's audit, which exists because that is not a mistake a careful reading catches.

§15.5 establishes that a payer sees exactly what they sign. It says nothing about **whether the person holding the device should be signing at all.** A stolen unlocked phone is a bearer instrument: A1 working as designed, and also how people lose money.

EMV's answer is proportionality — no verification below a floor, stronger verification above it — and this takes the same shape with the thresholds moved into the user's hands.

#### Tiers

| Tier | Meaning |
|---|---|
| `None` | Tap and go, as contactless does below its floor limit |
| `DeviceUnlocked` | The OS reports the device unlocked — biometric or passcode, satisfied **passively** and possibly some time ago |
| `AppSecret` | A secret entered **into this application, deliberately, recently** |

**The gap between the last two is the load-bearing one and is easy to collapse by accident.** A device unlocked twenty minutes ago is a passive fact that a thief holding the phone already satisfies. A secret entered into this app just now is an active knowledge factor they do not have. A client that treats "unlocked" as sufficient at every value has built a bearer instrument with extra steps.

#### Policy

Thresholds are **user-settable** and denominated in the **reference currency's minor units**, never piconero — a threshold stored in piconero silently drifts every time the rate moves, so a "$100 limit" quietly becomes a $70 one after a price rise.

Four rules are **not** the user's to relax:

1. **Thresholds must ascend.** A policy where a larger payment demands less than a smaller one is rejected at construction, not quietly normalised.
2. **In-app secrets expire.** Without a validity window, "deliberate" decays into "happened at some point today."
3. **Velocity counts alongside per-payment value**, and the payment under consideration is included in the window — otherwise the transaction that crosses the line is the one that gets through. A per-transaction limit alone does not stop twenty payments just under it, which is how a lifted phone is actually drained.
4. **A stale exchange rate escalates to the strongest tier** (§17.7). The thresholds are denominated in real money, so without a trustworthy rate the client cannot know which rung it is on. Failing the other way would let anyone who can stall a rate feed *lower* the verification requirement, turning a liveness problem into a security one.

#### This never touches the wire

Verification is evaluated entirely by the payer's own client. **The payee never learns which tier was satisfied and cannot request one.** A counterparty able to influence verification would ask for the weakest, which is a downgrade attack EMV spent years patching. A payee's only options remain accept or decline.

The consequence for §4.3 is that these thresholds are safe to carry in a backup: they are the user's instruction to their own client and nothing else.

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

**Every meter is bounded at `start`, because an unbounded one cannot be consented to.** The payer confirms three things, not one: the rate, a **cap** on the total, and a **maximum duration**. A confirm screen offering an open-ended obligation is not informed consent, and §15.5's whole argument fails if the number the payer approves is "whatever it turns out to be." Clients MUST reject a `start` carrying no cap.

**And meters get abandoned.** The customer leaves the bar, the rider walks away from the scooter, the passenger jumps out. There is no `stop`, so there is no co-signed total, so there is nothing to settle against. The resolution is uncomfortable and is the same one cash businesses already live with:

- At `max_duration` the meter **auto-stops** and the provider computes what was owed, capped at the declared cap.
- With no `stop` co-signature, the provider emits a **single-sided receipt** (§6.2) recording what accrued. It proves what was authorized and what was metered; it does not prove the payer agreed to the total.
- **Collection then depends entirely on collateral.** Against a bonded payer (§17.2), the accrued amount is claimable up to capacity like any other obligation. Against an unbonded one it is **uncollectable, and the provider bears the loss** — precisely as a bar bears a walked tab today. No protocol mechanism recovers money from someone who left and signed nothing.

This is the clearest case in the document where `rated` mode wants a bond, and providers running metered services SHOULD set `accept_unbonded` (§17.6) with that in mind rather than discovering it after a busy night.

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

**Persona pinning is weaker than earlier drafts implied, and the difference matters.** This section suggested pinning a persona and warning on an unrecognised one. An attacker who replaces the physical tag replaces the persona too, so that warning fires only for a payer who already knows which persona to expect — for a stranger tapping a donation box it fires never.

A pinned persona is therefore worth nothing on its own: it is a *claim*, and an attacker can print a charity's name over their own address. A `TapStatic` MUST carry a **signature by the pinned persona over the address**, which raises it from a claim to evidence: `payto` provably belongs to that persona, and substituting the address under a borrowed name fails. A persona pinned without a signature MUST be reported to the payer as unauthenticated rather than shown as an identity.

What that still does not fix, stated plainly rather than left to be discovered: an attacker who replaces the whole tag supplies their own persona *and* a valid signature over it, and the result verifies. The only remaining defence is that it is a **different** persona than expected, which protects a repeat donor or someone who learned the persona out of band, and nobody else.

Use static tags only where the worst case of a swapped tag is "money went to the wrong address" — a donation, a tip. Anything with a price to verify or a service to deliver needs a live phone emitting a fresh `TapPresent`.

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

## 15.11 Operating modes

A device at a counter is a till all day. A phone on a bar is a tab book all night. These are **stances**, not features: each one fixes the answers to §15.2's two questions — `presenter_role` (who presents) and `amount_authority` (whose number is it) — and an implementation SHOULD let the user set the stance once rather than re-derive it per customer — and SHOULD let the stance own the whole client while set: a till that still carries the wallet's tabs is a till asking its operator to navigate.

The claim that matters most here is negative: **no mode below requires a new wire object.** Each is a discipline over machinery this protocol already has, and an implementation that invents per-mode message types has misread the section.

| Mode | presenter_role | amount_authority | State | Rides on |
|---|---|---|---|---|
| Point of sale | payee | presenter | one sale | card → thread → itemised request → receipt (§16.13) |
| Bar tab | payee | presenter | **a tab per customer, across events** | one thread per tab; settlement is one itemised request |
| Taxi | payee | presenter, **disclosed at start** | one ride | thread; terms message at meter start; itemised request at end |
| Donate | payee | **open** | none | `TapStatic` (§15.9) for the static target; a live screen MAY add a claim-once card (`donate`) |

**Bar tab** is the first mode with state across events, and the state lives on the *payee's* device, not the wire: lines accumulate locally, and the network sees one itemised `PAYMENT_REQUEST` when the tab settles. The thread is the tab's identity — opened by a card claim (or reusing an existing contact, which is what a regular is), and still reachable when the customer has gone home, which is §16.12's whole reason to exist. An implementation SHOULD send each added line to the customer as an ordinary message with the running total — a tab that is silent until close is a bill that arrives as a surprise — and MUST NOT send per-line payment requests, because five requests for one evening is five confirm screens where one was owed.

**Taxi** adds one requirement, and it is about consent rather than formatting: **the rate MUST be disclosed in the thread when the meter starts** — base fare and per-unit price, as a message the rider keeps — and the final bill MUST carry the arithmetic in its line items ("base fare", "23 min × 0.0005 XMR/min"), which §16.13's sum rule then makes checkable rather than assertable. A rate quoted in fiat MUST be converted to piconero *at meter start* and used unchanged at settlement: the figure the rider agreed to is the figure they are billed, and exchange movement during the ride is the driver's exposure, who chose the quoting currency.

**Donate** has two targets, divided by what stands behind the code — and the division is §15.9's own rule, applied rather than repealed.

The **static** target — the sticker, the printed sign, the code on a website — is not a contact card and MUST NOT be built as one: a claim-once card dies at its first scan, and a printed surface has no phone behind it to cut the next. It is §15.9's `TapStatic` stance — reusable, public, receive-only, establishing no relationship — and it inherits §15.9's admitted limits verbatim: the address is reused (every donor is linkable to every other on the public ledger), and a wholly swapped target verifies. An implementation MUST state the reuse cost where the target is created, once and plainly. What it buys is the property print needs: any Monero wallet can pay it, with no DUCAT on the donor's side.

The **live screen** is a phone, which is exactly what §15.9's closing rule asks for — so an implementation MAY additionally offer a **claim-once card with purpose `donate`** (§16.9, field 217), reissued when claimed, precisely as the bar tab and the kiosk cut theirs. This one establishes the relationship *on purpose*: the donation lands in a thread, and this section's own receipt rule applies — the donor is the party who benefits from the record (a donation receipt is tax paperwork in most jurisdictions), the recipient is the only party who can issue it, so on observed settlement of an unprompted payment in a donate-card thread the recipient SHOULD send `RECEIPT` automatically, subject to the same chain-observation discipline as every vendor receipt. No new wire object: the purpose field is presentation-scoping (§16.9), the receipt is §16.13's, and how a client files the payment in its own statement is client-local.

### Tips

A client presenting a received request SHOULD show the requested amount as **fixed**, with an optional **additive tip**, and MUST NOT offer to edit the amount itself. This does not touch §16.13's no-authority rule — declining is still declining, and nothing obliges the payer to answer at all. What it prevents is a payment that answers nothing: an edited amount matches no outstanding request on the payee's side, so it settles no bill and thanks nobody. The honest way to pay a different amount is a different payment.

The total actually sent — bill plus tip — travels in the payer's `PAYMENT_SENT` notice as usual, and the receipt that follows settlement covers the amount that arrived, with the tip as a visible line item so §16.13's sum rule holds. A tip on a receipt is also simply the truth of what was paid.

### A claim answers a card

An issuer may have several cards outstanding at once — a standing profile code, and a till's per-sale handshake. A claim MUST be bound to the card whose inbox it was written into, and a flow waiting for "its" claimant MUST wait on that card specifically, never on *the next contact to appear*. The failure this prevents was observed before it was specified: an implementation that watched for any new contact would have billed a bystander who scanned the profile code mid-sale.

### Withdrawing a bill

Two exits from billed-but-unpaid, and every vendor mode needs both:

- **Cancelled.** A payee MUST be able to withdraw a bill, and MUST say so in the thread when it does: the counterparty's client still holds an actionable request pointing at money nobody is watching for, and a cancellation they never hear about is a payment into the void. There is no wire object for this and none is needed — a request carries no authority (§16.13), so withdrawing one is advisory text, like the request's own claim on attention.
- **Settled outside DUCAT.** Cash across the bar is a settlement, not an exception — the fallback rails existing is half this design. The customer SHOULD still receive a `RECEIPT`, which the wire permits without a transaction: their record ought not depend on which rail the money took.

An implementation using unified settlement tracking MUST treat *abandoning* a billed sale as cancellation, not as a screen change: a settled record left watching is one a later unrelated payment of the same amount will match, and the receipt fires into a dead sale's thread.

### Observed settlement, and the receipt that follows

Vendor modes settle by **watching the chain**, not by believing a notice (§17.5). A payee matching an arriving output to an outstanding request by exact amount:

- SHOULD prefer amounts nominated by the payer's own `PAYMENT_SENT` notices in the request's thread (at or above the billed amount, arriving after the bill), since a tipped payment is larger than the bill and exact matching alone would never find it — the notice *nominates*, the chain *confirms* (§17.5), and a notice with no matching output settles nothing;
- MUST match against its own outstanding requests **oldest-first**, and
- MUST NOT acknowledge the same output for two requests.

The residual is stated rather than waved off: two customers paying *identical* totals in the same window are indistinguishable by amount alone, and an implementation SHOULD surface an unmatched or ambiguous arrival to the operator instead of guessing. (Per-request payment IDs would close this and are deliberately not specified here: integrated addresses are being deprecated ecosystem-wide, and a DUCAT-only convention would break the "any wallet can pay this" property the fallback exists for.)

An implementation MAY additionally scan the **transaction pool** for a payment matching an outstanding bill, and surface the sighting to the operator. A sighting is §17.5's *seen*, never settlement: the receipt MUST NOT be issued and goods MUST NOT be released on sight alone — accepting unconfirmed payment is §8.6's bonded mode, with a bond behind it. An implementation SHOULD stop offering to cancel a bill once its payment is sighted, since money in flight has nowhere else to go.

On observed settlement a vendor-mode implementation SHOULD send `RECEIPT` (§16.13) into the thread automatically, carrying the same line items and the transaction it acknowledges. The reasoning: the party who benefits from the record is the payer, the payee is the only party who *can* issue it, and a receipt that depends on the vendor remembering is a receipt the busy vendor forgets.

## 15.12 Hail — dispatch without a dispatcher

Everything above connects people who **met**: a card crossed a table or a screen. Dispatch needs the one thing §16.12's no-directory stance refuses — a rider and a driver who have never met, converging with nothing in common but *where they are*. A server that knows everyone's location solves this trivially, which is why every ride-hail has one. This section is the same problem solved with nobody knowing anything.

### The stand convention

Veilid computes a record key locally from a schema and an owner public key. Therefore a keypair derived from a **public string** gives everyone who knows the string the same record: the DHT is a map from *names* to *bulletin boards*, and a geohash is a name. So is "the taxi rank at the airport". A named place is a **stand**; the derivation is the convention, pinned exactly:

```
seed        = SHA-256("DUCAT-STAND-v0" ‖ 0x00 ‖ cell)      → ed25519 owner keypair
enc         = SHA-256("DUCAT-STAND-v0-ENC" ‖ 0x00 ‖ cell)  → record encryption key
schema      = DFLT(8)
record key  = get_dht_record_key(schema, owner_public, enc)
```

Both halves are load-bearing. Veilid encrypts every record's values under a key carried in the record-key *handle*, and `create` always draws a random one — so a public board must derive its encryption key from the cell name too, both sides constructing the full key locally, the create-time key simply never used. Values "encrypted" under a public secret are public, which is the point. The version tag in the seed means a future scheme change lands on different records rather than fighting over these. Proven over the live network cold, cross-process: `harness/src/stand.rs`, `research/dispatch/REPORT.md`.

### Geocells: the map is the name space

A geohash interleaves latitude and longitude into a base32 string whose every prefix is a containing cell — a place at a stated coarseness. The cell name `geo:<geohash>` fed to the derivation above makes the DHT a map of the Earth at the coarseness the string chooses: 5 characters is ≈ 4.9 km, 6 is ≈ 1.2 × 0.6 km, and **6 is the cap on any public surface** (§16.17), so "no precise location on a board" is true by construction. Rules with reasons:

- **Truncate, never round.** Rounding can jump a cell boundary, posting a rider to a board nobody near them watches.
- **Encoding is integer arithmetic** over 1e-7-degree coordinates with floor midpoints (the reference and §18.11's second implementation agree on it): two implementations disagreeing on a boundary is two people on the same corner posting to different boards.
- **A driver watches the 3×3 neighbourhood** — their cell and its 8 neighbours. A rider fifty metres over a border is otherwise invisible, and nine boards also spread §15.12's per-replica watch ceiling across nine records.
- **Density picks the precision, by a rule either side can compute alone.** A rider posts at precision 6; when the 6-cell's shard 0 holds no other live notice — a deserted corner — they SHOULD post a second copy of the *same* notice (same card) on the containing 5-cell, where a driver kilometres away is actually looking. A driver watches the 3×3 neighbourhood at **both** precisions. Both sides MUST dedupe by card, and the two copies are safe for the same reason migration's two copies are: one claim-once card, one referee. A claimed or withdrawn notice clears both slots. The asymmetry is deliberate — the busy case pays nothing extra (no second copy, and a driver's extra nine 5-cell boards are one quiet read each), while the sparse case buys a ~5 km radius for one extra post.

### Generations: a griefed board is abandoned, not lost

A stand's write key derives from its public name, so **anyone who can name a board can write any slot on it**. That is stated plainly above and it is the price of having no operator: boards are hostile surfaces, notices are tiny and expiring, and value flees immediately into sealed threads. Wiping somebody's notice is griefing a bulletin board, and the answer is to post again.

That answer depends on being able to post again, and until 0.88 it was not true. A Veilid subkey carries a sequence number; a node accepts an inbound write whenever its sequence is merely **greater** than the one it holds, not exactly one past it; and `ValueSeqNum::next()` fails at `u32::MAX - 1`. So a single write per slot at the maximum leaves every slot on a board unwritable **by anyone, permanently** — and because the record key is a pure function of the name, there is no other board for that cell to move to. 128 writes (16 shards × 8 slots) would end a neighbourhood's boards for good, from anywhere in the world, with no way to repair them short of abandoning every board globally.

So a board name names a **generation**: `<name>@<epoch>`, decimal and unpadded, applied *before* the shard suffix, so a full board name reads `geo:u4pruy@3021-3`. The epoch is `floor(unix_seconds / 604800)` — one week. The spelling is pinned by vector (`stand.epoch`), and a name that already names a generation MUST be refused rather than stamped again: re-stamping computes a board nobody else computes, and moves a poster off the board its own notice is standing on.

Three rules follow:

- **Writers and readers both stamp at the moment of use**, never store a stamped name as a cell. A reader that keeps raw cells and stamps on each sweep follows a rollover with no rollover logic at all.
- **A poster whose board has gone stale re-posts at once** rather than waiting out its ordinary refresh, and MUST NOT keep its tenancy on the old board. Its notice is still there and still unexpired; nobody is reading it.
- **An implementation SHOULD refuse a board name that names no generation** rather than derive from it. The epoch has to be stamped at every site that forms a name, and a site that forgets would read and write a board nobody else computes — a failure that appears only in the field, only against other people, and looks exactly like a quiet network.

**What this does not do is stop an attacker.** Re-poisoning a cell costs the same 128 writes once a week, which is nothing, and a determined adversary keeps a neighbourhood dark for as long as they care to pay. It is not a defence and must not be described as one. What it removes is the *ratchet*: without it, every cell anyone ever poisons stays poisoned, so the reachable surface only ever shrinks and nobody can repair it. With it, damage lasts exactly as long as somebody is maintaining it. A board that can be griefed is the design; a board that can be griefed *once, for ever* was a defect.

Two costs are accepted. Clock skew across a rollover puts two clients on different boards for as long as they disagree — minutes out of a week — and rollover costs a live notice one poll of visibility while its poster notices and re-posts. Deployed boards from before this change are abandoned rather than migrated; at pre-alpha that is a smaller price than the alternative, and it heals every board already poisoned.

### The overflow ladder: capacity that costs what it uses

Eight slots is a neighbourhood, not a stadium letting out. A stand grows by **shards**: shard 0 is the bare stand name, and overflow shards append `-<n>` — decimal, no leading zeros, `MALFORMED` at 16 — before the name enters the derivation above. A shard is just a name; nothing else changes. The format is pinned by vector (`stand.shard`) because both sides construct it independently, and a writer and a reader disagreeing on a spelling are standing at different corners.

Two rules make the cost proportional to demand rather than to the namespace:

- **Writers backfill low.** A notice goes to the lowest-numbered shard with a free slot (a free slot is empty, expired, or undecodable — debris does not hold a place). This keeps the ladder compact, which is what the reader's stopping rule relies on.
- **Readers sweep from shard 0 and stop at the first shard holding nothing live.** A quiet cell costs one read; a busy one costs its actual height; 16 reads is the worst case ever. The race this trades away is honest: a claim can empty a low shard while a notice still stands above it, and that notice waits until the sweep's next pass or its poster re-posts lower — minutes of visibility lag at the edge, in exchange for never paying for capacity nobody is using. A rider's client SHOULD move its notice down when a lower slot frees.

The ladder's height is a live congestion signal — "this cell is four shards deep" — published by nobody, readable by anyone, and it composes with precision: shards absorb bursts inside a cell, finer geohashes spread sustained density across cells. When the ladder tops out, the cell has outgrown itself, and the answer is precision 6 instead of 5, not shard 17.

**Read-before-write is a correctness rule on boards.** A subkey write is silently refused by the network unless the writer's local store knows the slot's current value sequence — so a client MUST read a slot before overwriting a tenant it did not write in the same session (finding a free slot does this naturally), and a client that needs delivery certainty confirms by reading its bytes back, not by trusting the set call's return.

The fare estimate this enables is deliberately ordinary: every taxi on Earth prices as `base + per-distance + per-time`, distance approximated as the great-circle length times a circuity factor (≈ 1.3 — roads detour), time from an average speed. The estimate seeds the rider's `fare_pxmr` **offer**, snapshotted to piconero at post time exactly as §15.11 snapshots a meter rate. There is no surge because there is no one to decree it: its replacement is the negotiation the thread already carries — a counter-quote on a busy night is price discovery, not price setting.

**The board is honest about what it is.** The seed is public, therefore the secret is public, therefore **anyone can write or wipe the board** — a bulletin board in the literal sense, pinned in a public square. What keeps a real one useful keeps this one useful: notices are tiny and short-lived, everything of value moves immediately into a sealed thread, and a wiped board is re-pinned by the next person who needs it. A vandal buys minutes of nuisance in one cell. This is a weaker guarantee than a dispatcher and it is the honest price of not having one. A notice MUST NOT carry precise coordinates, an identity, or anything worth scraping; a client MUST treat everything read off a board as untrusted input.

### The hail

1. **A rider posts a notice** (§16.17): a freshly minted **claim-once card** with purpose `hail`, a coarse area or destination — never a coordinate — and an expiry. Notices land on a subkey chosen at random from the schema's eight; a collision overwrites, which at board scale is weather.
2. **Drivers watch the cells they are actually in.** Veilid's per-replica watch limits (32 public watchers) size the cell: a neighbourhood fits, a city does not.
3. **A driver claims the card.** Claim-once means exactly one driver wins — **the race is settled by the record, not by a matchmaker**. On a board the card's writer secret is public text, so the rule that settles it is the reader's, not the schema's: a claimant refuses a card that already holds a reply, and a rider whose reply subkey shows more than one write adopts nobody and posts a fresh hail (§16.12, *Single use, and why*).
4. **Everything else leaves the board** for the claimed thread, sealed (§16.11): precise pickup, quote, ETA. The quote is the driver's §15.11 taxi terms message — base fare, rate, and optionally a **pickup deposit**, which is not escrow but simply a first payment (§16.13 machinery unchanged); default zero.
5. **The ride ends as a §15.11 taxi ride**: one itemised bill whose arithmetic is checkable, notice-nominated tip, receipt on observed settlement. Dispatch changes how the parties *found* each other and nothing about how money moves.

### The offer, the accept, and the retract

A claim used to *be* the deal, which put the commitment at the wrong moment: the driver had committed before stating a price, and the rider had committed before hearing one. Three message kinds (§16.13's registry: `RETRACT` 5, `RIDE_OFFER` 6, `RIDE_ACCEPT` 7) move the commitment to where the parties actually decide, and all three are ordinary sealed messages — chained, sequenced, advisory, no new delivery machinery:

1. **The claim opens a channel and owes nothing.** Claiming the card is *applying*: it hands both sides a sealed thread and the driver's identity (car and plate ride the `CONTACT_ACCEPT`, §16.9). The DHT still referees the race — one claimant, everyone else never existed.
2. **The driver's first word is a `RIDE_OFFER`**: the fare in piconero (MUST — an offer without a fare offers nothing), optionally `eta_secs` (213) — how far away they are, a courtesy figure bounded at a day, meaningful on no other kind. The fare may match the notice's `fare_pxmr` or counter it; the counter-quote is §15.12's price discovery doing its job.
3. **The rider answers with `RIDE_ACCEPT` or `RETRACT`.** An accept MUST name the offer (`re_seq`) and MUST echo its fare — "accepted" bound to a number both parties said, so two offers in one thread can never leave the price ambiguous. A reader MUST verify the echo against the referenced offer. Only now is there a deal — and it is a *promise*, exactly as strong as flagging a cab (§15.12's honesty about unbonded hails is unchanged).
4. **A `RETRACT` withdraws the message it names.** With `re_own`, the sender withdraws their *own* earlier message — a driver pulling their offer, a vendor cancelling a bill (§16.13: the request's button on the other phone goes dead instead of paying into a sale nobody is watching). Without it, the sender declines the counterparty's. It carries no amount and no bill: it withdraws, it does not transact. Advisory like everything here — no money moves or un-moves.

A declined or retracted application leaves the rider free to repost (fresh card — the old one is spent, which is what claim-once means), and leaves the driver exactly one message poorer. Money never moves in this ceremony: the ride still ends as §15.11's taxi bill through §16.13's machinery, and the accept's fare is the number that bill has to answer to.

### Live position after the accept — the map that lives after the match

§5.2.3 promised the ladder's last rung — *"during service: live position, over E2EE"* — and refused everything before it. This is that rung's mechanics (built in 0.89; `core::position` and kind 11), and the gate is the ceremony above: **a client MUST NOT send a position reference before a `RIDE_ACCEPT` exists in the thread, and MUST ignore one that arrives earlier.** Watching your driver approach is safe *because* both parties have chosen each other and are about to be physically co-present anyway (§2.3); the same stream before the accept is a stranger-tracking primitive, which is the thing §5.2.3 exists to refuse.

**Consent is per ride, per direction, off by default.** Each side decides independently — a rider may share while the driver does not, and either alone is useful. The ask happens at the accept, when it means something, and MUST NOT be a standing profile setting: a toggle that silently shares on every future ride converts one moment's consent into a policy nobody remembers choosing. (§5.2.3's *"optional visibility is not optional"* argument does not bite here — a pairwise stream creates no market-wide pressure the way public opt-in position does — but a *standing* toggle would quietly manufacture the same always-on exposure one ride at a time.)

**The stream is a record, not messages.** A thread is a ring of a few dozen slots that exists so words can stop being readable (§16.12); telemetry at a five-second cadence would evict the conversation it is supposed to accompany, and a chat history that doubles as a movement log is §5.2.3's surveillance database rebuilt inside the E2EE. So position rides §16.15's pattern instead: a fresh record created for the ride, its reference sealed into the thread once, and every update **overwriting the same subkey** — the stream has a *now* and no past by construction.

- **`POSITION_REF` (kind 11)** is an ordinary sealed message carrying the record key (field 218) and a fresh 32-byte stream key (field 219). Both fields travel together or not at all (§16.15's rule, same reason). The thread's own encryption protects the reference, so the record on the network is noise to anyone who was not a party.
- **Each update** is sealed with XChaCha20-Poly1305 under the stream key, fresh random nonce, the record key as AAD (a value lifted from one record cannot authenticate in another). The plaintext is a monotonic counter, position as §15.12's 1e-7-degree integers, an optional heading, and the capture time — padded to one fixed size, so every update is the same length and the ciphertext sequence carries nothing but its own cadence.
- **A receiver** MUST drop a counter *lower* than the highest it has accepted (an old update replayed inside the ride). An **equal** counter is not a replay and MUST NOT be treated as an absent position: the record holds one value between writes, so any reader whose polls outpace the sender's cadence reads the same frame more than once, and a sender whose phone cannot get a fix leaves it standing. That frame carries its own capture time, which is what ages — reading "same counter" as "no position" reports a working stream as a dead one, which is the same lie as a guessed dot told the other way round. A receiver MUST render staleness as staleness — *"last seen 40 s ago"*, never a guessed position — and MUST treat the whole stream as the claim it is: spoofable input, drawn like the plate, checked against the window.
- **Cadence SHOULD be fixed** while sharing (three to five seconds). A constant heartbeat leaks liveness and nothing else; an adaptive one turns the update pattern itself into a channel.

**Bounded to the ride, and the bound is enforced twice.** Sharing MUST stop at the receipt (§16.13's observed settlement), at a `RETRACT` naming the reference (`re_own` — the sender withdrawing its own stream, §16.13's existing verb doing one more job), or at the notice's own expiry, whichever is first. The record's TTL is ride-scale, so the transport forgets even if a client does not. At stop: the sender SHOULD overwrite the subkey empty and MUST delete local record state (§18.7 stewardship — same rule as a spent hail); the receiver MUST discard the reference — and an **empty** slot is how it learns to, once it has read at least one frame from that record, since the reference is handed over before the first update exists and an empty read beforehand means only "not started". A receiver that never lets go keeps ageing one last position for the rest of the ride against a sender who has explicitly stopped. It **MUST NOT retain the counterparty's track beyond the ride** — the map shows where they are, not where they have been, and a client that archives a peer's movements has rebuilt pairwise what §5.2.3 refused publicly. A new ride mints a new record and a new key; reuse would make the record key a long-lived identifier linking rides.

**What the network sees, stated honestly.** A replica holding the record sees an encrypted value rewritten on a fixed cadence: liveness and cadence leak, content and parties do not (the keypair is random, the reference sealed). Who *fetches* the record is Veilid's anonymity story (§2.4), not this section's. And position remains display-only input everywhere: it MAY prompt, it MUST NOT transact — which is the next rule, already load-bearing.

### The bonded hail — escrow at the accept

An unbonded hail is a mutual promise, and §8.2's escrow is how the promise grows teeth without growing an operator: **at the accept, the fare goes into a 2-of-3 the parties build on the spot.** The rider is the funder, the driver the payee, and the third key belongs to an arbiter both sides already hold as a contact — chosen by the rider until markets carry arbiter descriptors (§10). No arbiter configured, no bond: the hail stays the promise it always was, stated rather than implied.

The mechanics reuse everything already on the table:

1. **The accept starts the ceremony.** Alongside `RIDE_ACCEPT`, the rider opens a §17.9 DKG with driver and arbiter. The ceremony's round-0 frame names its kind (`ride`), its funder, and its fare — the escrow describes itself, so the driver checks the ceremony's number against the number the accept echoed, not against a separate message. The build is sealed messages over existing threads; everyone must be mutual contacts, which the claim (driver) and the configuration (arbiter) already guarantee.
2. **The rider funds the address every party derived.** An ordinary wallet send to an address that needs two of three keys to leave. The escrow is fresh per ride; nothing links it to any other.
3. **"Fare secured" is a scan, not a claim.** Each side verifies the funding by scanning the escrow with the view key derived from the group key (§17.5: a payment is verified by finding the output, never by believing a note from the party who benefits). The scan starts at the build height — an escrow minted minutes ago needs minutes of chain.
4. **The driver completes; the rider releases.** The payee proposes the FROST release to their own wallet; the funder's co-signature is a **screen, never an automatic signature** — §15.5's confirm rule surviving into escrow. A ride ceremony's release proposal parks until the human says yes; only a plain deposit-return (§17.9's 2-of-2 bond) may auto-co-sign. The missing co-signer consent view (§17.9) does not bite here: the sweep can only pay where the proposer says, and the proposer is the party the fare was always for — a redirected destination changes who holds the driver's key, not who earned the fare.
5. **The arbiter holds a share and silence.** On the happy path it participates in the build and is never contacted again — it cannot move money alone, never learns the fare was funded, and never learns the ride ended. On a dispute, either principal shows it the thread and it co-signs with whoever is right (§9.3); the ruling is a co-signature, which is why it cannot take the money for itself.

**The ladder: nobody depends on an arbiter existing.** With no arbiter shared, the same entry point builds a **2-of-2 on mutual stakes**: each side stakes a percentage of the price, and the release is a **split** — one co-signed transaction returning each stake and paying the price to the provider. The game theory replaces the judge: the price is already beyond the payer's unilateral reach, so releasing costs nothing and returns their stake — sulking is strictly worse; the provider extorting burns the price they earned.

**The numbers, and where they come from.** A client SHOULD suggest **10%** each side for a ride, **20%** for a stay, **30%** for a vehicle — the more an asset can be damaged beyond its rental price, the more each side puts down. These are suggestions a client MAY let either party change, not protocol constants; what the protocol carries is whatever the ceremony frame names. Two bounds are worth honouring: a stake below roughly twice the release fee SHOULD be zero rather than decoration, because it deters nothing and costs more to hand back than it is worth; and no stake SHOULD exceed half the price, beyond which a deal stops being takeable. The closest working precedent is Bisq — 2-of-2 with no custodian, deposits from both sides, 15% minimum and 50% cap, chosen expressly so cooperation is likely *without* a reputation system, which is a privacy cost DUCAT also declines to pay. The theory (Asgaonkar & Krishnamachari, ICBC 2019) proves a dual-deposit escrow is cheat-proof at equilibrium with deposits merely positive and safety rising with size, but derives no optimum; the ceiling therefore comes from practice, where large deposits are known to price people out.

What that means at prices people recognise, at a ride's 10% and a rough
$250/XMR:

| Fare | Each side stakes | Payer locks | Provider locks |
|---|---|---|---|
| 0.0005 XMR (~$0.12) | nothing — below the floor | 0.0005 | 0 |
| 0.004 XMR (~$1) | 0.0004 (the floor) | 0.0044 | 0.0004 |
| 0.04 XMR (~$10) | 0.004 (~$1) | 0.044 | 0.004 |
| 0.4 XMR (~$100) | 0.04 (~$10) | 0.44 | 0.04 |

The floor is visible at the small end and that is deliberate: a stake worth
less than the fee to hand it back deters nothing, so the smallest deals
carry none and say so, rather than holding a fee-sized token that looks
like protection.

**The exposed side funds second.** Whoever pays into the escrow first stands alone until the other follows, and the two sides are not equally exposed: the payer is carrying the price *and* a stake, the provider only a stake. So when a provider stake is asked for, a client SHOULD hold the payer's funding until the provider's stake is on chain, and show them why it is waiting. This is also how booking anything works — the other side confirms, then you pay — and it retires the sequencing caveat the reservation shape was published with: a provider who never stakes has simply declined, and nobody's money is sitting in a shared address over it. Where no provider stake is asked for (a ride whose fare is below the floor, or a provider who has nothing to put up), the payer funds first and the exposure is stated rather than hidden.

**Symmetry is a default, not a requirement, and the reason matters.** An earlier draft had the driver stake *nothing* on the argument that their skin is the fare at risk and that a driver-side deposit gatekeeps exactly the people who start with nothing. That argument still holds and is why zero remains a legitimate setting: a client MUST be able to build this ceremony with a provider stake of zero, and the small-fare floor above means the smallest rides carry no stake at all. What changed is the default, for two reasons. It is explainable in one sentence — *you both put up a stake, and finishing gives it back* — and a model a user can hold in their head is itself a security property. And it generalises: the same sentence covers a room and a car, where a provider with nothing at risk is exactly the party a guest cannot safely trust. What 2-of-2 gives up is named plainly: a lost phone with no backup strands the money (§4.3.3's multisig shares in the backup are the recovery), a genuine dispute has no partial ruling — only the split both agree to, or the burn — and at reservation-scale sums that trade gets worse, which is why an arbiter is preferred the moment one exists.

The split release is one primitive serving every rung: fixed slices to named addresses, the residual (minus the true network fee) to the payee. The margin's return, a two-deposit reservation coming apart cleanly, a negotiated 80/20 settlement, an arbiter's partial ruling — all the same transaction shape, differing only in the list.

**Settlement is proposals until a signature.** Either principal may propose a split — one number, what the funder gets back — and a proposal is a fresh FROST round 0 that supersedes whatever stood before it, including the proposer's own. The counterparty's screen states the claimed split and offers exactly two moves: sign it (the negotiation ends, the money moves) or counter with a different number (the roles swap). A rider proposing routes the payee's slice only to the address the driver published in the handshake — a proposer can never aim the other side's money somewhere the other side did not name. The burn is not a third move; it is what remains if nobody ever signs, and its whole job is to make one of the two moves happen.

**And when the counterparty is gone, the same proposal goes to the arbiter.** No new machinery: the stranded principal sends the identical round-0 to the third key instead, the arbiter's screen (or console — an arbiter is a machine that stays on and a human who judges) states the claimed split, and the arbiter's co-signature *is* the ruling (§9.3) — declining is simply never signing, and §9.3.4's clock still forces an expired dispute to end in a real ruling rather than silence. The destinations do not change with the audience: an asking rider still cannot route the driver's slice anywhere the driver did not publish, so a captured or careless arbiter can at worst pick a split between the named parties — never a beneficiary.

The honest costs, named: the arbiter must be online at the build (§17.8's governance problem wearing work clothes); the release spend waits out Monero's ten-block output maturity — ~20 minutes at target pace, and a client MUST surface "the fare needs N more confirmations" rather than relay the daemon's unexplained refusal, because on a slow chain this is the common case, not the corner; and walking away means abandoning staked money rather than shrugging, in both directions — which is exactly what the bond buys.

**The same shape serves any reservation with a deposit** — a night's lodging, a rented car — and the reservation is where the escrow's whole arithmetic moves into the frame. The guest initiates with three numbers: the rent, their own deposit, and the deposit they ask of the host; the ceremony's round-0 names all three, so the host's phone states exactly what accepting costs before anything exists but keys. **The host's acceptance is funding their deposit** — not a signature, not a message kind: until money moves nothing is at risk, so consent lives where it always does in this protocol, at the moment money moves, and a host who never funds has simply declined. "Secured" means the escrow holds rent plus *both* deposits, verified by each side's own scan. The default checkout release is one split: the guest's deposit home to the frame's refund address, rent and the host's deposit to the address the host published — so a host who under-funded their deposit shorted only themselves. Everything after — settlement by proposals, counters, the arbiter's ruling — is the ride's machinery verbatim; nothing in the escrow knows what the middle was.

One honest sequencing cost, stated: whoever funds first is exposed until the other side funds — a guest whose host never accepts, or a host whose guest never pays, holds money in an escrow the other must co-sign out. With an arbiter (2-of-3) the stranded party asks for their refund and the arbiter rules; in a 2-of-2 the stake sits until the counterparty signs or forever. At reservation sums the arbiter is therefore strongly preferred, and a client SHOULD say so before the first coin moves.

### Position may trigger the request. It MUST NOT trigger the payment.

Arriving at the destination MAY prompt the driver's client to send the bill — the Uber-smooth "it just asks at the end" is the request firing on arrival. But coordinates are spoofable input, and a payment triggered by them is §15.5's confirm screen deleted and §16.13's no-authority rule inverted. Money moves through a human confirmation, every time, including this one. The rider taps pay from the back seat; that tap is the §15 moment, both parties present.

### What this does not solve, named

No protocol makes a car arrive. The platforms solve no-shows with reputation and a card on file — trust plus recourse, not cryptography — and an **unbonded hail is a mutual promise, like flagging a cab**: stated in the UI, not hidden. The deposit shifts the deadhead risk; receipts and the thread are the reputation substrate a regular relationship accretes; and the durable answer for strangers is a **driver bond** posted through Part IV's escrow machinery, which is future work this section deliberately does not specify. What this section refuses: any design where dispute resolution reintroduces an operator.

**Driver identity is scoped to the claim.** The car fields (§16.9, 210–212) SHOULD travel only in the `CONTACT_ACCEPT` written when claiming a hail — the one moment a rider needs to find a stranger's vehicle. A plate is a real-world identifier; publishing it on every card handed across a bar would spend deanonymization for nothing. The `purpose` field (§16.9, 217) is how the claimant knows which moment it is in: a hail card says `hail`, and the reply scopes itself accordingly — the same mechanism that keeps email, phone and signal off every transactional handshake in both directions.

**Route safety inherited (O17):** the hail imports no route blobs from strangers — the board is read-only DHT, the claim is a card, and the thread is §16.12's mailbox model throughout, which is precisely the containment O17 prescribes while Veilid #395 stands.

**Stewardship (§18.7):** a hail's records are the shortest-lived in the protocol. A rider's client SHOULD overwrite its notice's subkey with an empty value once claimed or expired, and MUST delete local record state when the hail is spent; a board is never rewritten merely to keep it warm.

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

## 16.9 Contact cards — identity without a transaction

§16.3 exchanges identity **after** a receipt, bound by `H(RECEIPT) ‖ session_pk`. That binding is what makes the coda strong: it proves *the persistent identity I am handing you is the same entity you just transacted with*.

A card handed over at a table, or sent through Signal, has no receipt to bind to. It therefore proves strictly less, and the honest thing is to say exactly how much less:

- **It proves key possession.** Whoever produced the card holds the persona key.
- **It proves nothing about who handed it to you.** A card that arrived in a chat app was authenticated by *that app*, not by DUCAT. Received over NFC, the authentication is that a person was standing in front of you.

This is §15.9's lesson for the third time: a signature proves who owns a key, never that the artifact carrying it is the one its author put there. What differs from a static tag is that the carrying channel is usually a person you already trust, which is why the pattern is worth having at all — and why the spec must not overstate it.

**Contact-first is a legitimate ordering, not a weakening of §16.3.** Paying a stall and paying a friend are different relationships. §16.3's anonymity-first ordering remains mandatory for the transactional flows, where the counterparty is a stranger and the deal must close without either side learning who the other is. A card is for the case where you already know.

```
ContactCard  { persona, inbox_key, writer_public, display_name?, expiry }
      │       + writer_secret alongside, in the URI but never in the object
      │       carried by NFC, QR, or a `ducat:` URI (§18.7)
      ▼
inbox  SMPL(1, [writer])          subkey 0  issuer's ContactDetails
      │                           subkey 1  claimant's ContactDetails
      ▼
both now hold the other's outbox key and prekeys (§16.12)
```

**The writer secret is the capability.** Whoever holds it can write the reply subkey and nobody else can, and **Veilid enforces that** rather than this document defining a check an implementation could get wrong. It travels beside the card in the URI and never inside the signed object, so a card that appears in a log or a screenshot of the object alone cannot be answered.

### Single use, and why

An invitation is claimable **once** and MUST carry an expiry.

A card that could be claimed repeatedly is a standing offer to everyone who ever saw the message it arrived in: a screenshot in a group chat, a forwarded DM, someone else's phone backup. Claim-once means the issuer learns it was used and by whom, and a second attempt is refused rather than silently succeeding.

The secret lives in the card; the issuer stores only `claim_commit = H_chain(secret)`. A stolen list of issued invitations is therefore not a set of usable claims.

**Single use is not enforced by the shape of the record.** Earlier drafts of this section said it was — "the inbox has exactly one reply subkey, so a second claimant has nowhere to write" — and that argument is false. `SMPL(1, [writer])` bounds how many *subkeys* a member may write, not how many *times*; a subkey is a mutable slot, and a write that carries a newer sequence replaces what is there. So a second claimant has exactly the same place to write as the first, and wins by writing later. Where the card travels privately — a tap, a QR held up between two people — the capability travels with it and this costs nothing. On a **public surface it is the whole game**: a hail or a listing publishes the card URI, writer secret included, so every reader of the board holds the capability to overwrite whoever answered and be adopted as the counterparty, payment address and all.

What makes single use real is a rule at the **reader**, and the sequence is what it reads:

- A claimant MUST read the reply subkey before writing it and MUST refuse a card that already holds a reply. This is what makes one write the only honest history the slot has.
- An issuer MUST treat a reply subkey whose sequence shows **more than one write** as contested, MUST NOT adopt any claimant from it, and SHOULD discard the card unclaimed. It cannot do better than discard: nothing in the record says which writer was the person the card was handed to.
- An issuer that has read a reply SHOULD delete the record, which turns a stale card into a clean failure instead of a silent one.

A contested card costs its issuer a fresh card and nothing else. An attacker who can reach the board could always have claimed first; being able to *overwrite* is the part that had to go.

Two refusals remain normative:

| Condition | Reject | Why |
|---|---|---|
| `now > expiry` | `EXPIRED` | A card that never expires is a credential its issuer has forgotten they published. |
| Reply subkey already written | `REPLAY` | The screenshot case. Checked by reading subkey 1 before accepting, not by trusting a local flag. |

A **public, reusable** artifact is a different object and already exists: `TapStatic` (§15.9), which receives money and establishes no relationship. A donation QR on a website is that, not this — and it inherits §15.9's stated limit, that a swapped tag verifies perfectly.

### Carrying it

A signed card is **402 characters** as a `ducat:` URI. The record-based form (§16.12) replaced a 1070-byte route blob with a ~100-character record key, taking the URI from 1710 characters to 402 — **under a quarter**, and no longer dominated by a field that could grow.

| Channel | Verdict |
|---|---|
| NFC | Comfortable, and now trivially so. |
| QR | About a **version-13** symbol at level L, down from version 31. Scans from a phone screen at a glance and survives print at ordinary sizes, which the previous form did not. |
| `ducat:` URI (§18.7) through a messaging app | Short enough to paste without wrapping mangling it — a practical property the long form did not have. |

Nothing in the card grows with use now: a record key is fixed-width, so the size is stable rather than merely acceptable today.

### A card outlives its route, and that is a problem

A private route does **not** survive the issuing node restarting. Observed on a device: the app crashed, and every card it had handed out became unclaimable — the signature still verified, the expiry was still hours away, and the rendezvous pointed nowhere. The claimant sees a timeout, which says nothing about why.

So the 24-hour expiry in §16.9 is an **upper** bound on usefulness, not an estimate of it. In practice a card is good until the issuing app next restarts, which on a phone is a matter of hours at best and is not under the issuer's control.

Two consequences, neither yet addressed:

- **A claim failure cannot currently distinguish** "they are offline", "the route died", and "the card was already used". Only the last is a refusal the issuer sent; the other two are silence, and silence is what a timeout looks like too.
- **Re-publishing a route** for an outstanding card needs somewhere stable to publish it — §16.4's rendezvous DHT record rather than an inline blob. That is the fix, and it is the difference between a card being an address and a card being a snapshot of one.

Until then an implementation SHOULD issue cards with short expiries and expect to reissue, rather than presenting a card as something that keeps working for a day.

A **donation QR on a website is not this object**. That is `TapStatic` (§15.9): reusable, public, no relationship established, and carrying §15.9's admitted limit that a swapped tag verifies perfectly.

### Display names are not names

`display_name` is self-asserted and worth exactly what the channel that carried it is worth. The name a user actually sees MUST be the petname they assigned locally (§7.5). An unassigned contact MAY show the asserted name, and MUST show it as unverified.

Bounded at 32 characters, counted in **characters, not bytes** — a byte bound silently shortens every script that needs more than one byte per character, so 32 Japanese characters would be refused while 32 Latin ones passed.

---

### A profile, and why it is not in the card

A contact list of hex strings is a contact list nobody uses. `CONTACT_ACCEPT` therefore carries an optional profile: a small picture, an email, a phone number, a Signal username, and pronouns.

**None of it is in the card.** This is the design, not an implementation choice. A card is a QR code held up across a counter and scanned by a phone camera; a picture in it makes it unscannable, and every field added shrinks the distance at which it works. The card's whole job is to get two people **connected** — after that they have a record and a log, and everything else arrives over those. It is also why a profile can change afterwards without reissuing anything anyone is holding.

**Every field is validated on the wire, not at the screen.** These render as identity on a stranger's device, and a field nobody validates says whatever the sender wants:

| Field | Rule | Why that rule |
|---|---|---|
| avatar | bounded, and the format checked by magic number | Attacker-supplied bytes handed to an image decoder. A decoder should never have to guess what it was given, and an avatar is a thumbnail rather than a file transfer — it also has to fit in the record beside the keys. |
| email | a strict shape, stricter than RFC 5322 | That grammar admits quoted strings, comments and control characters no client should be drawing as somebody's identity. |
| phone | digits only, country code included | One number has a dozen spellings. Accepting all of them means two clients render it two ways and neither matches when someone searches. |
| signal | `name.digits` | Signal's own shape. A username that cannot exist points at nobody. |
| pronouns | a closed set | Drawn beside a name on a stranger's screen; free text there is a place to put a message. |

The closed set is `she/her`, `she/they`, `he/him`, `he/they`, `they/them`, `any`. **The cost of closing it is real and is not hidden:** it cannot express every pronoun anyone uses, and a person whose pronouns are not on the list has only `any` or absence. Absence is not a failure state — a client MUST render a person with no pronouns set exactly as it renders anyone else, and MUST NOT substitute a guess.

Everything here is a **claim**, and a client MUST present it as one. DUCAT binds a persona to a key and binds that key to nothing in the world; an email shown beside a persona is what that persona said, which is worth having and is not identity.

**The handshake says what it is for, and the profile is scoped to it.** `CONTACT_ACCEPT` carries an optional `purpose` (field 217, text, ≤ 16 chars): `profile` for a standing contact code, `sale`, `hail`, `intro`, … for a transaction. The issuer stamps it; the claimant reads it and scopes its own reply to match. The rule extends §15.12's plate discipline to the whole record: **email, phone and signal are reach-me identifiers — ways to locate a person off DUCAT, exactly the plate's class — and they SHOULD travel only on a `profile` handshake**, never on a till's, a tab's or a hail's. A bar tab does not need the till owner's Signal handle, and the till does not need the customer's, yet before this rule both crossed on every sale, in both directions. The car fields keep their own gate (a driving claim, §15.12); name, avatar and pronouns ride wherever the person's share switch allows — recognising who is at the counter locates nobody off the app; and the payout address keeps its separate opt-in (field 182). An absent `purpose` — an older peer, or a card that did not say — MUST be read as *not* a contact exchange: the private default, nothing optional beyond a name. The field is presentation-scoping only: it carries no authority, changes no protocol behaviour, and a claimant that ignores it merely overshares its own data, never the issuer's.

All of it travels in the encrypted backup (§4.3). A persona restored with the right money and no face is not the same person to anyone who knew them, and nothing else in a wallet would report that it had been lost.

## 16.10 Messages

A persistent contact carries `chat/1` (§16.5). One message, and what it must satisfy:

```
MESSAGE { seq, prev, body, timestamp }
```

**Per-sender sequences, not a shared one.** Each participant numbers their own messages. A shared counter needs agreement, agreement needs a round trip, and an offline sender does not have one.

**`prev` chains to the sender's previous message**, or 32 zero bytes for the first. This makes a *dropped* message detectable rather than merely absent — the case worth catching is a message removed and replaced, where the sequence number still fits and only the link disagrees.

**A gap is refused, not stored around.** A thread that silently skips a message displays a conversation that did not happen, and the reader cannot tell. `STATE_VIOLATION` on a sequence gap or a replayed sequence; `COMMIT_MISMATCH` on a link that does not follow.

**Bodies are bounded at 2000 characters.** Larger than a memo because this is prose rather than a label, and still bounded: an unbounded field on a channel that persists is a file transfer nobody designed, with the storage and retention consequences of one.

### One meaning, one encoding

A present-but-empty text field is `MALFORMED` in every object that has one — memo, display name, message body. Omitting the key is already how you say nothing, and §18.1 admits one encoding per meaning. Without this rule `H(FullOffer)` would depend on whether a client wrote `""` or nothing into a field the user left blank, and two implementations would hash the same user intent differently.

This was a real defect: §7.5's memo shipped at 0.61 accepting `Some("")`, with a test asserting the wart was a feature.

### What this does not solve

- **Forward secrecy** is specified in §16.11, which is where the encryption lives. §16.10 covers ordering and framing only.
- **Retention.** §7.4's dossier argument applies with more force to conversation content than to transcripts. A message log is the most sensitive thing the app will hold, and §2.2's endpoint-compromise scope now includes it.
- **Groups.** 1:1 only. Splitting a bill does not need a group primitive — §15.2's `amount_authority: open` already covers it, with each person tapping the presenter separately, which is how a table splits a bill anyway.
- **Store-and-forward delivery.** Both participants being online is assumed here. Deferred delivery is §8.4's research track and shares its unsolved parts.

---


## 16.11 Message encryption, and what "forward secrecy" cost

### HPKE alone was the wrong half of the property

The obvious move was HPKE, because that is what VeilidChat migrated to. HPKE **base mode** encrypts with an ephemeral *sender* key against a **static receiver** key. That is sender-side forward secrecy: it protects against compromise of the sender, and not at all against compromise of the receiver. Seize the receiver's phone, recover one long-term X25519 key, and every message ever sent to them decrypts.

For a protocol whose stated threat model is §2.2's endpoint compromise, that is the wrong half. **Adopting HPKE and stopping there would have let this document claim forward secrecy while providing the one direction that matters least.**

### Rotating receiver keys

Forward secrecy requires the *receiver's* key to be gone. So the receiver publishes short-lived keys to its rendezvous (§16.4) and deletes each after use:

```
PREKEY_BUNDLE { signed_prekey, one_time[ (id, key) … ], expiry }   signed by the persona
        │
        ▼
SEALED_MESSAGE { prekey_id, enc, ciphertext }
```

- **One-time prekeys** — used once, withdrawn from the bundle on successful decryption, the secret deleted after a bounded grace window (the table below says why not immediately). After that the ciphertext is undecryptable **by anyone, including its recipient**. This is the property.
- **A signed prekey** — the fallback when the one-time supply runs out, rotated on a schedule. Messages sealed to it are forward-secret only from the next rotation.

This is X3DH's structure, named rather than reinvented; the exhaustion fallback is Signal's, and so is its known weakness. A sender MUST prefer a one-time key, and an implementation MUST be able to tell the two cases apart — falling back is a real weakening and silently treating both as success hides it.

Four rules, each carrying a failure it prevents:

| Rule | What it stops |
|---|---|
| A key is consumed **only on successful decryption** | Otherwise anyone who can reach the rendezvous exhausts a recipient's one-time keys with garbage, forcing them onto the weaker fallback — a denial of service that lands the victim in exactly the state an attacker wants. |
| Duplicate prekey ids are `MALFORMED` | "Delete after use" becomes ambiguous, and that deletion is the only thing the property rests on. |
| Id `0` is reserved for the signed prekey | A one-time key claiming it would be treated as the non-consumed fallback. |
| Ciphertexts are bounded before any key is consulted | A peer must not be able to make a recipient allocate arbitrarily to reach a decryption failure. |
| A publisher MUST be able to **replenish**, and readers MUST take the refresh | The handshake inbox is a one-time artifact and may be deleted, so a supply exhausted after it was read has nowhere to be topped up from — the pair stays on the signed prekey permanently, forward secrecy quietly gone with no path back. §16.12's log head carries the publisher's current bundle for this reason: it is read on every poll, so a refresh costs no extra round trip. Replenish on a **threshold rather than at zero**, since reaching zero means the next sender is already on the fallback. |
| A sender MUST **withdraw a used key from its cached copy** of the recipient's bundle | `select` is not stateful and must not be — it is a pure read of a published list. A sender that never prunes therefore seals every message to the same key: the first is accepted, the receiver burns it, and every message after that returns an unknown prekey. It presents as the recipient breaking after exactly one message. Observed between two devices, and it is the row below on the other side of the wire — fixing one does not fix the other. |
| Consuming a key MUST also **withdraw it from the published bundle** | Deleting the secret alone leaves the bundle advertising a key that can no longer decrypt anything. Senders take the first one-time entry, so the first key ever consumed is offered forever, and every later message is refused — identically after a re-fetch, since the stale bundle is what gets re-served. Observed between two real devices: the first message worked and every one after it failed. Half a deletion fails closed on everything that follows, which is worse than no deletion at all. |
| A one-time id is offered to **at most one counterparty** | Bundles ride per-thread log heads (§16.12), and nothing else makes them disjoint: one global bundle published to every head means two counterparties holding the same cached copy seal to the same key — the first message in consumes it and the second arrives permanently unreadable, through no fault of either sender. The failure needs two chatty contacts and a shared cached bundle, so a one-contact field test never sees it. Partition the offering, not the secrets: ids stay globally unique on the device, and each thread's head advertises its own disjoint batch. |
| The secret itself is deleted after a **bounded grace window**, not at once | The bundle travels in the log head, an eventually-consistent DHT record: a sender's fetch was observed trailing the receiver's republish, so a sender can seal to a key burned seconds earlier through no fault of its own. Immediate deletion converts that propagation race into a message unreadable by anyone, ever. Withdrawal from the bundle stays immediate; the secret waits out a grace window (RECOMMENDED: 30 minutes) and MUST then be deleted — within the window, the forward-secrecy delete has not yet landed for those messages, which is stated rather than hidden. |
| A message that cannot be opened MUST NOT block the log | Grace expired, or the key never existed: the ciphertext will never open, for anyone. A reader honouring the chain rule literally waits forever, and one dead ciphertext freezes every message after it — observed in the field as a live thread silent for forty minutes behind one lost prekey. The reader records the loss in place, exactly as §16.10 requires for a ring that lapped it, and the chain restarts at the next message, `prev` unverifiable across the gap and said so. |

### Suite, and why this one

RFC 9180 base mode, **DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20Poly1305** — the RFC's A.2 configuration. Chosen partly because it matches suite 1's curve, and partly because A.2 has **published test vectors**.

That matters more here than anywhere else in the document. Both existing conformance efforts — the vector set and the second implementation (§18.11) — share an author with the reference, which is precisely why **O21 stays open**. RFC 9180's vectors do not. `core/tests/hpke.rs` reproduces the RFC's encapsulated key and ciphertext byte for byte, making this **the first externally-validated component in the project**.

The `info` parameter carries §18.3's domain separation (`"DUCAT-v1" ‖ 0x00 ‖ "MESSAGE" ‖ 0x00 ‖ suite`), so a ciphertext sealed for one purpose cannot open under another even with identical keys. `aad` binds a ciphertext to its conversation.

### What this still does not give

**No post-compromise security.** There is no ratchet. An attacker holding current prekey state reads everything sealed to those keys until they rotate; recovery is not automatic the way a Double Ratchet makes it. A ratchet is deliberately not attempted here because it changes the ordering model — §16.10's per-sender sequences would have to become ratchet state — and doing it badly is worse than not doing it. It is the next thing to do, not a thing that has been done.

**Metadata is unprotected by this section.** Who is talking to whom, and when, is a routing property that §16.6's privacy accounting covers, not an encryption one.

---

## 16.12 Delivery — why messages live in records, not in calls

The first messaging build used Veilid's `app_call` against a live private route. It worked, and every operational failure it produced came from the same mistake: **`app_call` is a remote procedure call and it was being used as a mailbox.**

A private route does not survive the issuing node restarting. So a contact card went stale the moment the app was killed, both parties had to be present in the same instant for anything to move, and a delivery failure could not be told apart from the recipient simply being away. Section §16.9 records the measurements; this section records the conclusion.

**VeilidChat does not use `app_call` at all.** Reading their source settles it: their update handler logs and discards `VeilidAppCall`, and nothing in the application allocates, imports, or exchanges a private route. Every message travels through **DHT records**. That is not an implementation detail they happened to choose — it is the thing that makes their contacts survive a restart and their delivery survive an absence.

### The model

```
CONTACT INBOX     SMPL(1, [writer])      created by the card issuer
   subkey 0   →   issuer's details      (owner writes)
   subkey 1   →   claimant's reply      (card holder writes)

OUTBOX            DFLT(n)                one per contact, per direction
   subkey 0   →   head: next sequence
   subkey 1..  →  sealed messages (§16.11), as a ring
```

A card therefore carries a **record key**, not a route blob. A record key is permanent; a route is a snapshot of a process that no longer exists.

Delivery becomes: the sender appends to **their own** record whenever they are online, and the reader collects whenever *they* are. Neither has to be present for the other.

### Why this matters beyond chat

**A payment request that waits.** §15's tap assumes both parties are standing together — it is a presence protocol, and correctly so. Asking a contact for money is not: the request has to survive the recipient being asleep, and the answer has to survive the asker being asleep. That flow cannot be expressed over a live call at all, and falls out of a mailbox for free.

This also covers the cases that arrive later: a subscription charge due while a payer is offline, and a receipt delivered after the fact. A vendor is expected to be reachable more or less continuously, but **nothing in the protocol may depend on that**, because the moment it does, being briefly offline becomes indistinguishable from refusing to pay.

### What is proven, and what is not

Measured between two independent nodes: a record written by one, whose node then **shut down**, opened and read by another — 702 ms to open, 330 ms to read. The writer's absence did not matter, which is the entire property.

Not yet established, and it bounds everything above: **how long a record survives without its owner online.** That is a `veilid-core` replication property, not an application one, and it sets the real ceiling on offline delivery. An implementation MUST NOT present a queued message as delivered on the strength of a successful write, because a write that lands and later evaporates is indistinguishable from one that never happened.

`app_call` remains correct for the tap (§15.3), where both parties genuinely are present and a live round trip is the right shape. The mistake was never the primitive; it was using a call where a mailbox belonged.

---


## 16.13 Money in a conversation

Asking a contact for money is not a tap. §15's flow assumes both parties are standing together, correctly — and the whole point of asking someone for twenty is that they might be asleep. So a request rides the **same log as the text around it**: sealed (§16.11), chained (§16.10), and waiting in a record until the other side looks (§16.12).

A message therefore carries a kind:

| Kind | Meaning |
|---|---|
| *(omitted)* | Ordinary text. Encoded by **omission** — an explicit zero is `MALFORMED`, because one meaning with two encodings is what §18.1 exists to prevent. |
| `PAYMENT_REQUEST` | "Please send me this much." |
| `PAYMENT_SENT` | "I sent you this much", with a transaction to look for. |
| `RECEIPT` | "I have your payment, and this is what it settles." Issued by whoever **received** the money. |

`RECEIPT` is a separate kind because it is a separate claim, and neither of the others can make it: a vendor sending `PAYMENT_SENT` would be stating they sent money, and a second `PAYMENT_REQUEST` after the fact would be asking again. A notice points at the transaction it made; a receipt points at the one it acknowledges. A request may point at neither, and may not itself be a receipt for the payment it is still asking for.

### A request carries no authority

**None.** It is a message. The payer still decides at §15.5's confirm screen, and nothing in a request shortens that path. This is not a limitation to be relaxed later: a request that could move money is a request that malware sends on your behalf, and the entire value of the confirm screen is that it stands between an arriving message and a spend.

A `PAYMENT_SENT` notice is likewise **advisory**. §17.5 verifies a payment by finding the output, never by believing a note that says one exists. A notice is a pointer that saves the recipient a search; it is not evidence, and an implementation that credits one has been told what to believe by the party who benefits.

### Reading a history back out of a wallet

A wallet stores **outputs**, because outputs are what scanning finds. A history is a list of **payments**, and the two are not the same list. Three differences, each of which produces a specific wrong screen:

- Two outputs of one transaction are one event, not two.
- **Change is not income.** Monero spends whole outputs, so paying 0.0025 out of a 0.01 output returns 0.0075 to your own wallet as a new one. Rendered unlabelled it is money arriving — and it is usually the largest figure on the screen.
- **A send leaves no local trace.** Nothing records it on the sender's side; the only on-chain evidence is that one of your outputs stops being unspent.

An implementation SHOULD therefore record, for each output, **the transaction that created it**, and MAY identify its own spends with no stored record at all: fetch that transaction and compare the key images it consumed against your own. If any matches, you sent it, and

> `paid out = (your inputs it consumed) − (your outputs in it) − fee`

This is exact, needs nothing kept locally, and so recovers payments made before an implementation kept records. It also **reconciles**: the events' net effects sum to the wallet's balance. That reconciliation is the requirement worth stating, because a history screen and a balance screen disagreeing about the same money is not a display fault — it is the wallet not knowing what it holds.

Two honesty constraints follow, and both are about not stating more than is known:

- **A received payment has no sender.** Monero does not carry one. A `PAYMENT_SENT` notice naming its transaction is the only thing that can attach a name to an arriving output, and an implementation MUST present that name as the sender's *claim*. The money is verified by §17.5; the sender is not, and never can be.
- **Before a transaction has been read, its direction is unknown.** An implementation MUST NOT assert one it cannot support. The case it would get wrong is precisely the change output that looks like income, so "still checking" is the correct row and a confident arrow is a guess.

### What the money is for

An amount alone is enough to move money and not enough to sell anything. "0.352 XMR" is not something a customer can check against what they walked out with, and a shop cannot issue that as a receipt.

A payment message MAY therefore carry **line items** — a description and an amount each — and an optional **tax**, under one rule:

> `sum(items) + tax` **MUST** equal the message's amount, or the message is `MALFORMED`.

That rule is the entire value of the feature. A breakdown printed beside a total it disagrees with is worse than no breakdown at all, because it *looks* like a check that was performed. With the rule, every itemisation on the wire is arithmetic the recipient can confirm by eye.

The surrounding rules follow §18.1's one-meaning-one-encoding discipline:

- Absent means not itemised. A **present-but-empty list is `MALFORMED`** — that is the same claim spelled a second way.
- **Tax without items is `MALFORMED`.** Tax on nothing states a split of a total the message never breaks down, which the reader can only take on faith; itemisation is worth carrying precisely because it is always checkable.
- A line item needs a description. An amount with no words is a number on a receipt with nothing to say what it bought.
- Items and descriptions are bounded. A receipt is rendered on someone else's device, so the length of the list it has to draw is not the sender's to choose without limit.

### The network fee is not the vendor's to bill

**There is deliberately no field for it.** A Monero fee is paid by the *sender* to the *network* — it is not a charge from the vendor, and it does not arrive at the vendor. A fee line inside a bill therefore charges it twice: once inside the total the customer is asked for, and again when the customer's wallet builds the transaction and pays the real fee on top.

This is not a rounding argument. It is the difference between a till that overcharges every customer by the fee and one that does not.

What the transfer cost belongs on the payer's own history, taken from their own wallet's record of the transaction they built. That is the only party that knows it and the only one that can state it truthfully; a number supplied by the party being paid is a claim, and this one does not even have the right payer.

### Where to pay

A `PAYMENT_REQUEST` names its own destination. Two reasons, and the second is the one that matters:

- **Self-contained.** The payer needs nothing from a contact record that may be stale.
- **Not stored, not reused.** An address kept against a contact gets reused, and a reused address is a public ledger entry linking every payment anyone ever made to that person. A fresh address per request costs nothing and removes that.

**This does not make the address trustworthy.** Nothing in DUCAT binds a Monero address to a persona, so a contact whose device is compromised can ask you to pay a stranger, and the request will look exactly like a genuine one. §15.5's confirm screen MUST show the destination, and an implementation MUST NOT offer a one-tap pay from an arriving message — the confirm screen is the only thing standing between a message and a spend.

The destination is inside the message, so §16.10's chain covers it: altering where a request points breaks the link.

### Amounts

Piconero, and required for both payment kinds. A payment message without an amount is a payment screen with a blank where the number goes, and an amount on a *text* message is a number nothing will honour — both are refused rather than ignored, because both would render.

The amount is inside the message, so §16.10's chain covers it: altering a request breaks the link, and a request is only as trustworthy as the thread it arrived in.

### Paying without asking first

§16.12's `ContactDetails` may carry an optional `payto`, so a contact can be paid without a request.

**This is a real trade and the specification does not pretend otherwise.** A stored address is a reused address, and a reused address is a public ledger entry linking every payment anyone ever made to that person. The per-request destination above avoids that entirely and MUST be preferred wherever the two sides can wait for a request.

It exists because the alternative is a wall in front of the ordinary case. "Ask them to send a request first" is a reasonable sentence to a protocol designer and an absurd one to somebody trying to send a friend twenty. A protocol nobody can use protects nobody.

Two rules make it a choice rather than a default:

- Publishing an address is **optional**, and an implementation MUST let a contact decline. Absent is how they decline; an empty string is `MALFORMED`, because one meaning may not have two encodings.
- A newer per-request destination **supersedes** the stored one. A contact who rotates addresses should not be undone by a copy someone kept.

An implementation SHOULD say, once and plainly, what publishing costs — and then let the person decide. Someone choosing convenience for themselves is entitled to; someone doing it without being told is not choosing.

### What this makes possible

Subscriptions, an invoice sent to someone offline, a receipt delivered after the fact, and splitting a bill among people who have already left. Each needs a counterparty who is not there, which is exactly the case a tap cannot express and a mailbox handles without special machinery.

---

# Part IV — Bonded Fast Settlement
**L4/L5 detail: making the tap actually settle in seconds**

Everything prior assumed settlement was instant. It isn't. Monero targets ~2-minute blocks and the customary finality convention is ~10 confirmations, so a naive tap-to-ride leaves the rider at the curb for twenty minutes or the driver eating unconfirmed-transaction risk. This part closes that gap with a pre-loaded, bonded consumer float.

---

## 16.14 Reactions

An emoji about a message: kind `REACTION`, the body is the emoji (at most 16 characters — enough for any emoji sequence, not enough for a paragraph, which is a message and should be one), and the target is named by sequence number — in the *recipient's* log by default, in the sender's own under a presence flag (§18.1: presence is the encoding; an explicit zero is `MALFORMED`).

A reaction is a **message like any other** — sealed (§16.11), chained (§16.10), sequenced, occupying a ring slot. The alternative, some lighter side channel, would be a second delivery path with its own ordering, its own loss modes and its own bugs, purchased to save bytes that were never the constraint. A reaction carries no money and no attachment; a later reaction from the same sender to the same target supersedes the earlier, which is how changing your mind works.

### Naming another message, generally

The target reference is not the reaction's own. Three kinds **MUST** carry one — a reaction, a `RETRACT`, and a `RIDE_ACCEPT`. Three **MAY** — a `TEXT`, which is a reply; a `PAYMENT_SENT`, naming the request it settles; and a `RECEIPT`, naming the request it receipts. Every other kind **MUST NOT**, and a reference on one is `MALFORMED`: the allow-list is the rule, so a kind that grows a meaning for it grows a vector too.

**Why the money messages matter more than the reply does.** Before this, nothing connected a payment to the bill it answered — the only thread between them was the amount, and a reader had to guess. The guess had a wrong answer in it: two identical requests answered by one payment both read as settled, because both matched. A reference replaces the inference with a statement, and request → payment → receipt becomes a chain a reader follows rather than a relationship it reconstructs.

The reference stays **advisory**, like every other claim a message makes. A payment naming a request does not make money arrive; §17.5 verifies by finding the output. What the reference settles is *which* request the sender says the money was for — a question the chain has never been able to answer, because a chain records amounts and never reasons.

**Nothing of the target travels.** A reference is a sequence number, so a reply carries no copy of what it answers. This is deliberate and it is what keeps §16.13's retraction meaningful: a message withdrawn by its sender cannot be brought back by the reply that followed it. A reader resolves the sequence against the thread it holds, and where it cannot — the message was withdrawn, deleted locally, aged out under §16.12's disappearing setting, or lost to a ring wrap — it says so and renders the reply anyway. An answer to something no longer here is still an answer, and hiding it would lose a second message to the loss of a first.

`re_own` is unconstrained on the three that MAY: a reply to one's own earlier message is ordinary, and a payment naming one's own sentence — "the £20 I said I would send" — is the same gesture pointed at a promise rather than a bill. Only `RIDE_ACCEPT` forbids it, because accepting your own offer is a soliloquy.

## 16.15 Attachments

Veilid's measured limits (from source, v0.5.7): **32 KiB per DHT subkey, 1 MiB per record**. That makes a re-encoded photograph a comfortable fit and a video a different design — so this section stops at one record.

An attachment rides an ordinary text message **by reference**. The bytes are sealed with XChaCha20-Poly1305 under a fresh key and random nonce — one use each, never reused — and the ciphertext is chunked across the subkeys of a record created for the purpose. What travels in the message is the reference: record key, decryption key, nonce, plaintext length, **ciphertext hash**, and mime type, with an optional filename. The thread's own encryption (§16.11) protects the reference, so the record on the network is indistinguishable from noise to anyone who was not a party to the conversation.

Rules, each earned:

- **Fetch, hash, then decrypt.** Bytes from the network never reach the AEAD without matching the hash the sealed message promised. The hash is of the *ciphertext*, so verification needs no key and a cache can be addressed by it.
- **All six reference fields travel together or not at all.** Every subset is a trap: fetchable but not decryptable, decryptable but not verifiable, verifiable but undecodable. A filename without an attachment names nothing and is refused.
- **One record is the unit.** An attachment is at most 1 MiB less the AEAD tag. A larger file is not a larger number; it is multi-record design work this section deliberately does not do.
- Attachments ride **text messages only**. A bill with a file in it is two features fused at their least-tested corner.
- Records are best-effort storage: a receiver SHOULD fetch promptly and cache locally (the implementation caches under the ciphertext hash), and a sender SHOULD keep its copy. An attachment that expired unfetched degrades to its message — "picture unavailable" — never to an error that takes the thread with it.

## 16.16 Read receipts

The watermark — "I have accepted your messages below sequence N" — rides the **log head**, not a message. The head is rewritten on every send and every prekey refresh anyway, so a receipt costs no ring slot, no prekey and no chain entry; a receipt-as-message would spend all three per glance, which is how a feature this small becomes a supply problem.

**Off by default, and that is the stance rather than a caution.** When a message was read is behavioural data — it reveals presence, attention and habit — and it leaves the device by explicit opt-in, never as a side effect of installing a chat app. A published watermark is the publisher's **claim**, rendered as a claim; its absence means the feature is off, and a client MUST NOT render absence as "unread".

### The ring becomes elastic

Receipts and reactions multiply message count, and the original eight-slot ring was sized for text. The head therefore carries the ring size (2..=1024; eight, the historical size, MUST be encoded by omission — §18.1). **Readers take the ring from the head, never from a constant.** The failure of a mismatch is quiet and total: every sequence maps to the wrong subkey, and a valid thread is refused as broken.

## 16.17 The hail notice

The one DUCAT object that lives on a **public surface** (§15.12's board), and the field list is short because the surface is hostile:

```
HAIL_NOTICE {
  version      (203)  uint, = 1
  card         (204)  tstr, a ducat: card URI (purpose hail, claim-once)
  dest         (205)  tstr, coarse destination or area, 1..=64 bytes
  fare_pxmr    (206)  uint, optional — the rider's offer, absent means "quote me"
  expiry       (207)  uint, unix seconds, required
  origin_cell  (208)  tstr, optional — pickup geocell, precision ≤ 6
  dest_cell    (209)  tstr, optional — destination geocell, precision ≤ 6
}
```

The two cells are the Uber-shaped triage fields: a driver reads the *job* — distance to the fare, length of the ride — before claiming, priced in privacy the board already spends, since a notice is pinned to a cell regardless. A cell finer than precision 6 (~1.2 km) is `MALFORMED`, as is one outside the geohash alphabet. Either may travel alone: a rider may say where they are and keep where they are going for the sealed thread.

Deterministic CBOR like everything else (§18.1). `version` ≠ 1, a `card` that does not begin `ducat:`, an empty or oversize `dest`, a zero `fare_pxmr`, or a missing `expiry` are `MALFORMED`. A reader MUST drop an expired notice unrendered, and MUST treat every field as an untrusted claim — the card is the only part with teeth, because claiming it is what §16.9 already verifies. No precise location field exists **by construction**: the place a notice can say is the cell it is already pinned to, plus 64 bytes of human words. An implementation wanting to say more has misread §15.12's board for a channel; the channel is the thread the card opens.

## 16.18 The listing

The second object on a **public surface**, and the one that stays there. A hail lives for minutes and describes a person about to move; a listing lives for days and describes a car or a home that does not. Everything below follows from that difference.

```
RENTAL_NOTICE {
  version      (220)  uint, = 2
  card         (221)  text, a ducat: card URI, claim-once
  kind         (222)  uint, 1 place, 2 vehicle, 3 for sale, 4 gear, 5 a skill
  title        (223)  text, ≤ 60 chars — one human line
  area         (224)  text, ≤ 40 chars — human words, never an address
  cell         (225)  text, geohash, **≤ precision 5 (~5 km)**
  price        (226)  uint, piconero, nonzero — the unit follows the kind:
                      per night (1), per day (2, 4), per hour (5),
                      the whole price once (3)
  deposit      (227)  uint, what *each* side stakes (§15.12); 0 is legal
  expiry       (228)  uint, unix seconds

  // a vehicle's searchable shape — MALFORMED on anything else
  make (229) model (230) year (231) gearbox (232) fuel (233)
  seats (234) color (235) trim (240)

  // a place's searchable shape — MALFORMED on anything else
  rooms (236) sleeps (237) size_m2 (241)

  subtype      (238)  uint, per kind — see the table below
  features     (239)  array of ≤ 8 short tags, ≤ 16 chars each
  quantity     (248)  uint, 2..=999 — how many the poster has.
                      Absent means one. MALFORMED on a skill, and MALFORMED
                      written as 0 or 1
}
```

**Almost every listing is one thing, so one is written as nothing.** A bicycle, a spare room, an afternoon of somebody's time — the count only says something for the stall with six identical kayaks, and a reader deciding whether to ask wants to know they are not competing for the last one. So `quantity` is absent for the ordinary listing and carries no bytes on a board that is expensive to read, and an explicit `1` is `MALFORMED` rather than accepted: the signature is over these exact bytes and over the slot they went into, and two byte strings that mean the same listing is the seam a signature exists to close. `0` is refused for the same reason it is never needed — an owner who has run out stops refreshing the notice rather than advertising the absence. A **skill** may not carry one at all: the price there is an hourly rate for one person's time, and "3 available" against it would describe staffing the listing does not have.

**The board is coarser than a hail's, by rule.** A hail may name precision 6 (~1.2 km) because it describes someone standing at a kerb for ten minutes. A listing describes a home that will still be there next week, so precision 5 (~5 km) is the cap and anything finer is `MALFORMED`. This is also how people actually look: nobody searches one square kilometre for a car to rent — they search a city and drive to it. A seeker reads their cell and its 3×3 neighbourhood, which is a metro area, and that is the intended granularity of the whole feature.

**What is on the board is what a stranger needs to *decide*. What they need to *arrive* is not.** The address, the plate, the door code, the photographs of someone's living room — none of it appears here. Those pass through the sealed thread after the two of them have agreed, because a listing is an advertisement, and an advertisement everyone can read must not double as a burglary brief. A client MUST NOT put an exact location, a registration plate, or a photograph on a board.

**Five kinds, two of which have a shape.** A place and a vehicle carry typed
fields because those are the things people filter on — nobody searches for a
car without caring about the gearbox. The three added in draft 0.89 carry
none: a kayak has no gearbox, a bicycle for sale has no bedrooms, and an
electrician has neither. What each of them needs is a title, a price, an
area, a category and a handful of tags, all of which already existed — so
they cost no new field numbers, and a reader that understood a listing in
0.88 understands the shape of these too. The typed fields of either shape are
`MALFORMED` on kinds 3, 4 and 5.

**`subtype` is bounded per kind**, because a category legal for a trade is not
legal for a kayak:

| kind | top | the set |
|---|---|---|
| 1 place | 2 | whole · room |
| 2 vehicle | 3 | car · van · motorbike |
| 3 for sale | 9 | goods · furniture · tools · sport · garden · electronics · music · vehicle · other |
| 4 gear | 5 | sport · tools · outdoor · party · other |
| 5 a skill | 12 | the trades and services set |

The sets are deliberately small and flat rather than a tree. They are a coarse
filter on a board that is expensive to read, every entry must be translated
everywhere a client ships, and a taxonomy fine enough to be accurate is one
nobody fits — most tradespeople are the handyman who also does electrics. What
somebody *actually* does belongs in `features`, and the fine sorting belongs to
the conversation the card opens.

**Nothing comes back from a sale, and the escrow says so.** On kinds 1, 2 and 4
the deposit is a deposit: the thing is returned and the deposit with it. On
kind 3 there is no return, so `deposit` is a **stake** — each side posts one and
gets it back on handover, and the pair of them is what makes completing beat
walking away. The money moves identically either way, which is why this needs
no new ceremony; only the words a client uses differ.

**A place has no gearbox and a car has no bedrooms.** The vehicle fields are `MALFORMED` on a place and the place fields on a vehicle. A listing that carried both would be describing two things, and a reader would have to guess which half to believe — §18.1's rule against two encodings of one meaning, applied to a surface where the encoder is a stranger.

**Enumerations are refused, not rendered.** An unknown gearbox, fuel or subtype is `MALFORMED` rather than shown as a number, and a year outside 1900–2200 is a typo or a joke. Features are a *summary* — eight short tags — because the description belongs in the conversation, where it is not being broadcast.

**The listing is the invitation; the escrow is the deal.** Claiming the card opens an ordinary sealed thread. What follows is §15.12's reservation shape unchanged: rent and two stakes in one escrow, the provider's stake landing before the payer's (the exposed side funds second), the release returning each stake and paying the provider. The suggested stakes are the ones §15.12 already argues for — 20% for a place, 30% for a vehicle — and the listing states its deposit so the reader sees the whole cost before asking rather than after.

**An owner who stops renting simply stops refreshing.** Expiry is what keeps a board honest: a reader MUST drop an expired listing unrendered, and nothing needs a withdrawal message or a delete that a hostile writer could forge.

### 16.18.1 The seal: who wrote a notice, and what it cost them

A stand's write key is `SHA-256("DUCAT-STAND-v0" ‖ 0x00 ‖ cell)` (§15.12). Everybody who can *find* a board therefore holds the key to every slot on it, and no design here changes that: **anyone can overwrite anything.** That is what having no operator means. Three narrower things are still worth establishing, and both notices carry them — `RENTAL_NOTICE` in fields 242–244 and 249–250, `HAIL_NOTICE` in 245–247 and 251–252.

**Be precise about what a seal never priced.** Denying a *slot* is free and cannot be made otherwise: junk with no stamp at all still occupies the record, and a write at `u32::MAX - 1` leaves the slot unwritable for good. §15.12's board generations are the answer to that one. What a seal prices is **readable spam** — a hundred and twenty-eight plausible listings a browser has to wade through — and it is worth not claiming more.

**A notice says who wrote it.** Not who owns the slot; nobody owns a slot. Who authored the bytes. That is what makes substitution visible: copy a listing, swap in your own card, and it comes back as a *different author*, which a reader who has seen the original before can be told about. The signature is over `board ‖ 0x00 ‖ subkey(4, LE) ‖ 0x00 ‖ beacon_height(8, LE) ‖ 0x00 ‖ beacon_hash(32) ‖ 0x00 ‖ body`, under object type `BOARD_NOTICE`, where `body` is the notice re-encoded with these five fields removed. **The slot is inside the signature**, so a valid notice cannot be lifted onto another one — without that binding, an attacker holding the public write key could paper a whole cell with one signed listing and have it read as that person flooding the board.

The signing key is **per listing**, not the poster's persona: `SHA-256("DUCAT-LISTING-v0" ‖ 0x00 ‖ persona_secret ‖ 0x00 ‖ listing_id)`. A board is read by everyone, and a persona here would publish which persona posted which listing to anybody browsing a marketplace, linkable against every contact who already knows it — §16.3 keeps transactions anonymous and a signature must not undo that for a bystander. Per-listing derivation is stable across that listing's refreshes, which is the whole property a reader needs, and unlinkable to anything else.

**Writing one costs something, and the cost has to be memory.** A nonce such that `Argon2id(password = nonce(8, LE), salt = SHA-256("DUCAT-POW-v1" ‖ 0x00 ‖ signed ‖ 0x00 ‖ sig)[0..16], m = 4096 KiB, t = 1, p = 1, out = 32)` shows at least **8** leading zero bits. The nonce is the password and the notice the salt so that the listing goes through SHA-256 once per notice rather than once per attempt; Argon2 has no midstate to clone, so the saving has to come from the shape of the call.

The earlier construction wanted 20 bits of a SHA-256 search, and its stated cost model — a cell in about a minute, a region in a couple of hours — was true only of an attacker using a CPU. A commodity GPU runs SHA-256 some three orders of magnitude faster than one core, which turns those hours into seconds, and rented mining hardware is faster again by orders of magnitude beyond that. **A price that collapses by 10³ in the attacker's hands is not a price.** Argon2id is bounded by memory bandwidth rather than hash throughput, so the same hardware buys perhaps one or two orders less. This does not make an attacker equal to a phone; it stops them being three thousand times better, which is the honest claim and the whole of the gain.

The memory is 4 MiB rather than more because **the reader pays too, and the reader is a phone.** A sweep opens up to eighteen boards of eight slots and every notice costs one evaluation whether it is honest or not. An implementation MUST verify the signature *before* the work: a slot full of random bytes is then refused for the price of one Ed25519 verify instead of costing every reader on the board a memory-hard evaluation to say no, and a defence whose failure path is the expensive one is a denial of service with extra steps.

**And the work has to perish.** Every other field in the preimage is the poster's own — cell, slot, body, signature — and a board name's generation is a floor division of the clock, so an attacker could once have mined every slot of every cell in a region for every epoch of the coming year in a single afternoon and posted at no marginal cost thereafter. §15.12's rotation assumes re-poisoning is paid for again each week; that assumption was not true. A notice therefore names the **Monero block it was stamped against**: `beacon_height` and the 32-byte `beacon_hash`, both inside the signature, so neither can be restated once the work is done.

A reader with a view of the chain MUST refuse a notice whose `beacon_height` is more than **720 blocks** below its own tip, or more than **2** above it — a day back, and a couple of blocks of slack for a reader whose node lags the poster's. A day rather than the hour a precomputation argument alone would want, because the limit here is not the attacker but the reader: a phone that has been in a drawer would otherwise show an empty marketplace and no reason for it. That still collapses the precomputation window from fifty-two weeks to one day.

**And it MUST confirm that the height really carries that hash before displaying the notice.** Not SHOULD: the height test alone secures nothing, and its failure is total rather than partial. Monero aims at a block every two minutes, so a height months away is predictable to within a few hundred — an attacker mining in January knows August's tip closely enough to pre-mine a spread of future heights against block hashes they simply *invented*, and every reader that runs only the cheap comparison accepts the lot. Precomputation is entirely back against that reader, which is the thing the beacon exists to end. **A beacon nobody looks up is 32 bytes the attacker chose.**

The cost is small and does not scale with the attack. One `on_get_block_hash` is 119 bytes; an answer is good for ever, including the answer *that is not its hash*, so a doctored slot costs one lookup once. A reader tracking the tip records that block's hash for free with every poll — which is the height honest posters are stamping against — so in practice it has already answered most of what a board will name. A reader SHOULD bound how many *new* heights one board may make it ask about, because the heights an attacker names are whatever they like and a lookup per notice would turn a doctored cell into network amplification pointed at everyone reading it. The whole window is 720 lookups and 86 KB, which is the ceiling and only an attacker pays to approach it. Fetching the window in bulk is the wrong trade: `get_block_headers_range` over 720 blocks is 624 KB, seven times the worst case, paid in full every session for heights nobody named.

**Three answers, not two.** Confirmed, refused, and *not yet knowable* — and the third MUST NOT be shown. A notice stamped up to two blocks above the reader's own tip is inside the window and carries a hash the reader cannot check yet; it is held for the minutes until the tip catches up, rather than displayed on the height test alone. Same for a height whose lookup failed or fell outside the reader's budget. Collapsing "cannot say" into "yes" is precisely the reader class the paragraph above describes, so the forward slack carves no exception into the MUST: it keeps an honest notice from being *refused*, and never causes one to be shown unchecked.

A reader deciding whether a slot is **occupied** — as a poster does before writing — applies the window and not the confirmation, deliberately. A notice whose block cannot be confirmed yet is most likely an honest poster whose node is slightly ahead, and overwriting it would do the damage the confirmation exists to prevent. Only what is shown to somebody must be confirmed.

**A reader with no chain view skips both tests** and judges the notice on its signature and its work alone. Reading a board has never required a Monero node and this does not make it require one: a marketplace that goes dark because a daemon is unreachable is a worse answer than the spam it was avoiding, and an attacker cannot choose which readers have a node. Decoding itself MUST NOT consult a chain or a clock — freshness is the caller's judgement, made with the caller's own view, for the same reason §18.9 gives about the clock.

The five are one mechanism. A notice missing any of them is refused rather than treated as legacy — an accepted unsigned or unstamped notice is the way to skip the work entirely, and a defence with an opt-out is not one.

The seal carries no version of its own, and does not need one: it is not part of the notice it wraps, and every way of getting it wrong fails closed. A reader from before draft 0.89 refuses a stamped notice because its strict reader does not know fields 249–252; a reader from after refuses an unstamped one because they are absent. Boards written before 0.89 are therefore abandoned rather than migrated, which for a mechanism whose entire argument is that there is no unstamped path is the right direction to fail.

**Expiry is bounded** at `MAX_NOTICE_TTL_SECS` (31 days), and by the *reader* rather than at decode. The repetition above is what the cost model rests on, and a notice good until the next century turns one payment into a permanent squat that no honest client will clear, since clearing another writer's slot is refused. Decoding must not consult the clock: the conformance vectors pin exact bytes to exact outcomes, and a time-dependent decoder would begin failing on its own with nothing changed.

### 16.18.2 The publication listing (`PUB_NOTICE`, fields 265–275)

A digital good discovered like a physical one — on a board — and
delivered like a subscription, because after discovery it IS one
(§16.20). The notice carries what a stranger needs to decide: a title
(≤60 chars), an optional one-sentence blurb (≤280 — the description
belongs in the issues), an optional price in piconero per §16.20 period
— **absent means free, and an explicit zero is `MALFORMED`**, because
one meaning gets one encoding (§18.1) and the signature is over these
bytes — an expiry, and a claim-once `ducat:` card of purpose `publish`.
**Claiming the card is subscribing**: the enrolment, the billing on the
thread, the shelf and the swarm are all §16.20 unchanged. The version
is 1; the strict reader refuses everything it does not know.

**Where it lives is the board name, not a field.** A worldwide listing
posts to `topic:<category>[.<lang>]` — category from the pinned set
below, language an optional BCP-47 primary subtag — and a local one to
the same `local:<cell>` boards every listing uses; cross-posting is two
stamps, paid honestly. The board name sits inside the seal's signature
(§16.18.1) exactly as a cell does, so a notice cannot be lifted onto
another topic any more than onto another slot. The stamp block —
poster, signature, work, beacon — is §16.18.1 verbatim in this notice's
own field namespace (271–275), with the same Argon2id price and the
same Monero-block freshness.

The categories, pinned so every implementation shards the same way:
`news`, `serials`, `sound`, `software`, `art`, `other`. Boundary calls
are settled here rather than argued on boards: fonts are `software`;
courses are `other`. Adding a category is an appendix row in a later
draft; removing one strands every board that used it, which is why the
set starts small and `other` is the pressure valve.

## 16.19 Small groups over pairwise threads

A group is §17.9's roster pattern carrying words instead of DKG rounds: a member list, then fan-out — the sender seals the same body into each member's existing pairwise thread. There is **no group key, no shared record, and no new object on the network** that says *these N people are a group*. Every property a thread has — §16.11's forward secrecy per pair, prekey partitioning, deniability — comes along unchanged, because nothing changes about how a message is sealed. The cost is stated rather than hidden: N−1 writes per message, which bounds this at *small* — a household, a stall's two phones, the three people organising a thing — and that bound is the shape, not a limitation to engineer away.

**Authenticity needs nothing added.** Each copy travels in a pairwise thread whose keys only its two ends hold, so a member cannot write as another member — the property a shared group key gives away first. What no signing can add is cross-member consistency: a sender *can* say different things to different members, and only a shared record could prevent it, at the price of everything above. The group is N conversations that agree because their participants do.

### Identity and reference

- A group is **16 random bytes**, minted at creation, plus a name fixed at creation. The id travels only inside sealed messages; a contact who was never told it cannot name it.
- Every group message carries the group id (field 253) and the sender's own counter within the group, `group_seq` (254) — **together or not at all**. `(sender, group_seq)` is the one name a group message has that every member can resolve: the same body fans into N threads and takes a different pairwise `seq` in each, so the thread sequence stops naming anything shared. A pairwise `re_seq` on a group message is `MALFORMED` for exactly that reason (§18.1: one meaning, one encoding).
- A reply or reaction inside a group targets by **group reference**: the target's sender persona (255) and that sender's counter (256), both or neither, and only on a message that carries a group id.
- Kinds that travel in a group: `TEXT`, `REACTION`, `RETRACT`, and `GROUP_ROSTER` (kind 12). **Money stays pairwise** — a bill to a group is N debts wearing one number, and every settlement rail here is pairwise or a ceremony. The ride and ceremony kinds are two-party by construction.

### The roster is a grow-only set

`GROUP_ROSTER` carries the member list in its payload (canonical CBOR; bounded like a ceremony round's). The creator's first roster **is** the invitation. Any member may add anyone: adding sends the grown set to everyone including the newcomer. **Nobody can be removed.** That is not a policy shortfall — removal is the one roster operation that needs a consensus a peer-to-peer group cannot have, and nothing can un-tell a person a group id. A grow-only set needs no consensus at all: merging two views is a union, unions commute, and every member's roster converges to the same set whatever order the adds arrive in.

Admission is the one rule with teeth: **for a group already known, a roster update is accepted only from a persona already in the local member set.** The first roster for an unknown id, from any contact, creates the group — any contact can invite you to a *new* group; only members can grow an existing one. A non-member who learns the id cannot add themselves.

### The mesh, and who checks it

Fan-out writes into existing pairwise threads, so a member can only reach members they hold as contacts. The requirement is therefore **full mesh: everyone holds everyone.** No coordinator verifies this, and none is needed — contact edges are mutual, every edge has two ends, and each end can check its own edges locally. *Every member's local check passing is exactly the mesh being complete.*

Enforcement follows the check: **receiving is never gated** (the bytes arrive in threads already trusted, and rendering what arrived is honesty), but **a client MUST NOT send into a group while its own mesh is incomplete**, and MUST say who is missing rather than dimming a button. Since any member who can send holds the full roster, anything sent reaches everyone — the silent partial delivery that plagues naive designs is structurally impossible: a message either reaches the whole group or its sender was told, before typing, why it could not.

### What is stated plainly to the person

A client MUST disclose, at creation and on first opening a group one was added to: that everyone must hold everyone before sending; that any member can add and nobody can be removed; that a newcomer sees nothing from before they joined (there is no shared history — "no history" is not a policy but the absence of any store to have one); that messages cannot be forged member-to-member; and that leaving is local — it stops your sending and rendering, and cannot make the others stop.

Ordering across senders is arrival order, and two phones may disagree about interleaving; per-sender order is exact (`group_seq` is monotonic). The group reference is what makes that honest: a reply names its target outright, so adjacency stops carrying meaning it cannot bear.

## 16.20 Publications: membership is the paid thread

The subscription machinery (§16.13's recurring bills) meets content. A
**publication** is sealed content on the DHT plus a paying relationship per
reader — no platform, no member list anywhere except the publisher's own
threads, and no credential to leak because none exists.

**The shelf.** A publication's content lives in DHT records the publisher
writes — chunks sealed XChaCha20-Poly1305 under a per-period content key,
with the landing site (record key **and** subkey index) as associated data,
so a chunk moved between records or shuffled between slots fails to open
rather than decrypting out of order. The publication's root record — the
shelf — holds an index sealed under a standing **head key**; the index is
how a reader finds the period records. To an observer holding neither key,
the shelf and every chunk are noise: content addressing here does not
testify about what is being shared.

**The keys derive; they are not stored.** The publisher holds one 32-byte
master secret. A period's content key is `keyed_hash(derive_key("DUCAT
publication period v1", master), period_id)` — both steps BLAKE3, the
transport's own hash. Selling a back-catalogue period to a new member is a
re-derivation, not an archive lookup, and restoring the phone restores
every key ever issued because it restores the one secret they all come
from. A **period id** is the publisher's label ("2026-09", "issue-12"), at
most 64 characters, never parsed for meaning by the reader.

**The handover (kind 13, `PUBLICATION_KEY`, fields 257–260).** When a
reader's payment for a period settles, the publisher's reconcile loop —
the same one that already auto-sends receipts (§15.11), with the same
mark-before-send discipline — sends the period's key down the thread:

| field | content | rule |
|---|---|---|
| 259 | `period_id`, text ≤64 | with 260 or not at all |
| 260 | `period_key`, 32 bytes | with 259 or not at all |
| 257 | `record_key` — the shelf | first delivery; with 258 or not at all |
| 258 | `head_key`, 32 bytes | with 257 or not at all |
| 261 | `swarm_key` — a heavy period's share, text ≤128 | with 262 or not at all |
| 262 | `swarm_digest`, 32 bytes | with 261 or not at all |

The pairs travel whole or not at all: a half is `MALFORMED`. The key IS
the kind — a `PUBLICATION_KEY` without the period pair is `MALFORMED`, and
any of these fields on another kind is `MALFORMED`. No amount rides it
(the bill travelled separately and settled before this message existed).
The body MAY carry a note in the publisher's language, like any message.

**The shipment (fields 261–262).** A period too heavy for the shelf —
an album, an archive — ships by swarm (the vendored engine under
`mobile/vendor/`, BLAKE3 pieces, every peer a seeder): the manifest MAY
carry the share key and the index digest beside the period's key, and
the reader fetches with both, verifying every piece against the digest
it was promised on the thread. The pair travels whole or not at all,
and only aboard a publication key. Content fetched by swarm is the same
ciphertext the shelf would hold — the period key opens it either way,
so the truck can be swapped without the club noticing.

**What this deliberately is not.** The message hands over a capability,
never content — the thread stays small while the shelf holds the weight,
which is what lets a publisher's device serve hundreds of readers with two
tiny messages per reader per period while the network serves the bytes.
Keys, once handed over, are the reader's: there is no revocation, and a
missed payment costs the *next* period's key, never one already paid for —
cancelling is stopping, exactly as §16.13's recurring bills promise. A
reader who shares a key outside the club can — as they could the content
itself; the protocol prices copying at its true cost, zero, and leaves the
social contract where it lives.

**Client rules.** A publisher MUST derive period keys as above (two
implementations inventing different schedules would strand every restored
back-catalogue); a reader MUST treat `period_id` as an opaque label and
`period_key` as opaque bytes; a reader SHOULD file keys by (publisher
persona, period id) and keep them across thread deletion — the receipt
outlives the small talk, and so does what it paid for.

## 16.21 Calls: the door is a message

Voice between two people who already share a thread. The thread does the
*signalling* — everything with a truth to keep — and the media never
touches it.

**The handover (kinds 14–15, fields 263–264).** A `CALL_OFFER` (kind 14)
carries a freshly allocated private-route blob (field 263, 1–4096 bytes —
blobs embed the full peer-info of their hops, so size follows network
shape: one desk measured 832, one phone overflowed 1200; past the cap
something is being smuggled that is not a route) and a call id (field 264, eight random bytes). A
`CALL_ANSWER` (kind 15) carries the callee's own route and quotes the
offer's id, so an answer names its call even when two offers cross. Each
pair travels whole or not at all; the door IS the kind — an offer or
answer with no route is `MALFORMED`, a route on any other kind is a door
held open where no call is happening; no amount rides either kind.

**Media.** Frames flow as Veilid app-messages on the exchanged routes,
one route per direction, allocated for this call and released when it
ends — a call's route is never the mailbox's. Client format v1: an
8-byte header — frame sequence (u32be) and sender-relative milliseconds
(u32be) — then one Opus packet, 16 kHz mono, 20 ms per frame, **hard
CBR at 24 kbit/s** (a 60-byte packet, every frame). Constant bitrate is
a privacy rule, not a tuning choice: encrypted variable-bitrate voice
leaks speech through packet sizes alone — phrase spotting and phoneme
reconstruction are published attacks — so a frame must leave the same
size whether the speaker is talking or holding their breath. For the
same reason endpoints MUST NOT enable DTX. The measured ground
(research/post-1.0/CALLS.md): p50 RTT 187 ms through default private
routes, 500 of 500 frames delivered at 50 Hz, 65 ms jitter —
mouth-to-ear lands near 260 ms with full cover.

**Control frames.** A frame whose sequence field is the sentinel
`0xFFFFFFFF` is control, not sound: one byte of type — 1 `ANSWER`, 2
`DECLINE`, 3 `BYE` — then the eight-byte call id, then for `ANSWER` the
answering side's route blob. `ANSWER` and `DECLINE` ride the offer's
route the moment the callee decides, because the door is already open
and the mailbox costs two cold DHT trips; the sealed `CALL_ANSWER` or
Retract MUST still follow by mailbox as the canonical record — the
frame is the fast provisional word. A control frame is honored only
when its id matches a live call on the route it arrived by; possession
of route and id — which exist only inside the sealed offer — is the
same trust the media itself stands on. `BYE` ends a call at route
speed; silence remains the fallback the watchdog hears. Type 4 `RENEW`
carries a fresh route blob for the SENDER's own receiving direction:
routes are drawn from the network and some draws are bad (a live pair
measured 71% one-way loss beside a perfect reverse), so a starving
receiver allocates a new door mid-call and hands it over on the
direction that still works; the far side simply re-aims its media.
Old and new doors both stay open until the call ends — late frames are
late, not wrong. Types above 4 are ignored, not errors: the media
channel is two consenting endpoints, not the sealed wire.

**Ending things.** Declining is §16.13's Retract naming the offer's
sequence — the same word the till uses to take back a bill, now usually
preceded by its `DECLINE` frame. Hanging up is `BYE`, or simply
stopping: release the route, stop sending; the far side's watchdog
treats silence as the end, exactly as the swarm treats a quiet stream.
A missed call is the offer read later — ringing was a message all along,
which is why missed calls need no second channel.

**What this deliberately is not.** No conference rooms, no voicemail
service, no TURN infrastructure to subpoena: two routes between two
paid-up correspondents, end to end, with the same cover traffic as
every other DUCAT byte.

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
  bond_ms        : 2-of-3 multisig { user, arbiter_key_A, arbiter_key_B }
                   both arbiter keys independently held by the market's
                   arbiter set (§10.1) — see "what the bond actually is"

  bond_amount    : user-chosen, e.g. ~$100 equivalent
  spend_ledger   : local record of in-flight (unconfirmed) obligations
}
```

- **What the bond actually is, stated plainly.** Earlier drafts wrote the third key as `(recovery)` and never said who held it. That could not be filled. If the user holds it, `user + recovery` lets them empty the bond before being slashed and collateral means nothing. If a stranger holds it, that stranger plus the arbiter set can move the user's funds — the exact 2-of-3 structure exploited in §2.5. A pre-signed timelocked refund would have squared the circle, but **Monero's custom `unlock_time` is already unusable**: a relay rule blocks such transactions today and the feature is removed at consensus with FCMP++.

  So the honest resolution is to name the structure rather than disguise it. **Both non-user keys belong to the market's arbiter set**, independently held. That yields:

  | Pair | Effect |
  |---|---|
  | `arbiter_A + arbiter_B` | The market can slash without the user — which is what §17.5 requires |
  | `user + either arbiter` | Normal withdrawal, needing the market's cooperation |
  | `user` alone | Nothing. The user cannot outrun a claim |

  **Demonstrated on stagenet.** A funded 2-of-3 bond was seized by `arbiter + recovery` with the user's wallet never contacted — not for signing, and not for key images (`monero-spike/REPORT.md`). The second half of that was not guaranteed in advance: Monero reconstructs key images from participants' partial ones, and had it required *all three* exports, a bond could only ever have been taken with the cooperation of the party being taken from, which is not collateral. It requires only the threshold count. Seizure over the holder's objection works.

  **A bond is therefore a deposit held under the market's threshold control, not self-custodied collateral.** This costs A1: while locked, the bond is not bearer-held, and the user depends on the arbiter set both to survive and not to collude. **If a market dies, its bonds are forfeit** — there is no recovery path, and inventing one would hand the user a key that defeats slashing. The mitigations are that bonds are small by design and that market choice is now visibly a financial commitment, not just a namespace subscription (§10.1).

- **Most users should never form one.** Zero-conf risk is bounded by the transaction value: a merchant accepting a $4 coffee unconfirmed risks $4, and no collateral improves that trade. Bonds earn their complexity only where a single transaction's value is large relative to the deposit — rides, not coffees. Providers set `accept_unbonded` accordingly (§17.6), and a client SHOULD NOT prompt for a bond that the user's transaction sizes do not justify. This scopes the deposit problem out of most of the protocol's surface rather than solving it everywhere.

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

  **Measured on stagenet, and the rule is sharper than value arithmetic suggests.** Seven consecutive payments of 0.0005 XMR each consumed exactly 0.005 of unlocked balance — a tenfold discrepancy, identical every time — and the eighth was refused with nothing unlocked:

  ```
  payment 1 ok  unlocked 35000000000      payment 5 ok  unlocked 15000000000
  payment 2 ok  unlocked 30000000000      payment 6 ok  unlocked 10000000000
  payment 3 ok  unlocked 25000000000      payment 7 ok  unlocked  5000000000
  payment 4 ok  unlocked 20000000000      payment 8 REFUSED       0
  ```

  **A payment costs an entire output regardless of its size**, because the change returns locked. Consecutive capacity is therefore a *count*, not a balance: seven unlocked outputs bought seven payments and an eighth was impossible while 0.05 XMR sat in the wallet. A float holding a single large output can make exactly one payment per lock interval no matter how much it holds.

  So a client MUST surface two different numbers, and conflating them is the bug §17.2 exists to prevent:

  ```
  single payment capacity      = unlocked_output_value − fee_reserve
  consecutive payment capacity ≤ count(unlocked outputs)
  ```

  **The second is a bound, not an equality, and an earlier draft got this wrong.** A drain test spending to exhaustion predicted six consecutive purchases from six unlocked outputs and achieved four:

  ```
  purchase 1:  6 outputs → 4 outputs    (consumed 2)
  purchase 2:  4 outputs → 2 outputs    (consumed 2)
  purchase 3:  2 outputs → 1 output     (consumed 1)
  purchase 4:  1 output  → 0 outputs    (consumed 1)
  purchase 5:  refused
  ```

  **Input selection belongs to the wallet, not to the client.** A transaction may draw on more than one output — for fee coverage, for consolidation, or because the wallet prefers multiple inputs — so consecutive capacity is at most the output count and can be roughly half of it. An earlier measurement in which seven outputs yielded seven payments was a case where one input happened to suffice each time, and reading it as an equality was over-fitting to a single run.

  Practical consequences:

  - **Pre-split more finely than the naive calculation suggests.** If a client wants to guarantee *N* consecutive payments it should provision meaningfully more than *N* outputs.
  - **Treat the figure as an estimate that must degrade gracefully.** A client MUST check capacity before presenting an offer and refuse there — a refusal at the confirm screen is recoverable, a failure at settlement leaves a customer waiting on a payment that will not complete.
  - **Never promise an exact count to a user.** *"About 4 more payments"* is honest; *"4 more payments"* is not, because the client does not control the arithmetic that decides.

  A client that reports capacity from `hot_balance` will promise fares it cannot pay. **Two further traps, both observed:**
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
- `bond_proof` — rider's bond attestation: `{ bond_ms_address, bond_amount, arbiter_set_id, capacity_bucket, sig_by_bond_key }`, freshness-bounded and signed. **`capacity_bucket`, not an exact figure** — see §17.8.

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
- **Evidence is compact and verifiable.** The claim carries `{ signed ACCEPT, TXPROOF, RECEIPT, txid }` — and this is the one place a proof is irreplaceable, since the arbiter is not the recipient and cannot scan for itself. No he-said/she-said; this is the *easiest* possible dispute class, which is why fast-settle disputes are cheap to arbitrate.

  **Measured end to end on stagenet (0.47).** The payer generated an `OutProofV2` over a real settled transaction and a **third wallet — neither payer nor payee — verified it**: `good: true, received: 600000000, confirmations: 370`. The proof is 142 characters, so carrying it inside a claim costs nothing.

  **The proof MUST be bound to the transcript.** Monero's proof covers an arbitrary `message` chosen at generation time, and the obvious implementation leaves it empty. Setting it to the transcript's chain link makes the proof non-transferable between disputes; leaving it empty means any proof the payer ever generated for that transaction can be replayed into an unrelated claim. **Monero enforces the binding itself** — the same measurement, re-run with a different message, returned `good: false, received: 0` — so this costs one field and nothing else.

  **On implementation availability**, an earlier draft said proofs were DUCAT's own work. That is true of `monero-oxide`'s `monero-wallet`, which has no proof support, and **not** of `monero-wallet-rpc`, which exposes `get_tx_proof` and `check_tx_proof` and is what the measurement above used. A client embedding a wallet (§8.2's intended path) still owes this work; a client driving wallet-rpc does not.
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
- **Bond capacity as a side channel — bucketed as of 0.48.** An exact `capacity_remaining`, shown to every provider a rider taps, is a running meter on that rider's spending: two merchants comparing notes recover what was spent between the taps, and one merchant seen twice recovers it alone.

  The payee's real question is narrower than the field answered — they need **capacity ≥ fare**, a predicate, and were handed an integer. A predicate cannot be signed per-fare, because the attestation is signed ahead of time by the bond key and the fare is not known then. So `bond_proof` now carries **`capacity_bucket`**: the largest value of a fixed 1–2–5 ladder not exceeding the true capacity.

  Three rules, each doing work:

  - **Round down, always.** Rounding to nearest would let a bond claim capacity it does not have, converting a privacy feature into a solvency lie — and the party who benefits from the overstatement is the one publishing it.
  - **Ladder membership is part of the wire format.** Two clients bucketing differently would disagree about whether the same bond covers the same fare. A payee MUST reject a `capacity_bucket` that is not a ladder value, because an arbitrary integer here defeats the mechanism entirely: a rider could publish their balance exactly and call it a bucket.
  - **The check is one-directional.** A bucket covering the fare means the bond definitely covers it; a bucket falling short means only that it cannot be *proven* at this granularity.

  **The honest cost is a false negative**, and it is real: a rider with 0.004999 XMR of capacity facing a 0.0045 fare can pay and cannot prove it, and is refused. They top up to cross the rung. The leak falls from 64 bits to under 4.1.
- **Cure window abuse.** A rider could habitually underpay fees, force cure windows, and slow-walk drivers without ever being slashed. Track cure-window invocations against the bond and degrade it after repeated use.
- **Multisig setup remains the fragile step.** Amortized, off the critical path, and retryable — but still the least-proven machinery in the stack (main spec O1 stands).

---

## 17.9 The ceremony DUCAT owns: interactive DKG and FROST over the mailbox

§8.2's audit (changelog 0.50) found the seam plainly: **`dkg` 0.6.1 ships no interactive DKG**, so a deployment must bring its own ceremony between distrusting parties. The threshold library gives the rounds — PedPoP commitments and encrypted shares to build the key, FROST preprocess and signature shares to spend it — but it does not carry them anywhere. DUCAT carries them on the one channel it already has for two parties who have committed to each other: §16.12's sealed thread. Nothing new about transport, delivery, or ordering is invented; the ceremony is a sequence of ordinary sealed messages, and every property the thread already has (forward secrecy, chain-linked ordering, the patience windows, the read-before-write ring) applies to it unchanged.

The load-bearing crypto is proven: `mobile/examples/escrowtest.rs` builds a real 2-of-2 wallet on stagenet and releases it by FROST, both signers exchanging only the serialized wire messages — the same bytes this section seals — and asserting they derive one identical transaction. What the example fakes is only *who runs the machines* (one process, for the demo); this section says how they run on two devices that never share a secret.

### Message kinds

Three kinds carry the ceremony, all on the §16.10 message (kinds 5–7 belong to §15.12's ride ceremony; the bond ceremony takes the next block):

```
DKG_ROUND     8   a DKG wire message: round tag + opaque bytes
FROST_ROUND   9   a signing wire message: session id + round tag + opaque bytes
CEREMONY_ABORT 10 this ceremony is abandoned; state may be discarded
```

Each carries an opaque `payload` (field 214) — the serialized PedPoP or FROST message, which DUCAT does **not** parse; the threshold library validates it, and a malformed one aborts the ceremony rather than corrupting a thread. A `FROST_ROUND` proposing a release (round 0) MAY additionally carry an amount: what the proposal claims the funder gets back, so the consenting side's screen states the split beside the signed payload (§15.12's settlement). It is a statement, not authority — nothing verifies it but the eventual chain — and answering rounds carry none; every other ceremony kind still refuses an amount. A `round` byte (field 215) says which step it is, so a reader rejects a round it did not expect (the §2.5 discipline: out-of-order ceremony messages are refused, never applied). A `ceremony_id` (field 216) — the 32-byte thread-derived context that PedPoP requires be unique per multisig, and that binds every message to one escrow — so a stale message from an abandoned attempt cannot be replayed into a live one.

### The build: three rounds, both directions

For the 2-of-2 bond (rider + arbiter) or 2-of-3 (rider + two arbiter keys), the PedPoP flow is:

1. **Commit.** Each party runs `generate_coefficients` and sends its `DKG_ROUND{round: commit}` carrying the commitment message. A party that sends two commitments for one `ceremony_id` is faulty and the ceremony aborts (the library says so; DUCAT enforces it by accepting one per round per sender). The round-0 payload frames the commitment behind the ceremony's self-description — the sorted roster, the arbiter's index, a kind (`bond` or `ride`), the funder's index, the fare, and the roster nonce — because a pairwise thread names only two of three parties, and the third learns who else is in the room, and what the money is for, from the invitation itself. A joiner MUST verify the roster hashes to the ceremony id, and a client MUST process ceremony rounds serially: two mail polls dispatching one invitation concurrently joined twice, double-committed, and the racing sends cost a ring slot (found live, 2026-08-16).
2. **Share.** Having received every commitment, each party runs `generate_secret_shares` and sends each counterparty its `DKG_ROUND{round: share}` — the *encrypted* share, readable only by its addressee. The share is encrypted by the DKG layer independently of the thread's own sealing; the double wrap is deliberate and cheap.
3. **Confirm.** Each party runs `calculate_share`, arrives at its `ThresholdKeys`, and both independently compute the same group key — the escrow's spend key, which no device holds in full. The view key is derived from the group key by the §8.2 rule (fresh per group, since the FROST key has no private half to derive it from), so both parties, and only they, can scan the escrow. The escrow's funding address is now known to both without either learning the other's share.

The whole build is four sealed messages (two each way) for 2-of-2. It happens once, at bond load or rental agreement, off any curbside critical path — exactly §17.1's "calm, retryable onboarding" — and its failure mode is a clean abort with a retry, never a stranded party.

### The spend: FROST release, bound to a reason

Releasing the escrow — the deposit returned, the bond withdrawn, or an arbiter's `RULING` executed — is a FROST signature over one Monero transaction whose destination is fixed by the outcome:

1. **Preprocess.** Each signer runs `preprocess` and sends `FROST_ROUND{round: preprocess}`.
2. **Sign.** Having received the other's preprocess, each runs `sign` over *the same* `SignableTransaction` and sends `FROST_ROUND{round: share}`.
3. **Complete.** Either signer completes to the finished transaction and broadcasts it. Both derive the identical transaction hash (the example asserts this), so it does not matter who broadcasts.

The transaction both parties sign is not blank: its outputs are the release destination, and a signer MUST refuse to `sign` a transaction whose destination does not match the agreed outcome — a return-to-rider on a clean ride, the driver's address on a completed one, the arbiter's `RULING` allocation on a dispute. **This is where §15.5's WYSIWYS survives into escrow**: a co-signature is consent to a specific destination, and a client that signs whatever bytes it is handed has deleted the confirm screen. The `RULING` is itself a co-signature (§9.3), which is why an expired dispute must emit a real ruling rather than nothing (§9.3.4): "do nothing" leaves funds locked behind two signatures that will never both come.

### What this does not solve

- **The arbiter must run the ceremony.** A 2-of-3 bond needs the arbiter set online to build the key and, on dispute, to sign the ruling. This is the arbiter-set-governance open problem (§17.8) wearing work clothes: a captured or absent arbiter set makes the bond unspendable, not merely untrustworthy.
- **O22 stands.** A rider who loses their device mid-escrow loses their share, and a 2-of-3 buyer-favourable outcome then needs the *other two* keys — which for a bond is the arbiter set (fine) but for a two-party escrow is the counterparty (a hole with no clean answer, §4.3).
- **The library's own caveat is inherited.** `dkg` provides the rounds and explicitly does *not* provide the completion-consensus that confirms all parties finished; DUCAT's confirm step is the thread observing both `ThresholdKeys` produce the funding address, which is weaker than a consensus protocol and stated as such.

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
- **Nesting depth MUST NOT exceed 16 levels.** A deeper structure is `MALFORMED`.
- **No negative integers.** CBOR major type 1 is `MALFORMED`. No object in the protocol carries a signed number — money is unsigned piconero (§18.2), map keys are unsigned, and every timestamp, duration, and count is unsigned.

Both of the last two rules were added at 0.45 and both came from the same place — §18.11's second implementation — but they are different kinds of gap. The depth bound was **missing**: the reference enforced it and the document never said so. Negative integers were **unspecified**: the reference accepted them, the second implementation refused them, and neither was wrong. The second kind is the more dangerous, because nothing was inconsistent — two conformant clients simply behaved differently, and no test could have noticed.

It was resolved toward refusal because the directions are not symmetric. **A decoder that later starts accepting a value type is a clean extension; one that later starts refusing what it used to accept breaks every peer already relying on it.** Strict first is the only reversible choice, and this reasoning applies to every future addition to §18.1.

**The depth bound is normative and its exact value matters**, which is why it is a number here rather than "implementation-defined". Two reasons, and the second is the one that is easy to miss:

- *Denial of service.* Nesting costs one byte per level, so a 19-byte payload can drive a recursive decoder 17 frames deep and an arbitrarily small one can exhaust the stack. A parser reached over an anonymous transport by unauthenticated senders (§15.3) cannot afford an unbounded recursion.
- *Interoperability.* A limit left to the implementer is worse than none, because two clients choosing 16 and 32 will disagree about the same bytes — one accepting a signed object the other calls malformed, which is exactly the divergence §18.1 exists to prevent. The bound is part of the wire format, not a local hardening choice.

This requirement was absent until 0.45, and its absence was found the way it should have been: a second implementation written from this section accepted a payload the vector set rejects (§18.11).

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
| `METERING` | §15.7: a meter is running. The payer confirmed a rate and a cap; the total is unknown until `stop` |
| `FUNDED` | Payment broadcast, or escrow funded |
| `PROVISIONAL` | `fast/1` only: the payee has scanned and found the transaction; service may proceed, awaiting finality |
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
| `QUOTED` | `MeterStart` | Payer confirmed rate and cap; `terms` carries both | `METERING` |
| `METERING` | `MeterStop` | `stop` tap with a matching `session_ref` | `ACCEPTED` |
| `METERING` | `MeterExpired` | Ran past `terms.meter_max_s` without a stop | `CLOSED`, single-sided receipt |
| `METERING` | `ABORT` | **Payee only** — see below | `ABORTED` |
| `ACCEPTED` | timeout 60 s | — | `ABORTED` |
| `FUNDED` | `TXID` | `fast/1`; **the payee's own scan** finds the transaction in the mempool at the accepted amount | `PROVISIONAL` |
| `FUNDED` / `PROVISIONAL` | `PROOF` | Profile-defined | `DELIVERED` |
| `DELIVERED` | `RECEIPT` | Both signatures present | `CLOSED` |
| `DELIVERED` | timeout 120 s | — | `CLOSED`, single-sided receipt (§6.2) |
| `CLOSED` | N confirmations | `fast/1` | `SETTLED` |
| `CLOSED` | cure window expiry, unconfirmed | `fast/1` | `CLAIMED` (§17.5) |
| `FUNDED` … `DELIVERED` | `DISPUTE` | Escrow modes only | `DISPUTED` |
| `CLOSED` | `CONTACT_OFFER` / `CONTACT_ACCEPT` | Within the 120 s contact window (§4) | `CLOSED` — contact is a side effect, not a state change |
| `CLOSED` | timeout 120 s | — | `CLOSED` — the contact window closes; session keys are destroyed (§4) |

The last row looks like a no-op and is not one. It changes no state, which is why it was missing from this table until 0.45 while §6.2 carried it all along — but it ends the window in which `CONTACT_OFFER` is legal and it destroys the session keys, so a client that does not hold this deadline keeps session keys alive indefinitely and accepts contact offers forever. **A deadline whose effect is not a state change still belongs in a table that calls itself exhaustive.**

### 18.4.1 Rules the table does not show

Implementing §18.4 surfaced six decisions the transition table leaves open. Each is now normative, because an implementer who guesses differently produces a client that interoperates until it suddenly doesn't.

1. **Direction constrains the originator, never the evaluator.** The table is role-agnostic, but not every message is legal from every side: **only the payer may *emit* `ACCEPT`**, since a payee able to accept its own offer could drive the entire flow with no human checkpoint, defeating §15.5. Likewise only the payee may emit `REFUND` (§7.3).

   **The guard is on who sent it, not on who is asking.** Both parties run the same machine over the same message and must reach the same verdict, so the originator travels *with* the event — established by signature before the machine sees it. A client that instead guards on its own role refuses every `ACCEPT` it receives, and no transaction can ever complete. This is not hypothetical: the first implementation of this rule made exactly that mistake, and it survived a 75-test suite because every test drove the machine from one side only. A five-party market simulation caught it on its first run.
2. **`CANCEL` has a closing bound as well as an opening one.** It is legal only between `ACCEPTED` and `FUND` — before the price is locked `ABORT` is the free exit and there are no cancellation terms yet, and once funds have moved cancellation is not a thing that exists. Post-`FUND` recourse is dispute (escrow) or slash (`fast/1`).
3. **The post-`ACCEPT` deadline is mode-dependent.** §6.2 lists "FUND after ACCEPT: 60 s" and "multisig setup: 300 s" as though they were different states. They are the same state under different settlement modes: 60 s for `direct` and `fast`, **300 s for `escrow`**, whose window is spent on multi-round multisig setup (§8.2). Its expiry MUST run the fund-recovery path, not a bare abort.
4. **The `FUNDED` deadline applies only under `fast/1`.** That 30 s bounds the wait for `TXID`. Under `direct` and `escrow`, `FUNDED` awaits profile-defined delivery and carries no wall-clock deadline.
5. **Terminal states are absorbing.** `ABORTED`, `CANCELLED`, `DISPUTED`, `SETTLED`, and `CLAIMED` accept no further events, including timeouts. `CLOSED` is deliberately *not* terminal: it still admits the contact coda and `fast/1` finality.
6. **Elapsed time in an unbounded state is a no-op, not an error.** Clients poll on their own schedule, and a client that polls more often than another must not thereby reach a different state.
7. **`ABORT` is directional once a meter is running.** §6 lists `ABORT` as available to either party with no penalty, which is right before value accrues and wrong afterwards: a payer able to abort a live meter would start a tab, consume, abort, and owe nothing. From `METERING` only the **operator** may void cleanly — comping a drink is ordinary commerce — while a payer leaving is **abandonment**, which routes through `MeterExpired` and leaves a single-sided receipt as evidence rather than a clean exit with no record. `CANCEL` likewise does not apply to a running meter: §7.3's fixed cancellation schedule is the wrong instrument when the correct one already exists, which is stopping the meter and paying what accrued.
8. **A metered session needs its own state, and this was found the hard way.** §15.7's two-tap flow and §6.2's deadlines were written independently and disagreed: a `start` leg landed in `ACCEPTED`, whose 60-second deadline aborted a bar tab after one minute. `METERING` is therefore **not wall-clock bounded** — its limit lives in `terms.meter_max_s`, which the machine does not hold, so expiry arrives as an explicit `MeterExpired` event from the caller. That is the same pattern as `ConfirmationsReached` and `CureWindowExpired`: the caller establishes the condition, the machine decides the consequence.

---

## 18.4.2 Field-Number Registry

Part V numbers four objects (`TapPresent`, `FullOffer`, `ACCEPT`, `RECEIPT`) and the document references a dozen more. Unassigned numbers are how two implementations collide silently, and it is the same gap that made transcripts unbuildable until 0.14 — so the space is allocated here even where the objects are not yet specified.

**Allocation rule.** Keys 0–2 are reserved across every object: `0` type, `1` version, `2` suite. Object-specific fields start at 3. Keys 0–23 encode in one byte and are spent first; an object needing more than 21 fields should be reconsidered before it is numbered.

| Range | Object | Status |
|---|---|---|
| 0–2 | Common header, every object | **Assigned** |
| 3–14 | `TapPresent` (§15.3) | **Assigned** |
| 15–21 | `FullOffer` (§15.4) | **Assigned** |
| 22–27 | `ACCEPT` (§15.5) | **Assigned** |
| 28–30 | `RECEIPT` (§6) | **Assigned** |
| 31–33 | `TapStatic` (§15.9) | **Assigned** |
| 34–37 | `REFUND` (§7.3) | **Assigned** |
| 38–39 | `CANCEL` (§7.3) | **Assigned** |
| 97–101 | `MANDATE` (§7.3) | **Assigned** |
| 40–45 | `CONTACT_OFFER`, `CONTACT_ACCEPT` (§16.3) | Reserved |
| 46–49 | `TXID` (§17.4) | **Assigned** |
| 50–51 | Unallocated | — |
| 52–59 | `DISPUTE`, `RULING` (§9.3.2) | **Assigned** (`EVIDENCE` still reserved) |
| 60–67 | `HAIL`, sealed reply (§5.2.1) | **Assigned** |
| 68–79 | `MarketDescriptor` (§10.1) | Reserved |
| 80–95 | Delegations (§4.2), attestations (§9.2) | Reserved |
| 96 | `TERMS` (§7.3) | **Assigned** |
| 97–101 | `MANDATE` (§7.3) | **Assigned** |
| 102–103 | `REFUND_TO`, `REFUND_PAID_TO` (§7.3) | **Assigned** |
| 104–108 | `ESCROW_SETUP` (§8.2) | **Assigned** |
| 111–117 | `ESCROW_READY` (§8.2) | **Assigned** |
| 119–123 | `RELEASE` (§8.2) | **Assigned** |
| 125–130 | `TXPROOF` (§17.5) | **Assigned** |
| 132–138 | `SLASH_CLAIM` (§17.5) | **Assigned** |
| 140–144 | `bond_proof` (§17.4) | **Assigned** |
| 145–146 | `memo` on `FullOffer` and `ACCEPT` (§7.5) | **Assigned** |
| 147–156 | *Burned.* The route-blob card and claim (§16.9 before 0.65). | **Never reuse** |
| 157–160 | `MESSAGE` (§16.10) | **Assigned** |
| 161–163 | `PREKEY_BUNDLE` (§16.11) | **Assigned** |
| 164–166 | `SEALED_MESSAGE` (§16.11) | **Assigned** |
| 167–171 | `CONTACT_OFFER` — the record-based card (§16.12) | **Assigned** |
| 172–175 | `CONTACT_ACCEPT` — inbox details (§16.12) | **Assigned** |
| 176–177 | `LOG_HEAD` (§16.12) | **Assigned** |
| 178–181 | money in a conversation (§16.13) | **Assigned** |
| 182 | optional payout address on `CONTACT_ACCEPT` (§16.12) | **Assigned** |
| 183–184 | itemisation on a payment message (§16.13) | **Assigned** |
| 185–186 | fields of a `LINE_ITEM` (§16.13) | **Assigned** |
| 187–191 | profile on `CONTACT_ACCEPT` (§16.9) | **Assigned** |
| 192–193 | reaction target (§16.14) | **Assigned** |
| 194–200 | attachment reference (§16.15) | **Assigned** |
| 201–202 | read watermark and ring size on `LOG_HEAD` (§16.16, §16.12) | **Assigned** |
| 203–209 | `HAIL_NOTICE` (§16.17) | **Assigned** |
| 210–212 | car model, colour and plate on `CONTACT_ACCEPT` (§16.9, §15.12) | **Assigned** |
| 213 | `eta_secs` on a ride offer (§15.12) | **Assigned** |
| 214–216 | ceremony `payload`, `round`, `ceremony_id` (§17.9) | **Assigned** |
| 217 | `purpose` on `CONTACT_ACCEPT` (§16.9) | **Assigned** |
| 218–219 | `POSITION_REF` record key and stream key (§15.12) | **Assigned** |
| 220–241 | `RENTAL_NOTICE` (§16.18) | **Assigned** |
| 242–244 | poster key, signature and proof of work on `RENTAL_NOTICE` (§16.18) | **Assigned** |
| 245–247 | the same three on `HAIL_NOTICE` (§16.17) | **Assigned** |
| 249–250 | the freshness beacon's height and block hash on `RENTAL_NOTICE` (§16.18.1) | **Assigned** |
| 251–252 | the same pair on `HAIL_NOTICE` (§16.18.1) | **Assigned** |
| 248 | `quantity` on `RENTAL_NOTICE` (§16.18) | **Assigned** |
| 253–254 | group id and the sender's group counter (§16.19) | **Assigned** |
| 255–256 | group reference: target's sender and their counter (§16.19) | **Assigned** |
| 257–258 | publication shelf: root record and standing head key (§16.20) | **Assigned** |
| 259–260 | publication period: id and content key (§16.20) | **Assigned** |
| 261–262 | publication shipment: swarm share key and index digest (§16.20) | **Assigned** |
| 263–264 | live call: private-route blob and call id (§16.21) | **Assigned** |
| 265–270 | `PUB_NOTICE` body: version, card, title, blurb, price, expiry (§16.18.2) | **Assigned** |
| 271–275 | `PUB_NOTICE` stamp: poster, sig, pow, beacon height and hash (§16.18.1–.2) | **Assigned** |
| 276+ | Unallocated | — |

The `96+ Unallocated` row above was stale from 0.14 onward: 96–103 had been in use since `TERMS` and `MANDATE` shipped, and a second implementer allocating from 96 would have collided head-on. Registries decay silently unless something checks them, which is the argument for the type-code rule below.

**Every object's `type` field MUST be a registered code, and decoders MUST check it.** Five objects — `DISPUTE`, `RULING`, `HAIL`, `HAIL_REPLY`, `TapStatic`, every one added after the original four — carried improvised codes (`CANCEL + 100`, `+ 200`, …) and *discarded* the type field on decode rather than checking it. Two byte strings differing only in their declared type therefore decoded to the same object, which is §18.3's transcript-divergence bug in its purest form: both verify, both hash differently. Fixed at 0.47 with real codes 13–22. The pattern is worth naming because it was not one mistake but five copies of one — the later objects were written from each other rather than from the earlier, correct ones.

**Reserved is not assigned.** A client MUST reject any field it does not recognise (§18.8), so an object using a reserved-but-unspecified number is malformed today and will not silently start working when that number is defined. Reservation prevents collision; it does not grant meaning.

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

**NFC — assigned, and it was never waiting on anything.** The AID is:

    F0 44 55 43 41 54          (0xF0 ‖ "DUCAT", 6 bytes)

Earlier drafts carried `F0 44 43 41 54` (`0xF0` ‖ `"DCAT"`) and called it "a placeholder pending real RID registration". **There is no registration to wait for.** ISO/IEC 7816-5 assigns the first nibble of an AID by category — `'A'` for internationally registered, `'D'` for nationally registered — and reserves the range where bits 8–5 of the first byte are all `1` (i.e. `0xF…`) for **proprietary identifiers that require no registration at all**. `F0` is not a stand-in for a registered RID; it is the standard's answer for applications that do not have one, and it is what Android HCE documents for exactly this purpose.

Two consequences of that, both worth stating rather than discovering later:

- **No registry means no uniqueness guarantee.** Nothing prevents another vendor choosing the same bytes. The mitigation is length: AIDs may run to 16 bytes, so the four-character `"DCAT"` contraction bought nothing — it was chosen to fit a 5-byte minimum that was never a maximum. The full name is more distinctive at the cost of one byte in a `SELECT` APDU, which is not a cost.
- **The value is effectively immutable.** It cannot be discovered at runtime: iOS readers must declare selectable AIDs at build time in `com.apple.developer.nfc.readersession.iso7816.select-identifiers`, so changing it is a simultaneous app update on every iOS client in existence. That is the real reason to fix it now, and it is unrelated to registration.

**The APDU exchange behind the AID.** Two plain ISO 7816 verbs — plain on purpose, because an iOS reader (O19: iPhones can read HCE, never emulate) must be able to speak this with the system APIs as they stand:

| Step | APDU | Response |
|---|---|---|
| Select | `00 A4 04 00 06 F0 44 55 43 41 54 00` | payload length as two big-endian bytes, then `90 00`; `69 85` if the phone is present but offering nothing |
| Read | `00 B0 <off_hi> <off_lo> 00`, repeated | up to 250 bytes of payload at that offset, then `90 00`; `6B 00` at or past the end |

The payload is the UTF-8 bytes of a `ducat:` card URI — the same value the QR carries — and deliberately nothing more. Tap is **presence-only**: it proves two phones touched, and everything that follows rides the mailbox the card opens (§16.12). The offering phone serves whatever its visible screen presents (a sale's card, a tab's handshake, the standing profile code as fallback), snapshotted at `SELECT` so the reader walks offsets into one consistent value. Readers MUST also accept plain NDEF tags carrying `ducat:` or `monero:` URIs — §15.9's static stickers on the same antenna. Serving a card to two readers is harmless by construction: cards are claim-once, and it is the DHT that refuses the second claim.

**BLE — assigned.** One 128-bit service UUID and three characteristics, sharing a base with the 16-bit slot varying, which is the convention custom profiles follow so a sniffer groups them at a glance:

| Role | UUID |
|---|---|
| Service | `30910001-5923-472e-860f-56eaed5db906` |
| Bootstrap-write | `30910002-5923-472e-860f-56eaed5db906` |
| Session-notify | `30910003-5923-472e-860f-56eaed5db906` |
| PSM discovery | `30910004-5923-472e-860f-56eaed5db906` |

These need no registration either, and for a cleaner reason than the AID: the Bluetooth SIG registers only 16-bit UUIDs, and the 128-bit space exists precisely so that anyone can allocate without asking. Randomly generated, so collision is not a practical concern.

**The L2CAP PSM is read, never fixed.** LE Connection-Oriented Channels take dynamic PSMs in `0x0080–0x00FF`, assigned by the local stack at listen time — a spec that pinned one would be pinning a value it does not control. A presenter publishes its PSM in the discovery characteristic above; a reader reads it and connects. This is the one identifier here that MUST NOT be a constant.

### Stewardship of the transport

DUCAT rides a network built by people who explicitly refused to monetize one. That is a **dependency, not a coincidence** — the transport's neutrality, its lack of payment incentives, and its volunteer character are part of what makes §2's privacy claims hold — and a protocol that quietly eroded those properties while using them would be a parasite with a spec. So the obligations are normative:

- **DUCAT MUST NOT introduce protocol fees, node payment, or any mechanism that monetizes carriage.** Money settles on Monero; Veilid carries sealed bytes and is never itself a market. A future in which running a node pays is a future in which running a node is a business, with a business's incentives toward its traffic.
- **A client MUST be a full participant.** Every DUCAT device runs a node that routes and stores for strangers, giving back the class of service it takes. A leech mode is not a client option.
- **Records are for live purposes.** An implementation MUST NOT rewrite a record merely to extend its lifetime past its use, and SHOULD delete its local record state once the purpose is spent — an answered handshake inbox, a fetched attachment, an expired sale card. Honestly stated: deletion is local; the network reclaims its own copies by TTL, and no client can recall bytes from a distributed store. The obligation is to stop being a long-lived origin for dead purposes, and to keep one's own footprint proportionate — an attachment is one record, a card is minted per purpose and swept when spent.

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

### 18.9.1 The Vector Schema

`vectors/v1/schema.json` is normative for the vector files, and it exists because §18.11's second implementation spent most of its effort on the *harness* rather than on the protocol.

**Every case carries a `kind`, and `kind` is the only discriminator.** File names group cases for human convenience and carry no meaning to a consumer: a `commit.substitution` case runs identically wherever it lives, which is worth stating because it used to live in `negotiate.json`.

| `kind` | Asserts |
|---|---|
| `codec.decode` | Decode, and on success re-encode to the published bytes |
| `signing.verify` | A signature over `sig_input(verify_as, suite, object)` |
| `signing.pubkey` | Public-key parsing alone — no object, no signature |
| `negotiate.select` | Version and suite chosen from an offer under a local policy |
| `commit.purposes` | One byte string, four purposes, four distinct digests |
| `commit.substitution` | A stripped offer fails the published commitment |
| `state.sequence` | A run through the machine, checking `next` and `effect` at each step |
| `transcript.replay` | A complete four-object transaction |
| `transcript.substitution` | An offer delivered after the tap fails the tap's commitment |
| `backup.import` | A backup bundle decrypts to the published fields |
| `object.roundtrip` | One wire object decodes and re-encodes to the same bytes |
| `escrow.ceremony` | Setup messages fed in order — out-of-order and duplicate refused (§2.5) |
| `escrow.ready` | Participants agree on the wallet formed, and on an arbiter the market named |
| `escrow.release` | A payout is bounded by the balance and confined to a party |
| `bond.check` | A bond attestation is fresh, ladder-valued, and covers the fare (§17.4) |
| `slash.check` | A claim respects the cure window, or carries the evidence that skips it (§17.5) |
| `contact.card` | A record-based contact card decodes and re-encodes to the same bytes, including its text-field rules (§16.9, §16.12) |
| `contact.details` | What each side writes into the contact inbox: persona, outbox key, prekey bundle (§16.12) |
| `log.head` | An outbox's head subkey, carrying the next sequence number (§16.12) |
| `log.ring` | A sequence number maps to the right subkey, and a reader can tell when the ring has passed it (§16.12) |
| `hail.notice` | A notice off a public board decodes, or is refused for the hostile-surface rules (§16.17) |
| `rental.listing` | A listing off a public board decodes, or is refused for §16.18's rules — the coarser cell, the place/vehicle split, the refused enumerations, and `quantity`'s one-spelling rule |
| `pub.listing` | A publication listing decodes, or is refused for §16.18.2's rules — the one spelling of free, the version, the card's scheme, the text bounds, and the strict reader's edge (276) |
| `board.sealed` | A notice as it actually sits on a board (§16.18.1): the poster key is recovered, the signature verifies against *that slot*, and the proof of work clears — or the case is refused, as the same bytes offered at a different slot must be |
| `board.beacon_window` | §16.18.1's freshness range, as a *caller* applies it: a `tip_height` of zero is a device with no chain view and skips the test, and anything else is the range. Both edges and both sides, because decoding must not consult a chain and so the rule cannot live in the reader |
| `board.beacon_verdict` | The rest of §16.18.1's freshness rule: what a reader may *do* once it has looked at the beacon — show, refuse, or hold. Pinned because the third answer is the one a reader can lose in the attacker's favour without noticing, and because the height half of the test is free and secures nothing on its own |
| `position.frame` | §15.12's live-position stream, one update: the fixed-length XChaCha20-Poly1305 value written to the record's subkey, opened and its fields checked. Pins the constant length (so the sequence leaks only its cadence), the record-key-as-AAD binding (so a value cannot be lifted between rides), and the range and padding refusals |
| `stand.shard` | A shard of a stand's overflow ladder gets the pinned board name, and the cap is a hard edge (§15.12) |
| `stand.epoch` | A stand's generation gets the pinned board name, and a name that already names one is refused (§15.12) |
| `message.chain` | A 1:1 message thread links and sequences without gaps or substitutions (§16.10) |
| `message.payment` | A payment request or notice carries an amount, and text does not (§16.13) |

Three rules about the cases themselves, each earned:

- **`why` is required.** A case with no stated reason is one nobody can safely change: an implementer who knows what an input defends against finds the bug faster than one who sees a failing hex string, and a maintainer who does not know why a case exists will eventually "fix" it.
- **`hint` is non-normative and MUST NOT be parsed.** It names the internal rule that fired. Two clients may reject the same input for differently-named reasons and still interoperate — §18.5 requires agreement on the code, not on the explanation.
- **`expect.ok: false` always carries `reject_code` and `reject_name`.** "It failed somehow" is not an interoperable assertion.

**The schema is hand-written, not emitted by the generator.** A schema produced by the same program that produces the vectors would agree with that program's mistakes — O21's objection in a smaller frame. It earned that on its first run, catching a negotiation case that never said which versions the local client supported (forcing a consumer to invent a default, which is how two implementations diverge) and an offer-substitution attack filed as a transcript replay.

Validation is two commands, and both are cheap enough to run on every change:

    python3 conformance/validate_vectors.py     # cases against the schema
    python3 conformance/ducat_check.py          # a second implementation runs them

`cargo test` additionally enforces the half that must never be skippable: every case declares a known `kind`, carries a non-empty `why`, and has a name unique across the whole set — because a third-party client hits a broken discriminator before we do, and their first experience of DUCAT should not be a file they cannot dispatch on.

## 18.10 Conformance Levels

So that "DUCAT client" means something specific:

| Level | Requires |
|---|---|
| **Core** | §18.1–18.8 in full, **both the Ed25519/X25519 and P-256 suites** (§4.1 — otherwise personas fragment by platform), QR transport, `xfer/1`, `direct` settlement. The floor for using the name. |
| **Proximity** | Core + NFC and/or BLE transport + `pos/1` |
| **Fast** | Proximity + `fast/1`, bonded float, `TXID` scanning, slashing state machine |
| **Full** | Fast + escrow modes, arbitration, escrow-gated profiles |

A client MUST declare its level and the vector-set release it passes. A client that cannot pass Core vectors is not a DUCAT client regardless of what it implements — the point of levels is to make partial implementations legible rather than to make the name negotiable.

## 18.11 The Second Implementation, and What It Found

§18.9 built the vector set and §18.10 defined what passing it means. Neither addresses O21's actual objection: **a vector set generated and validated by one implementation encodes that implementation's bugs as the specification.** A suite cannot audit the client that produced it.

`conformance/ducat_check.py` is a second implementation, written from this Part rather than from `core/`, that runs the published vectors.

**Its independence is limited and the limit must be stated.** It has the same author as the reference. That is not clean-room, and no discipline makes it equivalent to a stranger reading this document cold. **O21 is therefore advanced, not closed.** What such an implementation can still catch is nonetheless real, and is precisely the category that costs interoperability: places where this document says something the reference does not do, places where the reference does something this document never says, and places where the prose admits two readings while the vectors quietly pick one.

### Result

    104 vector cases — 101 agreed on the first run, 3 disagreements

**All three were defects in this document, not in the vectors.** In two cases the reference was right and the text had not said so; in the third, nothing was wrong anywhere and that was the problem.

**1. §18.1 had no nesting bound.** A decoder written from §18.1 accepts arbitrarily deep structures — the word "nesting" did not appear in this document, while the vector's own hint referred to "the 16-level nesting bound" as though it were specified. Beyond the stack-exhaustion route, a limit left to the implementer is *worse* than none: two clients choosing 16 and 32 disagree about the same signed bytes.

**2. §18.4's exhaustive table was not exhaustive.** `CLOSED`'s 120-second contact window appeared only as a *guard* on `CONTACT_OFFER`, never as a deadline row, though §6.2 had carried it all along. It changes no state, which is exactly why it was omitted and exactly why omitting it matters: a client without it keeps session keys alive indefinitely and accepts contact offers forever.

**3. Negative integers were unspecified, and both implementations were conformant.** The reference accepted CBOR major type 1; the second implementation refused it. Nothing in §18.1 decided the question, so neither was wrong — two conformant clients simply disagreed about whether a byte string was a valid signed object, and **no conformance suite could have detected it, because there was no correct answer to test against.**

This is the finding that justifies the exercise. The first two were omissions a careful reader might have noticed. The third was invisible from inside the reference, where the code *is* the answer to the question, and invisible to the vector set, which only ever tests decisions someone already made. It was found by two implementations reaching different conclusions from the same text — the only mechanism that finds this class of defect at all.

### After the corrections

    104 vector cases — 104 agreed, 0 disagreements

### What the harness cost, and what was done about it (0.46)

Most of the second implementation's effort went into the *harness*, not the protocol. Four obstacles, none of them protocol bugs — the vector files were simply neither uniform nor described:

- `signing.json` had two shapes with nothing announcing which; the first encounter was a crash.
- `negotiate.json` contained two cases that were not negotiations.
- `transcript.json` contained one case that was not a transcript.
- The state event grammar had **five spellings of one concept**.

Writing a schema that *documented* five spellings of one event would have formalised the mess. The format was normalised first and then specified (§18.9.1): every case carries a `kind`, and `kind` is the only discriminator.

**The schema then found two more defects on its first run**, which is the argument for hand-writing it rather than generating it from the same program that writes the vectors. A negotiation case never said which versions the local client supported — a negotiation is a function of three inputs, and a case omitting one forces the consumer to invent a default, which is exactly how two implementations diverge. And `state.sequence` assertions were split between the case and its steps, so a case could assert a state transition without asserting the effect — which passes while a client emits the wrong evidence, and §6.2 has two unilateral receipts that assert opposite things.

One item is left as prose because it is non-normative: **the transcript cases name commitment purposes `Offer` and `ChainLink`**, the reference's internal enum spellings rather than §18.3's wire labels. The labels are correct — confirmed against `commitments_are_domain_separated_by_purpose`, which publishes all four digests — but the prose sends a reader looking for the wrong strings, and it cost time.

### What still stands between this and O21 closing

Only the thing that cannot be engineered away: **an implementer who has never read `core/`.** The schema, the `kind` discriminator, and the two validation commands remove the accidental difficulty; the remaining gap is authorship, and no amount of tooling substitutes for a second pair of eyes on the same document.

---

## 18.12 Auditing the Document Against the Implementation

§18.9 checks artifacts against each other and §18.11 checks two implementations against each other. Neither checks that **this document still describes what was built**, and prose drifts from code silently — a stale sentence throws no exception.

`conformance/audit_spec.py` is that check. It is a script rather than a review because a review is true on the day it happens.

| Check | What it catches |
|---|---|
| Every `§N.M` reference resolves | Sections renumbered, or never written |
| Every `O`*n* reference resolves | Open problems renumbered or removed |
| §18.5's reject codes match `core::reject` | A code changed on one side |
| Field numbers do not collide **within a namespace** | Two objects claiming one key |
| Object type codes unique; every type has a label | The §18.3 gap fixed at 0.47, mechanised |
| Vector `kind`s agree across schema, generator, and both runners | A case nobody executes |
| Header draft matches newest changelog entry | A version bump applied in one place |
| Transport identifiers in §18.7 match `core::transport` | An AID or UUID edited on one side |

### What later runs found

The checks grew as the failures did. §18.12's second pass added three that the first lacked, and each caught something:

- **Numeric claims about the vector set.** O21's live text still quoted the count from four drafts earlier, after the set had grown by a third. A document that miscounts its own artifacts is one a reader stops trusting, and prose counts go stale the moment a case is added. Changelog entries are exempt: they are history, and are supposed to say what was true then.
- **Field numbers outside every declared range.** `bond_proof` was allocated 140–144 in code while §18.4.2 still read "140+ Unallocated" — the exact collision hazard that registry exists to prevent, reintroduced by the person maintaining it.
- **Vector kinds the document never names.** §18.9.1's table listed ten kinds while the schema accepted sixteen. Six kinds — every escrow and bond case — were executable, published, and undiscoverable from the specification.

The last one is the pattern worth naming: **an artifact and a description of it drift in the direction of the artifact**, because the artifact is what gets run. Only a check that compares them notices, and it has to be a check rather than a habit.

### What the first run found

**A normative section that was referenced three times and did not exist.** §15.5.1 was announced in 0.41's changelog, implemented in `core/src/verify.rs`, tested, and cited by §4.3 and by O10 — but never written. An implementer following the reference found nothing. It is written now.

That is the failure mode this check is for: nothing was inconsistent, no test failed, and the missing text was invisible precisely because everything *around* it was correct.

### What it got wrong, and why that is recorded

The first version reported eight problems. **Three were real only in the sense that the checker was wrong**, and each is worth naming, because a checker that cries wolf gets ignored and then the one real finding is lost with it:

- It flagged `§4.2.1` — a reference to **RFC 8949's** section, not a DUCAT one. External citations are now skipped by line.
- It reported a reject code called `TERMS`, having matched the field registry's `| number | NAME |` rows with a pattern meant for §18.5's table. The search is now scoped to that section.
- It reported five field-number collisions between `TYPE`/`VERSION`/`SUITE` and Terms' keys. **Not collisions:** `Terms` is a *nested* map with its own key space, which legitimately restarts at 0. Field numbers are now compared within a namespace rather than across the file.

The ratio matters more than the count. One real defect against three checker bugs is the normal shape of a first audit, and the discipline is to fix the checker rather than to loosen it until it agrees.

---

---
*End of Part V. One line: behavior is specified in Parts I–IV, but interoperability lives in canonical bytes, domain-separated signatures, an exhaustive state table, strict rejection, and a vector set that a second implementation can fail.*
