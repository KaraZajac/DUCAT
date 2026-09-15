# ducat-core

Wire format and contract logic for the DUCAT protocol. No I/O, no platform
dependencies — testable in isolation and reusable by any client.

Part V (§18) is the floor, and was once all of it: the codec, domain-separated
signing, the state machine, the reject codes. Everything since arrived by the
same rule — **an object or a decision two clients could spell differently
belongs below both of them**, not in the phone and again in the desk. So §9's
trust objects, §15.5.1's verification policy, §16's boards, groups and
publications, and §4.3's backup bundle are here too, and the second table below
is the honest size of the crate.

## Why a hand-rolled CBOR codec

§18.1 constrains *decoding* as tightly as encoding, and no serde CBOR crate
does that. A signature is verified over received bytes (§18.3), and those bytes
must independently be proven canonical: otherwise a sender who encodes
non-canonically produces an object that verifies but hashes differently for the
two parties, and every commitment in the protocol — `offer_commit`, the §6
message chain, `H(RECEIPT)` — silently diverges.

The codec makes that structural rather than documented. `decode` succeeding is
proof the input was already canonical, so `decode(b).encode() == b` always
holds, and `SignedBytes` never re-serializes during verification.

`Value` cannot represent a float, a tag, an indefinite-length item, a
non-integer map key, or a duplicate key. Non-canonical objects are not
"rejected"; they are unconstructible.

## Status

**"Done" means implemented and tested here, and nothing more** — not reviewed by
anyone else, and not proven interoperable. That last one is O21's job, and
`../conformance/` is how far it has got. Counts are `#[test]` functions in the
module plus its matching file under `tests/`, where there is one.

### Part V — the wire format

| Component | State |
|---|---|
| Deterministic CBOR (§18.1) | done, 20 tests |
| Money as integers (§18.2) | enforced — floats unrepresentable |
| Domain-separated signing (§18.3) | done, both suites |
| P-256 suite (§4.1) | done, 10 tests — low-s + strict SEC1 |
| State machine (§18.4) | done, 17 tests |
| Reject codes (§18.5) | done |
| Version negotiation (§18.6) | done, 10 tests |
| Domain-separated commitments (§18.3) | done |
| Transport identifiers (§18.7) | done, 5 tests — constants, because nothing below them negotiates |
| Wire objects + field numbering | done, 30 tests — 28 of them end-to-end transcripts (§18.9(4)) |
| Exported vectors (§18.9) | done — 442 cases in `../vectors/v1/` |

### What sits on top of it

One row per module in `src/`, described in its own words — each file's doc
comment is the argument for why it exists, and is worth more than this table.

| Module | What it is | State |
|---|---|---|
| `contact.rs` | Contacts before money, and messages after (§16.9, §16.10) | done, 59 tests |
| `hpke.rs` | Forward-secret message encryption (§16.11) | done, 16 tests — one-time prekeys deleted on use, so a seized phone does not decrypt the archive |
| `trust.rs` | §9.5 — costly identity, and §9.2 — the rated receipt. `BurnProof` (28), `Attestation` (12) and `Vouch` (29), all §18.3 envelopes under the persona named inside them | done, 4 tests — the *arithmetic* is deliberately not here: whether a proof verifies against the chain is the reader's question, and lives with the wallet |
| `verify.rs` | Payer verification — who is doing the signing (§15.5.1) | done, 12 tests. **Local policy, never on the wire**: a payee cannot request a tier, because a counterparty that could would ask for the weakest |
| `escrow.rs` | Escrow (§8.2) and bonded fast settlement (§17.4, §17.5) | done, 29 tests, plus 9 on arbitration (§9.3). No *vector* drives the end-to-end transcripts — the manifest says so, and `../harness/` runs them instead |
| `bond.rs` | Bond capacity, published coarsely (O10) | done, 6 tests — buckets, because an exact `capacity_remaining` is a running meter on the rider's spending |
| `burning.rs` | The burning bug (O15): two outputs, one key image, one spendable coin | done, 7 tests — detection, not immune outputs; the reasoning is in the module |
| `custody.rs` | Where the spend key lives, and what that costs (§4.4) | done, 7 tests |
| `float.rs` | Hot-wallet float sizing (§17.2), and the exposure it forces (O9) | done, 5 tests |
| `backup.rs` | Persona and wallet backup — export and import, under the user's passphrase (§4.3) | done, 35 tests and 9 vectors, plus `tests/from_phone.rs` — the one test that opens a bundle the Android client actually wrote |
| `board.rs` | Who posted a board notice, and what it cost them (§16.18.1, over §15.12's stands) | done, 11 tests. A stand's write key is the cell name hashed, so anyone can overwrite anything; what a notice can still carry is an author and a cost |
| `group.rs` | Group boards (§16.24): a member's page of messages on the shared record | done, 6 tests |
| `publish.rs` | Publications: content keys that derive instead of accumulating (§16.20) | done, 6 tests — **post-1.0 track**, see `research/post-1.0/REPORT.md` |
| `geo.rs` | Geocells (§15.12): turning a place on Earth into a stand name | done, 8 tests — integer arithmetic throughout, because two clients disagreeing on a cell boundary is two people on one corner posting to different boards |
| `position.rs` | The live-position stream after a ride's accept (§15.12) | done, 7 tests — constant-length frames bound to their record, so the ciphertext carries nothing but its own heartbeat |

A vector file's *name* does not map to a module, and reading it that way will
mislead. `contact.json` is the largest family — 301 of the 442 — and carries
`board.sealed`, `group.page`, `position.frame`, `burn.proof`,
`attestation.receipt` and `vouch.known` cases beside the contact ones. A case's
`kind` is the discriminator; `../conformance/README.md` records why.

## Test

    cargo test

386 tests. One is `#[ignore]`d and is not a test of behaviour: `board::pow_cost`
measures what a proof of work costs, which is how `POW_BITS` was picked.
