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

- **Keys come from a trusted dealer** — but *not* because no interactive DKG
  exists. An earlier version of this file said `dkg` 0.6.1 "ships no interactive
  DKG", and the spec recorded that as §8.2's largest gap. **Retracted.** The DKG
  was split into its own crate: `dkg-pedpop` 0.6.0 implements PedPoP, the
  construction the FROST paper specifies, as a three-round protocol with blame
  assignment — precisely what mutually distrusting parties need.

  The real problem is narrower and more awkward: **it does not compose with the
  wallet as published.** Two independent defects, both found by compiling:

  1. `dkg-pedpop` 0.6.0 declares `multiexp` with `features = ["std"]` while its
     own source uses `multiexp::BatchVerifier`, which lives behind the `batch`
     feature. It does not build standalone at all.
  2. Forcing that feature exposes the real conflict. `dkg-pedpop` pins
     `multiexp 0.4`; `modular-frost 0.11.1` — which `monero-wallet` 0.2.0
     requires for multisig — pins `multiexp 0.5`. Both land in the tree, and
     `dkg-pedpop` hands a `BatchVerifier` from 0.4.2 to `schnorr-signatures`
     0.5.3, which wants the 0.5.1 type.

  So **key generation and threshold signing cannot both come from crates.io
  today**. Untested but likely: building against the serai workspace from git,
  where these are version-aligned. The gap is smaller than "nobody has built
  this" and larger than "add a dependency", and the difference matters — the
  first would mean DUCAT must design and audit a DKG, the second means it must
  pin a source and verify a build.
- **The view key is separate shared secret material.** The group *spend* key is
  the FROST group key, which nobody holds the private half of; the view key is
  distributed alongside. Every participant therefore sees every payment into the
  escrow. Correct for an escrow, wrong for a bond, and the spec does not currently
  distinguish them.
## Signing and settlement — done, on stagenet

A 3-of-5 group was funded and spent. **This is a configuration Monero's native
multisig cannot express at all.**

```
group    525uzvqwmLVVTGiMhqwizL3dA2hR…
found    800000000 pXMR at height 2183921
signers  [1, 2, 3] of 5
signed   in 0.088s
txid     3e29714d4ebf28d608804a87b841880fc5077b1ef89d0bf7d8b2cc396559d0fa
mined    height 2183934
```

`user_01` received 400000000 pXMR; the remainder returned to the group as change.
So FROSTLASS forms a threshold wallet2 cannot, and that wallet spends real funds
under a real CLSAG, signed by a subset in under a tenth of a second.

## Two things this run got wrong before it got them right

**A relay accepted transactions and dropped them — twice.** The funding
transaction and the first signed spend were both accepted by
`xmr-lux.boldsuck.org` with `Ok(())` returned, and neither ever appeared on any
other node. The second one never appeared on the accepting node either. This is
§8.7.2's rule earning itself: *a txid from one relay is that relay's word that it
took the transaction, not evidence the network has it.* The spike now submits to
every reachable relay and verifies elsewhere.

**And then the verification lied in the other direction.** The first version
checked propagation with the typed `transaction()` helper, which resolves only
*confirmed* transactions — so it reported a freshly-broadcast transaction as lost
while that transaction was sitting in two independent mempools. A propagation
check that cannot see the mempool is checking the wrong thing, since propagation
is precisely the window before confirmation. Ground truth via `get_transactions`
corrected it.

Worth stating plainly because the two failures look identical from inside the
program: *nothing visible on any relay* was true once and false once, and only an
independent query could tell them apart.
