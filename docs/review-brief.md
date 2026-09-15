# Adversarial review: what to attack, and what it is worth

DUCAT's own specification says, in §2.5, that it has had **no review by
anyone outside the project**. That is still true and it is the last honest
blocker to calling anything 1.0. What has happened since is an *internal*
adversarial pass over five surfaces
([`research/security/2026-09-07-adversarial-review.md`](../research/security/2026-09-07-adversarial-review.md)),
which found forty-odd things and closed most of them. A review by the people
who wrote it tells you what they thought to look at; that ledger is in the
repository so your time goes somewhere else. This document is the rest of
the orientation.

It is written to be handed over. If you are that reviewer: everything below
is a claim we are asking you to break, not a description we are asking you to
admire.

## What the thing is, in one paragraph

A peer-to-peer proximity-commerce protocol. Transport is [Veilid](https://veilid.com)
DHT records and private routes; settlement is [Monero](https://www.getmonero.org),
scanned and spent by an embedded wallet with no `wallet-rpc` daemon. There is
no DUCAT server anywhere — no directory, no matchmaker, no escrow company. A
persona is a keypair; a contact card is a capability to write one DHT record;
a conversation is two mailboxes and a hash chain. On top of that sit bills,
receipts, a point of sale, a bar tab, ride-hailing with no dispatcher, and
threshold escrow (FROST) for bonded rides and reservations.

## The artifacts

| What | Where | Why you would read it |
|---|---|---|
| The specification | [`ducat-protocol.md`](../ducat-protocol.md) | The normative document. **Draft 1.1.0-dev13** on this branch; 1.0.0-rc1 is the frozen line, and the trust and verification surfaces below are newer than it. Changelog first. |
| Reference implementation | [`core/`](../core) | Rust. The vectors are generated from it. |
| Conformance vectors | [`vectors/v1/`](../vectors/v1) | 442 cases + schema — the published artifact. |
| Second implementation | [`conformance/ducat_check.py`](../conformance/ducat_check.py) | An independent reading of the spec, in Python. It agrees on all 442. |
| Internal review ledger | [`research/security/`](../research/security) | What we already attacked, what we fixed, and what is still open. Start here to avoid repeating it. |
| Spec audit | [`conformance/audit_spec.py`](../conformance/audit_spec.py) | Catches prose that stopped describing the code. |
| Clients | [`applications/`](../applications) | `android/` the phone, `desk/` the Tauri desktop client over the Rust in `app/`, `desktop/` the earlier Compose desk that compiles the phone's own sources against a shim. Two implementations of the protocol rather than one — which is itself a place to look for divergence. |
| Wire bridge | [`mobile/`](../mobile) | UniFFI wrapper. Adds no logic, by rule. |

Everything runs on every commit (`.github/workflows/checks.yml`). To run it
yourself: `python3 -m pip install -r conformance/requirements.txt`, then
`cargo test --workspace` and the four checkers.

## Scope, in the order we think it matters

**1. The sealed thread (§16).** X3DH-shaped handshake with one-time prekeys,
per-contact prekey partitioning, a hash chain over an outbox ring, and a
"patience window" for out-of-order arrival. Attack: key reuse across
contacts, a forged or replayed chain link, a message that arrives readable to
someone it was not sealed for, forward-secrecy claims that the signed-prekey
fallback quietly breaks. Note that the fallback is *shown* to the user (an
open lock) — tell us if showing it is doing less work than we think.

Two pieces of this are days old and worth your attention first. A card's
**claimant half is now HPKE-sealed**, not merely signed, because a card on a
public board hands its inbox record key to every reader of that board; attack
what a board reader can still learn from subkey 1, and what happens to an
issuer handed a reply it cannot open. And `INTRODUCTION` (**message kind
17**) carries a signed `CONTACT_ACCEPT` *inside* a thread, so a card can
publish less than it hands over. Its two ties are the whole of its security:
it must open under the persona it names, and it must name the inbox the
thread was born from. Attack both — an introduction replayed from another
relationship, one naming a third party, one into a thread with no card
binding to check it against — and attack the rule that a later, different
name is shown rather than adopted.

**2. The escrow ceremonies (§17.9).** PedPoP distributed key generation, then
FROST signing, both carried as opaque payloads over the sealed thread. Two or
three parties; threshold two. The round-0 frame is rebuilt independently by
every participant and the ceremony id is a hash of the roster. Attack: a
participant who lies about the roster, a rejoin that re-derives a different
key, a proposal whose stated split differs from the transaction it signs — the
co-signer is now shown the destinations, the inputs total and the fee, read
back out of the transaction's own re-encoding, so **attack the reader**: a
second decoder for a structure the wallet already parses is exactly where a
disagreement between the two would hide — a captured arbiter, and a party who
can strand funds rather than merely refuse.

**3. The public boards (§15.12, §16.18.1).** A geocell is a DHT record whose
address is derived from the place itself — anyone can read or write one. Hails
and listings are claim-once cards; the DHT referees the race. A notice is
signed by a per-listing key, carries an Argon2id proof of work, and — since
0.89 — is stamped against a recent Monero block so the work perishes rather
than being mineable a year ahead. Attack: claim-stealing, board flooding
under the memory-hard cost, a driver who watches a cell they are nowhere
near, correlation of a rider across hails, the ~1.2 km coarseness claim, and
the beacon specifically — the freshness window (720 blocks back, 2 forward),
the three-answer verdict (show / hold / refuse, where "cannot say" must never
show), and the degraded read-only path a node outage forces a reader into.
This is the newest surface and the least reviewed; the changelog entry for
0.89 states the whole argument.

**4. Money handling.** Subaddress-per-contact attribution, output-to-person
matching by key image, the ten-block maturity rule, fee estimation, and the
rule that a payment request can never be one-tap paid. Attack: attribution
confusion (an output credited to the wrong person), a bill whose lines do not
sum to its total, a receipt that acknowledges a transaction that did not
happen, change shown as income.

**5. Costly identity (§9.5, §9.2).** The answer to "a persona is free to
mint" is a **proof of burn**: XMR sent to an unspendable address, proved to a
reader with a Monero out-proof and carried as a signed `BURN_PROOF` under the
persona. On top of it sit rated receipts (`ATTESTATION`) and vouches
(`VOUCH`), all weighed by the *reader* alone — one voice per signer, weighted
only by signers whose burn that reader verified itself, and a vouch counted
only when its signer is already one of the reader's own contacts. Nothing is
published and nothing is aggregated. Attack: a burn proof that does not
prove what it claims, a proof replayed under a second persona, an attestation
lifted out of one thread into another, a reader that can be made to count a
stranger's vouch or its own, and the economics — what does it actually cost
to farm a plausible history, and is the seller's minimum (`min_burn`, field
320) a wall or a speed bump?

**6. Payer verification (§15.5.1).** Whether the person holding the phone is
entitled to spend at all — the question WYSIWYS never asks. Every payment
wants a device unlocked inside a two-minute window; above a user-set
threshold the app's own PIN, and `app_secret_every_time` means *every* time
rather than once per window. The policy rides a backup as keys 9–13 and
28–29, the last two optional and defaulting to the stricter reading. None of
it touches the wire. Attack: any path to a spend that skips the gate, the
Android keystore binding behind the unlock window, the rolling-hour counter,
what a restore does to somebody's threshold, and the escalation rule for a
stale exchange rate (§17.7) — an attacker who can stall a rate feed must not
be able to *lower* the requirement.

**7. The stewardship claims (§18.7).** No protocol fees, no node payment,
every client a full participant. These are conformance requirements, not
license terms. Tell us if the protocol as written permits a client to defect
profitably.

## What we already know is wrong or unproven

Reviewing this list back to us is not useful; breaking something *not* on it is.

- ~~**Co-signer consent is partial.**~~ **Closed 2026-09-14.** A co-signer used
  to be shown the fee and not the destinations, because `monero-wallet` kept a
  `SignableTransaction`'s payments private. `read_tx` now walks the crate's own
  re-encoding and returns the destinations, the inputs total and the fee, and a
  client refuses a proposal it cannot describe. Worth attacking rather than
  taking on trust: it is a second decoder for a structure the wallet already
  parses, which is exactly where a disagreement between the two would hide.
- **NFC has never run on hardware.** Compile-verified, never field-tested.
- **Everything is stagenet.** No mainnet transaction has ever been made.
- **A desk's vault key is only as good as its passphrase**, and unlike a
  phone there is no hardware to rate-limit guesses against a stolen disk.
  Argon2id at 64 MiB / 3 passes is the only brake.
- **Builds are debug-signed.** §11's reproducible-build-and-independent-key
  requirement is not met.
- **No clean-room implementer.** O21 asks for someone who has never read
  `core/` to build from the document alone. Nobody has.
- **The latency figures in §8.7.2** are a desktop with an attached node, not a
  handset.
- **A board reader with no Monero node cannot check freshness** and falls
  back to signature-and-work alone — marked in the UI, but it means an
  attacker who can keep a specific reader's node unreachable (and only that
  reader) downgrades them to a class that accepts stale-but-signed spam. We
  treat this as an accepted trade, not a hole; tell us if the DoS-then-spam
  play is worth more than we think.
