# Live market run — stagenet

Five participants, six transactions, real sXMR, real protocol. Every transcript
verified by both parties.

## Settled

| Transaction | Amount | Flow | txid |
|---|---|---|---|
| coffee | 0.000600 | user_01 → coffee_01 | `332e95263326bd23…` |
| ride | 0.000900 | user_02 → taxi_01 | `3ff07d45d51268bd…` |
| bread | 0.000500 | user_01 → shopkeep_01 | `e90fabe12e364b63…` |
| cheese | 0.000700 | user_01 → shopkeep_01 | `a8eee4ecfd92a775…` |
| payback | 0.000800 | user_02 → user_01 | `f7143dfa0a9f5058…` |
| vendor-to-vendor | 0.000500 | coffee_01 → shopkeep_01 | `e503368cd8141112…` |

Three profiles exercised: `pos/1`, `ride/1`, `xfer/1`. The last one matters more
than it looks — a vendor spending what it earned is the market closing a loop
rather than a demo paying out of a faucet.

## §17.2 confirmed in a market, not a drain test

`user_01`'s unlocked outputs before each of its three purchases:

```
5 output(s), 0.010000 XMR   →  coffee   (0.0006)
4 output(s), 0.008000 XMR   →  bread    (0.0005)
3 output(s), 0.006000 XMR   →  cheese   (0.0007)
```

**One output consumed per payment, and unlocked value falling by exactly 0.002 —
a whole output — every time**, regardless of whether the payment was 0.0005 or
0.0007. The change returns locked for ten blocks, so it is unavailable for the
next purchase.

`user_01` made three consecutive purchases only because it was funded with five
pre-split outputs. Funded with one output of the same total value it would have
bought the coffee and then failed at the next counter with its balance intact on
screen — which is precisely the curb-side failure §17.2 exists to prevent, and
the reason consecutive capacity is a *count* rather than a balance.

## Wire sizes, measured

```
TapPresent   217 B      ACCEPT    186 B
FullOffer    226 B      RECEIPT   166 B
```

Four messages, ~795 B for a complete transaction including signatures and
envelopes. `TapPresent` at 217 B matches §15.3.1's figure exactly.

## What broke, and what it taught

**A relay died mid-run and one wallet silently stopped syncing.** `user_01` sat
four blocks short of the chain, kept answering `get_height` with a plausible
number, and never saw funds that had already settled. Four other wallets on
healthier relays were fine. Nothing surfaced as an error.

This is §8.7.2's documented trap, reproduced against the project's own code
within an hour of the section being written. It promoted the rule from SHOULD to
MUST and produced two additions to the spec: detection requires comparing the
wallet's height against the relay's own, because a stalled wallet and a synced
one give identical answers when asked alone; and silent divergence is worse for
a *payee*, who ends up telling a customer "not received" about money that is
already settled.

The wallet client now holds four relays and rotates on refresh failure. It fired
repeatedly during this run and recovered every time — visible in the log as
`refresh failed, rotating to …` immediately followed by `recovered on relay …`.

**Relay order matters more than it should.** With a flapping relay first in the
list, every call paid a timeout before rotating. Reordered by observed
stability, and `wait_for_outputs` now builds its wallets once rather than
recreating them each poll — a client that forgets which relay works has not
really implemented failover.

## Reproducing

    cargo run -- --live

Needs five `monero-wallet-rpc` instances on ports 28101–28105 with funded,
pre-split wallets. `monero-spike/` holds the setup.


---

# Drain to exhaustion — the model was too confident

The market run showed `user_01` consuming one output per purchase and looked
like clean confirmation of §17.2. A dedicated drain test, spending until
refused, disagreed:

```
starting: 0.008909 XMR across 6 unlocked outputs
predicted: 6 consecutive purchases

purchase 1:  6 outputs → 4 outputs    (consumed 2)
purchase 2:  4 outputs → 2 outputs    (consumed 2)
purchase 3:  2 outputs → 1 output     (consumed 1)
purchase 4:  1 output  → 0 outputs    (consumed 1)
purchase 5:  refused

actual: 4
```

**A payment can consume more than one output.** Input selection is the wallet's
decision — fee coverage, consolidation, or a preference for multiple inputs — and
the client does not control it. So `capacity = count(unlocked outputs)` is an
upper bound, and the earlier seven-outputs-seven-payments result was a run in
which one input happened to suffice every time. Reading it as an equality was
over-fitting to a single sample.

§17.2 now states the bound rather than the equality, requires provisioning more
outputs than the naive calculation suggests, and forbids promising an exact
count to a user: *"about 4 more payments"* is honest, *"4 more payments"* is not.

## The part that worked

The refusal came from the client's **own pre-check**, before any offer was
presented — not from a failed settlement with a customer waiting. That is the
behaviour §17.2 asks for, and the negative path had never been exercised until
this test.


---

# Device loss and restore — stagenet (§4.3)

    cargo run -- --restore

A backup is the one feature whose bugs surface only when it is already too late,
so this exercises the claim rather than the cryptography: **one 368-byte
encrypted file and a passphrase, restored onto a wallet-rpc instance in a fresh
directory on an unused port that had never seen the seed.**

```
original    59jre4NqgtUWBYoz…   7560100000 pXMR   4 unlocked outputs
exported    368 bytes            seed absent from the ciphertext
restored    59jre4NqgtUWBYoz…   7560100000 pXMR   4 unlocked outputs
```

Address, balance, **and output count** all match. The third is not decoration:
§17.2 makes consecutive payment capacity a function of output *count*, so a
restore that recovered the right balance across the wrong number of outputs would
mis-predict how many payments the user can make and fail at a counter.

## The height is wrong in both directions, asymmetrically

`restore_deterministic_wallet` from 500 blocks back took **118 seconds** — about
4 blocks/second against a remote node. The subsequent `refresh` was 0.9s. That
sets the price of backdating: 500 blocks ≈ 2 minutes, and the genesis case is the
~106 hours Phase 0b measured.

Then the failure the field exists to prevent, run deliberately:

```
restore_height = 2183821   (the height at export — the obvious implementation)
balance  0 pXMR across 0 unlocked outputs
```

**Correct seed, correct address, zero balance, no error raised.** A wallet
restored from a height above its own outputs scans forward past all of them and
finds nothing to report. Nothing in the log, the RPC response, or the balance
distinguishes this from a genuinely empty wallet — and the user's reasonable
conclusion is that the backup lost their money.

So "stamp the current height at export" is not a slow restore, it is a total one.
§4.3.1 now states the exact rule — at or below the oldest unspent output, as
close to it as possible — and both directions are demonstrated here rather than
asserted.
