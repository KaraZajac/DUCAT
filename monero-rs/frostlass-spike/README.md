# FROSTLASS spike (O1)

    cargo run              # threshold ceremony comparison
    cargo run -- --keygen  # form a 3-of-5 group, save it, print a fundable address
    cargo run -- --spend   # scan, sign with 3 of 5, broadcast

Everything empirical in this project so far is Monero's **native** multisig — the
2-round/134 s ceremony, the bond seized by arbiter + recovery, the 2,286-byte key
file. §8.2 intends to ship FROSTLASS, and the spec had been carrying its claims on
the strength of a README.

## The decisive result

Monero's native multisig supports only **N-of-N and (N−1)-of-N**. 2-of-3 exists;
**3-of-5 does not**. That is why §9.3's "multiple arbiters can co-sign for
higher-value escrows" was unbuildable on the path this project actually tested,
and why one of O22's candidate directions — an arbiter-heavy composition letting
two arbiters execute a ruling without the counterparty — was never available.

FROSTLASS forms all of these:

```
2-of-3    share 151 B    keygen 0.10s     (wallet2 can do this too)
3-of-5    share 215 B    keygen 0.20s     wallet2 cannot express this
2-of-5    share 215 B    keygen 0.18s     wallet2 cannot express this
7-of-11   share 407 B    keygen 0.73s     arbitrary thresholds
```

So the threshold limitation is an implementation choice, not a property of Monero.

## A claim this program made and its own output refuted

An earlier draft printed that each participant holds "one share of a fixed size
regardless of the group's shape". The numbers say otherwise: **a share is linear
in `n`** — 32 bytes per participant, independent of `t`. 151 / 215 / 407 bytes for
n = 3 / 5 / 11.

Linear is still the point. wallet2 gives each member a *combinatorial* set of
keys, and its 2-of-3 wallet file measured **2,286 bytes against 151 here** — same
group, 15× smaller, and serializable through `ThresholdKeys::serialize()` rather
than needing §4.3.3's file-copy workaround, which exists only because
`monero-wallet-rpc` has no multisig restore method.

## What this run does not settle

- **Keys come from a trusted dealer.** `dkg` 0.6.1 ships **no interactive DKG** —
  the crate exposes `ThresholdKeys::new` and nothing that runs a ceremony. A real
  deployment needs one, and a dealer who keeps the polynomial holds every share.
  This is a genuine gap for §8.2's "embed a wallet" plan and is not visible from
  the README's claims.
- **The view key is separate shared secret material.** The group *spend* key is
  the FROST group key, which nobody holds the private half of; the view key is
  distributed alongside. Every participant therefore sees every payment into the
  escrow. Correct for an escrow, wrong for a bond, and the spec does not currently
  distinguish them.
- **Signing and settlement** are exercised by `--spend`, separately.
