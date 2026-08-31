# Adversarial review: what to attack, and what it is worth

DUCAT's own specification says, in §2.5, that it has had **no adversarial
review whatsoever**. That sentence has been true since draft 0.82 and it is
the last honest blocker to calling anything 1.0. This document is what a
reviewer needs so that their time goes into the protocol rather than into
orientation.

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
| The specification | [`ducat-protocol.md`](../ducat-protocol.md) | The normative document. 1.0.0-rc1, feature-frozen. Changelog first. |
| Reference implementation | [`core/`](../core) | Rust. The vectors are generated from it. |
| Conformance vectors | [`vectors/v1/`](../vectors/v1) | 355 cases + schema — the published artifact. |
| Second implementation | [`conformance/ducat_check.py`](../conformance/ducat_check.py) | An independent reading of the spec, in Python. It agrees on all 345. |
| Spec audit | [`conformance/audit_spec.py`](../conformance/audit_spec.py) | Catches prose that stopped describing the code. |
| Clients | [`applications/`](../applications) | Android + desktop, one shared implementation. |
| Wire bridge | [`mobile/`](../mobile) | UniFFI wrapper. Adds no logic, by rule. |

Everything runs on every commit (`.github/workflows/checks.yml`). To run it
yourself: `python3 -m pip install -r conformance/requirements.txt`, then
`cargo test --workspace` and the three checkers.

## Scope, in the order we think it matters

**1. The sealed thread (§16).** X3DH-shaped handshake with one-time prekeys,
per-contact prekey partitioning, a hash chain over an outbox ring, and a
"patience window" for out-of-order arrival. Attack: key reuse across
contacts, a forged or replayed chain link, a message that arrives readable to
someone it was not sealed for, forward-secrecy claims that the signed-prekey
fallback quietly breaks. Note that the fallback is *shown* to the user (an
open lock) — tell us if showing it is doing less work than we think.

**2. The escrow ceremonies (§17.9).** PedPoP distributed key generation, then
FROST signing, both carried as opaque payloads over the sealed thread. Two or
three parties; threshold two. The round-0 frame is rebuilt independently by
every participant and the ceremony id is a hash of the roster. Attack: a
participant who lies about the roster, a rejoin that re-derives a different
key, a proposal whose stated split differs from the transaction it signs (the
co-signer today sees a fee, **not** an itemised destination list — this is a
known, stated weakness, see "what we already know" below), a captured arbiter,
a party who can strand funds rather than merely refuse.

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

**5. The stewardship claims (§18.7).** No protocol fees, no node payment,
every client a full participant. These are conformance requirements, not
license terms. Tell us if the protocol as written permits a client to defect
profitably.

## What we already know is wrong or unproven

Reviewing this list back to us is not useful; breaking something *not* on it is.

- **Co-signer consent is partial.** A FROST co-signer is shown the fee, not the
  destinations, because `monero-wallet` 0.2.0 keeps a `SignableTransaction`'s
  payments private. A malicious proposer can therefore ask someone to sign a
  transaction they cannot fully inspect. Mitigated only by the payee usually
  being the proposer.
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
- **No sybil cost on identities.** A persona and a per-listing key are both
  free to mint from a hash, so every board defence is a throughput speed
  bump, never a wall. The §9.2 reputation weight that would anchor this to
  proximity is designed but unbuilt.
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
