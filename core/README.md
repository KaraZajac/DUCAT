# ducat-core

Wire format and contract logic for the DUCAT protocol (Part V, §18). No I/O, no
platform dependencies — testable in isolation and reusable by any client.

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

| Component | State |
|---|---|
| Deterministic CBOR (§18.1) | done, 18 tests |
| Money as integers (§18.2) | enforced — floats unrepresentable |
| Domain-separated signing (§18.3) | done, suite 1 only |
| P-256 suite (§4.1, Core conformance) | **not implemented** |
| State machine (§18.4) | done, 17 tests |
| Reject codes (§18.5) | done |
| Version negotiation (§18.6) | not started |
| Exported test vectors (§18.9) | not started — tests are in-tree only |

## Test

    cargo test
