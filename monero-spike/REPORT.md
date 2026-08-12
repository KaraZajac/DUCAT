# DUCAT Monero Spike — Multisig (O1)

Run 2026-08-11 against Monero **v0.18.5.1** (current release, hash verified
against the published `hashes.txt`) on **stagenet**, via the remote node
`node.monerodevs.org:38089`.

Target: open problem **O1** — *"Monero multisig fragility makes §8.2 the
riskiest engineering in the protocol"* — and §17.2's `FLOAT`, whose bond is a
2-of-3 multisig `{ user, market_arbiter_set, recovery }`.

---

## Headline: it works, and the problem is not fragility

```
CONVERGED — all three parties agree
  53hUxmYTwGtR44fhL8f7JLATagSwjtdLB6y4Q3wQQnbtUsDiLTLCzwnKr2gtBRAAUdgWmD22pJ3GK5Z52sJpgiK624iqtKh
  exchange rounds:  2
  wall clock:       134s   (including wallet creation and two service restarts)
```

All three wallets reported `{"multisig":true,"ready":true,"threshold":2,"total":3}`
and derived the *same* address. §17.1's argument — that multisig is tolerable
because setup happens once, off the critical path, with unlimited retries — is
**supported**. Two minutes, deterministic, no flakiness observed across runs.

The real obstacle is somewhere else entirely.

## Monero disables multisig by default

Every `exchange_multisig_keys` call is refused unless a per-wallet flag is set.
The refusal text is upstream's own assessment, quoted in full because it bears
directly on §8.2:

> This wallet is multisig, and multisig is disabled. Multisig is an experimental
> feature and may have bugs. Things that could go wrong include: **funds sent to
> a multisig wallet can't be spent at all**, can only be spent with the
> participation of a malicious group member, or **can be stolen by a malicious
> group member**. You can enable it by running this once in monero-wallet-cli:
> `set enable-multisig-experimental 1`

That is the current release describing the mechanism DUCAT's escrow (§8.2) and
bond (§17.2) both rest on.

## There is no RPC path to enable it

`monero-wallet-rpc --help` exposes no multisig option. The only route is:

```
monero-wallet-cli --wallet-file W --command set enable-multisig-experimental 1
```

which persists into the wallet file. **A DUCAT client cannot enable multisig
through the standard integration surface.** It must drive `monero-wallet-cli`
out-of-band — not something a phone app does — or link the wallet library
directly and bypass the RPC.

This is a bigger constraint on client architecture than fragility ever was, and
§17.1's "calm, retryable onboarding flow" quietly assumes an API that does not
exist.

## A wallet can be stranded halfway

`prepare_multisig` and `make_multisig` **both succeed with the flag off**. Only
`exchange_multisig_keys` refuses. So a client that has not set the flag will:

1. prepare — succeeds
2. make — succeeds, wallet is now `2/3 multisig (not yet finalized)`
3. exchange — refused
4. retry from prepare — refused, *"This wallet is already multisig"*

The wallet is unusable and cannot be rewound. Recovery means discarding it and
restarting the ceremony with all three parties. Any client implementing §17.2
must check the flag **before** step 1, because there is no check after it.

## Correction: a finding I got wrong mid-run

I initially concluded that `make_multisig` resets the flag to `0`, and wrote a
ceremony script that stops wallet-rpc mid-flow to re-set it. **That was wrong.**
The flag reads `1` after `make_multisig` in a clean run.

The `0` I observed came from **stale wallet-rpc processes** left running from an
earlier attempt: they still held the previous wallets in memory, kept the ports
bound, and wrote their own state over files I had recreated underneath them.

This is the second time in this project that orphaned background processes
manufactured a false finding — the first was Phase 0's "Veilid needs port
forwarding," which was a port conflict with my own earlier node. The lesson is
now recorded twice: **before trusting a negative result from a daemon-backed
test, confirm no earlier instance is still running.**

Consequently, whether the mid-ceremony stop/restart in `full_ceremony.sh` is
actually required is **untested**. The successful run included it, so it is
sufficient; it may well be unnecessary.

## Impact on the spec

| Claim | Status |
|---|---|
| §17.1 "setup is fragile but retryable off the critical path" | **Supported** — 2 rounds, 134 s, deterministic |
| §17.1 "a calm onboarding flow" | **Qualified** — needs an out-of-band CLI step no RPC exposes |
| §8.2 "multisig is multi-round and historically brittle" | **Overstated on brittleness, understated on availability** |
| O1 "the riskiest engineering in the protocol" | **Reframe** — the risk is upstream's experimental status and the missing API, not round-trip fragility |

§8.2's hazard list should carry upstream's warning verbatim. A protocol whose
escrow depends on a feature its own implementation says may render funds
unspendable is making a bet that belongs in the document, not in a footnote.

## Not yet tested

- Signing an actual transaction from the multisig wallet (needs stagenet funds)
- Output locks and pre-splitting (§17.2)
- `OutProofV2` generation and verification (§17.3)
- Restore-height sync cost from a wallet's perspective (§17.1)

## Reproducing

