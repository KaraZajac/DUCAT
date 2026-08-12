# DUCAT Monero Spike — `monero-wallet` 0.2.0 (monero-oxide)

Assessment of the crate DUCAT intends to embed (§8.2), against what Part IV
actually requires. Source-level review of `monero-wallet 0.2.0`; no funds moved.

## What is there

| Requirement | Status |
|---|---|
| FROSTLASS multisig (§17.2 bond) | **Present**, behind the `multisig` feature. Entry point `SignableTransaction::multisig(ThresholdKeys<Ed25519>) -> TransactionMachine` |
| Subaddress generation (§15.10 fresh-per-tap) | **Present** — `SubaddressIndex`, `subaddress_keys` |
| Scanning / view keys | **Present** — `ViewPair`, `Scanner`, plus burning-bug-immune variants |
| Transaction building and signing | **Present** — `send` module, `SignableTransaction` |

## What is missing, and it matters

### 1. No transaction proofs at all

A source-wide search for `out_proof`, `tx_proof`, `OutProofV`, `InProofV`,
`prove_tx`, and `check_tx_key` returns **nothing**. §17.3 Layer 1 and §17.4's
`TXPROOF` message both assume `OutProofV2`-style proofs exist.

**But most of that requirement dissolves on inspection.** A tx proof exists to
convince someone who is *not* the recipient. The driver accepting a fare **is**
the recipient: given the transaction, their own view key tells them whether it
pays their subaddress. A proof handed over by the payer is a claim the driver
must independently validate anyway — so for the acceptance path it is redundant,
and §17.4's flow already has the driver checking the mempool regardless.

Where a proof is genuinely irreplaceable is **arbitration** (§17.5). An arbiter
adjudicating a slash claim is not the recipient and must verify payment without
being handed the driver's view key, which would expose their entire income.

So the recommendation is to split the requirement:

- **Acceptance** — the driver scans the transaction with their own keys. Simpler,
  unforgeable, and needs nothing from the payer but a txid.
- **Dispute evidence** — a proof is still required, and **must be implemented**;
  the crate will not provide it.

### 2. Scanning is block-oriented, which fights zero-conf

`Scanner::scan` takes a `ScannableBlock`. The per-transaction path,
`scan_transaction`, is **private**. There is no public API for scanning a loose
or unconfirmed transaction.

That is precisely the operation `fast/1` acceptance needs: the driver has a
mempool transaction and wants to know, in seconds, whether it pays them. As it
stands the public API can only answer that once the transaction is in a block —
which is the twenty-minute wait Part IV exists to eliminate.

This is an API limitation rather than a protocol one, and a small one: the
function exists and is merely not public. Options are to upstream a public
`scan_transaction`, carry a patch, or vendor. It should be resolved before
Phase 3, not during it.

### 3. Burning-bug immunity exists but is non-standard

`GuaranteedViewPair` / `GuaranteedScanner` provide outputs immune to the burning
bug, with an explicit caveat in the source:

> 'Guaranteed' outputs ... are not officially specified by the Monero project.
> They should only be used if necessary. No support outside of monero-wallet is
> promised.

This is relevant to `pos/1`. The burning bug lets an attacker send a merchant two
outputs sharing a one-time key; the merchant sees two payments and can spend only
one. §15.10's mandatory fresh-subaddress-per-tap narrows the window but does not
close it, since the attack lives *within* a single subaddress.

The embedded-wallet decision makes guaranteed outputs usable — both sides run
DUCAT — but it is a **lock-in**: funds received to guaranteed outputs have no
promised support in any other wallet, which cuts against A1's bearer property
and against §11's many-clients goal. A second DUCAT implementation would have to
reimplement an unspecified scheme.

**Recommendation:** do not adopt guaranteed outputs by default. Treat the burning
bug as a `pos/1` hazard to detect (a merchant client should flag duplicate
one-time keys across received outputs) rather than a reason to leave the
standard.

## Consequences for the spec

- §17.3 Layer 1 should say the recipient **scans**, not that the payer **proves**.
- §17.4's `TXPROOF` moves from the acceptance path to dispute evidence, and its
  implementation becomes DUCAT's own work.
- Two wallet-layer work items now block Phase 3: a public unconfirmed-transaction
  scan, and a proof implementation for arbiters.
- Neither blocks Phase 1, which needs no bonds and no zero-conf.

## Not tested

Everything requiring funds: spending from a FROSTLASS multisig, output locks and
pre-splitting (§17.2), and restore-height sync cost. Those need a stagenet
faucet and are the next step.
