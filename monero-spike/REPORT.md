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
