# ducat-sim

A five-participant DUCAT market, running the real protocol.

    cargo run          # summary
    cargo run -- -v    # every message on the wire

## What is real

Keys, signatures, canonical encoding, the wire objects, the contract state
machine, commitment verification, and transcript checking all come from
`ducat-core`. Messages cross as bytes and are opened and verified exactly as a
client would.

## What is simulated

The transport is an in-process queue rather than a Veilid private route, and
settlement is bookkeeping rather than on-chain transactions. Both were measured
separately — Veilid throughput in `phase0/`, Monero behaviour in
`monero-spike/` — and debugging three unfamiliar systems at once makes each
harder to see.

Addresses are placeholders. Fresh subaddresses per tap are mandatory in
production (§15.10) and the simulator does not model that.

## Participants

    user_01, user_02     consumers
    taxi_01              ride/1
    coffee_01            pos/1
    shopkeep_01          pos/1, several items

## Scenarios

Four transactions across three profiles, plus two attacks that must be refused:
an offer swapped after the tap committed to it (§15.3), and an out-of-order
message — the shape that drained RetoSwap (§2.5).

## What it has already found

The originator-versus-evaluator bug in §18.4.1, on its first run. The rule
"only the payer may emit ACCEPT" had been implemented as a check on the local
role, so a payee refused every ACCEPT it received. Seventy-five tests missed it
because each drove the state machine from a single side; two parties exchanging
real messages did not.
