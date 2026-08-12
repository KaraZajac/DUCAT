# DUCAT

A peer-to-peer proximity commerce protocol: Veilid for transport, Monero for
settlement, no operator in between.

**[`ducat-protocol.md`](ducat-protocol.md) is the specification** and the primary
artifact here. Everything else exists to keep it honest.

```
ducat-protocol.md   the spec
core/               reference implementation (Rust)
vectors/            conformance vectors + schema — the published artifact
conformance/        three checkers: schema, second implementation, spec audit
harness/            end-to-end over real Veilid routes and real settlement
sim/                offline simulator and market scenarios
android/            the client
research/           one-off measurements: Veilid throughput, Monero multisig,
                    FROSTLASS, wallet-layer probes. Evidence, not product.
```

## Checking it

```sh
cargo test --workspace                      # core, sim, harness
python3 conformance/validate_vectors.py     # every vector against schema.json
python3 conformance/ducat_check.py          # a second implementation runs them
python3 conformance/audit_spec.py           # the document against the code
```

The last one exists because prose drifts from code silently — a stale sentence
throws no exception. It has caught a normative section that was referenced three
times and never written, a field range the registry did not declare, and six
vector kinds the document never named.

## What is proven, and what is not

Demonstrated end to end on stagenet, over live private routes: `direct`, `fast/1`
and escrow settlement; both tap directions; ten attacks refused; the abandonment
paths that leave a single-sided receipt.

Not proven, and stated here rather than buried: **no external adversarial
review** (§2.5 is the project's own argument for why that matters), no
implementer who has never read `core/` (O21), and no measurement of the tap on a
handset. The latency figures in §8.7.2 are a desktop with an attached node.