- **The board proof of work still does not stop slot *denial*.** A junk
  write with no valid stamp still occupies a DHT subkey; the stamp prices
  readable spam, not availability. Weekly board-generation rotation is the
  only answer, and it costs an attacker only 128 writes a week to defeat.
- **Sybil cost is now optional, not structural.** §9.5's proof of burn makes
  an identity cost real money and §9.2's vouches anchor it to people the
  reader already met — both built and walked on stagenet — but neither is
  *required* to post. A persona and a per-listing key are still free to mint
  from a hash, so a board defence is still a throughput speed bump; what
  changed is that a reader can now price the difference. Tell us whether
  "burn to be taken seriously" survives contact with somebody who wants to
  farm it.
- **The desk has no second factor.** §15.5.1's spend gate runs on the phone
  and cannot run on the desk, which has no device credential and no secret of
  its own — so anyone at an unlocked laptop can spend the desk's wallet. It
  is stated in the spec and tracked as D11 rather than quietly absent.
- **The trust layer has no adversarial pass of its own.** The five-surface
  review predates it.
- One stated privacy trade: address search, routing and map tiles query
  OpenStreetMap's servers — the single place location leaves the device.

## What a good report looks like

We would rather have three findings with a reproduction than thirty
observations. For each: what an attacker controls, what they gain, and the
smallest change that would fix it. If a finding is in the *spec* rather than
the code, say so plainly — the document is the primary artifact, and a
document-level flaw is worth more to us than a bug.

Vectors are the currency here. A finding that arrives with a failing vector
(`vectors/v1/`, schema in the same directory) goes straight into the suite and
stays fixed forever.

## What we can offer

Stagenet funds for testing, a standing desk arbiter to run ceremonies against,
and whatever access to the maintainer the work needs. There is no bounty
programme; this is an unfunded project, and we would rather say so than imply
otherwise.

## The one question we most want answered

Not "is the cryptography right" — the primitives are borrowed and the
ceremonies are conventional. It is this: **does removing the operator remove
the safety the operator was quietly providing?** Every escrow, every board,
every claim-once race is a place where a company would normally absorb a
dispute. We claim the protocol replaces that with structure the participants
can verify themselves. That is the claim worth attacking.