```
./full_ceremony.sh          # complete ceremony from empty wallets
./multisig_test.sh          # ceremony only, assumes wallets exist with the flag set
```

Both need three `monero-wallet-rpc` instances on ports 28088–28090.
**Check `pgrep -f monero-wallet-rpc` first** — see the correction above.


---

# Pre-split measurement — inconclusive, and why

Attempted: measure consecutive payments before and after splitting the float,
expecting the count to track unlocked outputs.

Result as printed: `2 outputs -> 1 payment`, `13 outputs -> 1 payment`. **Do not
read that as "pre-splitting does not help."** The test did not isolate the
variable, for two reasons, both mine:

1. **The wait loop exits on any unlocked balance.** In phase C it fired when
   phase A's *change* unlocked, while the ten split outputs still had a block to
   run. Phase C therefore never exercised the split outputs.
2. **`incoming_transfers` with `transfer_type: "available"` includes locked
   outputs.** It reported 13 while `unlocked_balance` covered a fraction of
   them, so the "13 unlocked outputs" figure was never real.

Two genuine findings survive, and both belong in a client:

- **Fees must be reserved in the capacity calculation.** The final refusal came
  with exactly the payment amount unlocked and nothing for the fee. A client
  computing capacity as the full unlocked value overstates by at least one fee.
- **"Available" is not "unlocked."** A client counting spendable outputs from
  that RPC field will believe it holds funds it cannot spend.

The lock itself was confirmed twice — a second consecutive payment was refused
in both phases.

**Redesign needed:** wait on the specific split outputs (track by height or by
unlocked count crossing a threshold, not by `unlocked_balance != 0`), and size
payments so that unlocked *value* is never the binding constraint before
unlocked *output count* is.


---

# Pre-split, take two — confirmed

v1's faults fixed: wait on unlocked value crossing the split total (a threshold
leftover change cannot satisfy), and payments sized at an eighth of one output
so unlocked value could not run out before output count did.

```
payment 1 ok  unlocked 35000000000      payment 5 ok  unlocked 15000000000
payment 2 ok  unlocked 30000000000      payment 6 ok  unlocked 10000000000
payment 3 ok  unlocked 25000000000      payment 7 ok  unlocked  5000000000
payment 4 ok  unlocked 20000000000      payment 8 REFUSED       0
```

**Each payment of 0.0005 XMR consumed exactly 0.005 of unlocked balance.** The
same tenfold discrepancy every time, because a payment consumes a whole output
and the change comes back locked for 10 blocks. Seven unlocked outputs bought
seven payments; the eighth was impossible with 0.05 XMR still in the wallet.

**Consecutive capacity is a count, not a balance.** A float holding one large
output makes exactly one payment per lock interval regardless of its size.
§17.2's pre-split requirement is confirmed, and its capacity formula now
distinguishes single-payment capacity (a value) from consecutive capacity (a
count).

## The script's own verdict was wrong

It printed *"unlocked value was nearly gone: this measured fee headroom, not
output availability. Inconclusive again."* That heuristic checked whether value
remained at the refusal, and concluded that an empty balance meant value
exhaustion rather than output exhaustion.

They are the same event. Value hit zero *because* every output had been consumed
and its change locked. The stepwise 0.005 decrements are the evidence, and the
detector was looking at the wrong variable — a reminder that an automated
inconclusive-check is only as good as its model of the failure it is screening
for.


---

# Slashing — a bond can be seized over its holder's objection

Two tests, the second stricter than the first.

**Test 1** signed with `arbiter + recovery`, excluding the user from signing —
but all three wallets exchanged key images beforehand. That is not the slash
scenario. A rider facing a slash does not help.

**Test 2** never contacted the user wallet at all:

```
arbiter exported  1800 chars
recovery exported 1800 chars
arbiter imported recovery's only:  n_outputs=2
recovery imported arbiter's only:  n_outputs=2
submitted: 16bd0ee2f57fb777636c05b981091609d5a4571646abb345941d528ba1ce42a7
```

**Key-image reconstruction needs only the threshold count, not all
participants.** This was the open question and it was not guaranteed: Monero
builds key images from participants' partial ones, and had all three exports
been required, a 2-of-3 bond could only have been seized with the cooperation of
the party being seized from. §17.2's deposit model would have collapsed.

It requires two. Seizure over the holder's objection works, and collateral that
can be taken over an objection is collateral.

## What this does not establish

- **This was wallet2's multisig, not FROSTLASS.** §8.2 intends to ship
  monero-oxide's implementation. The *mechanism* is proven; the *code path* is
  not, and the two are different protocols producing the same on-chain result.
- **Nothing about arbiter honesty.** O8 is untouched. This shows a market
  *can* seize a bond, which is exactly as true for an honest market as a
  captured one.
- **Nothing under adversarial timing.** Both parties here were cooperative and
  online.

## Incidental

Funding a bond needs no coordination — the multisig sees incoming funds on a
plain refresh, and `multisig_import_needed` is a spend-time cost only. A freshly
funded bond is not slashable for ~10 blocks, like any other output.


---

## Multisig backup: exportable, and not importable where it matters

Prompted by §4.3. If a bond or escrow share could be backed up, the backup bundle
should carry it. Measured on v0.18.5.1/stagenet against the existing 2-of-3
`ms_user` wallet.

**A multisig wallet has a seed, and it is not 25 words.**

    query_key {"key_type":"mnemonic"}  →  592 hex chars (296 bytes)

**The seed is sufficient.** Restored into a fresh directory, nothing copied:

    monero-wallet-cli --stagenet --restore-multisig-wallet \
      --generate-new-wallet /tmp/ms-cli/restored --restore-height 2183000

    Generated new 2/3 multisig wallet:
      53hUxmYTwGtR44fhL8f7JLATagSwjtdLB6y4Q3wQQnbtUsDiLTLCzwnKr2gtBRAAUdgWmD22pJ3GK5Z52sJpgiK624iqtKh

Byte-identical to the original group address. The multisig restore path works.

**`monero-wallet-rpc` cannot use it.**

    restore_multisig_wallet  →  -32601 Method not found

Not an argument error — the method does not exist. The binary's own method table
confirms the asymmetry: `prepare`, `make`, `exchange`, `export`, `import`,
`sign`, `submit`, `is_multisig`. There is an export and no restore. Only the
interactive CLI, behind a `Y/N` experimental-feature prompt, can reconstruct a
share.

Two gates, not one: the CLI also refuses outright with `Error: Multisig is
disabled.` until the prompt is answered, and `--enable-multisig-experimental` is
a **wallet-rpc** flag that the CLI rejects as an unknown option. Anyone
automating this will hit both.

### Why this changed the spec rather than the backup format

wallet-rpc is the integration surface a phone client has. A multisig share in the
backup bundle would be a field a client can write and can never read back —
advertising a recovery that does not exist, discovered at the one moment the user
depends on it. So the bundle omits it deliberately (§4.3.3).

For **bonds** that costs nothing: §17.2 puts both non-user keys in the market's
arbiter set, so a bond never needed the user's signature to move. Losing the
device loses no capability the user had.

For **escrow** it is a real hole. §8.2's 2-of-3 is buyer, seller, arbiter. A
buyer who loses their device leaves every buyer-favourable outcome needing the
*seller's* signature — including a `RULING` for the buyer, since a ruling **is** a
co-signature and the arbiter provides only one of two. §9.3.4's expiry rule
cannot rescue it: it guarantees a ruling exists, not that two live keys remain to
execute one.

**DUCAT can restore a lost identity and a lost wallet. It cannot restore a lost
escrow.** §4.3.3 records what bounds the exposure — escrow is not the default
path, the window is short, and the value is known before entering — but none of
those is a fix.

### Correction: shares *are* recoverable — the earlier conclusion asked the wrong question

The section above concluded that multisig shares could not be backed up, because
`monero-wallet-rpc` has no restore method. The measurement was right and the
conclusion was wrong. It assumed restoring a share means *reconstructing* it.

**Reconstruction genuinely fails.** Two wallets given byte-identical key material
(the same `.keys` file copied twice, so a perfect restore) each ran
`prepare_multisig`:

    a: MultisigxV2R16v4qNgoz4exPFek7tiUXs6XjEWAt3J87eifHwHGv4RG…  189 chars
    b: MultisigxV2R16v4qNgoz4exPFek7tiUXs6XjEWAt3J87eifHwHGv4RG…  189 chars

    common prefix: 101 chars, then 88 chars of divergence

So the ceremony draws fresh randomness. Restoring a seed and replaying a recorded
ceremony produces a *different* share. There is no derivation shortcut.

**But reconstruction is unnecessary — the share is already a file.** Copying only
the 2-of-3 wallet's `.keys` into a virgin directory and starting a wallet-rpc
there:

    is_multisig: {'multisig': True, 'ready': True, 'threshold': 2, 'total': 3}
    address:     53hUxmYTwGtR44fhL8f7JLATagSwjtdLB6y4Q3wQQnbtUsDiLT…   (exact)
    export_multisig_info:  2508 chars
    balance:     96908580000 pXMR, scanned from the chain

`restore_multisig_wallet` is never called. The missing method is not on the path.

**Sizes decide feasibility:**

    ms_user.keys      2,286 bytes    ← the share. Back this up.
    ms_user      52,686,781 bytes    ← scan cache. Rebuilds itself. Do not.

### What was not demonstrated

An end-to-end multisig spend from the restored share. It refused with `-16 No
transaction created` — **and so did the original wallet**, identically, because
both need a fresh multisig-info exchange after the earlier spend test consumed
theirs. So the copy is indistinguishable from the original at every point
measured, which is the claim being made. It is not the same as having watched a
restored share co-sign a real transaction, and that test is still owed.

### Why this closed O22

The problem was never that funds were locked. It was *who held the key*. A buyer
who lost their device needed the **seller's signature** to receive a ruling in
their own favour — an adversary asked to sign away their own claim, which nothing
compels. Now they restore their own share and need the other participants to
re-share multisig info, which endorses no outcome and authorises no transfer.

The ask moved from an adversary's consent to a participant's cooperation in a
mechanical step. That is the whole difference.
