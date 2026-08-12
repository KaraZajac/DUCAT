# End-to-end harness

    ducat-harness --payee [amount_pxmr]   # allocates a route, writes a tap, serves
    ducat-harness --payer                 # reads the tap, transacts, settles

Two **separate processes**, two Veilid nodes, one Monero settlement.

Everything before this exercised the protocol against an in-process queue (`sim`)
or the transport against synthetic payloads (`phase0`). Neither answers the
question the spec makes claims about: *does a DUCAT transaction complete between
two nodes that have never met, over an anonymous route, ending in money moving?*

The tap is a **file**, deliberately. A tap is an out-of-band channel — a QR code
or an NFC exchange (§15.3) — and modelling it as bytes one process writes and
another reads is more faithful than passing a struct between threads. The payer
starts knowing nothing but those bytes.

## A run

```
payee                                     payer
  route    736 B blob
  tap      906 B written                → tap verified, route imported
  → offer requested                       offer verified against the tap
                                          verify satisfied (§15.5.1)
  → ACCEPT verified: 600000000 pXMR     ← ACCEPT signed and sent
                                          fund 58ede0b37496b479…
                                          relayed, visible on a node it
                                          did not submit through
  → TXID — scanning with my own view key
  ✓ observed 600000000 pXMR on chain
  CLOSED — receipt co-signed             CLOSED — transcript verified
```

`600000000 pXMR` settled, txid
`58ede0b37496b479da9ad4e4cbf723cc8980473380d4766549fcf12884ca46fb`.

Note what the payee does with the TXID: **it scans**. §17.4 makes the payee the
recipient, so its own view key answers "was I paid" and the payer's claim is only
a pointer to where to look.

## What it found

**A bug that no unit test could have caught.** `TapPresent` and `Accept` both
length-checked `dest` as **16 bytes** — the line was copied from the `nonce` read
above it. A Monero address is 95 characters, so any object naming a real
destination decoded as `MALFORMED`.

Every existing test passed `dest: None` or a 16-byte placeholder, so the fixtures
agreed with the bug. It surfaced the first time two processes exchanged a genuine
address over a live route, which is exactly the class of defect an integration
harness exists for.

**And two of its own, worth keeping because they were instructive:**

- The payee used `?` on a decode error, so a malformed message *killed the
  server* rather than being refused. To the payer that looked like a network
  timeout, which sent the investigation in the wrong direction. A server that
  dies on bad input has converted every client error into an outage.
- The propagation check fired the instant `transfer` returned and reported a
  perfectly healthy transaction as lost — it was in two independent pools seconds
  later. Propagation is not instantaneous. The check now retries on a bound,
  because the failure worth catching is a transaction that is *never* visible,
  and only waiting distinguishes that from one that is not visible *yet*.

## fast/1

    ducat-harness --payee 500000000 --fast
    ducat-harness --payer

Adds the bond leg (§17.4). The payer posts a `bond_proof` **before** the ACCEPT —
a provider that learns the bond is inadequate *after* accepting has already made
the decision the bond exists to inform — and publishes a **capacity bucket**
rather than a balance:

```
bond 100000000000 pXMR posted; publishing capacity bucket 50000000000
                              (true remaining 60000000000 withheld)
```

The provider learns "at least 0.05", not the running meter on the rider's
spending that an exact figure would be (§17.8, O10). Settlement then accepts at
**mempool visibility** rather than confirmation, which is what `fast/1` is for.

txid `a467796245c1b83a2a7f42316703b7d512009a938e0d3df75d62bd6443932b79`.

## Escrow

    ducat-harness --escrow-serve seller
    ducat-harness --escrow-serve arbiter     # start staggered; see below
    ducat-harness --escrow-drive

Three parties, three Veilid nodes. Escrow is the first flow where the *number of
parties* is part of the security argument, and where the failure that matters is
not a bad signature but a **message arriving out of turn** — §2.5's exploit
drained a production system of ~$2.7M with a forged, out-of-order ACK.

```
ceremony — 2 rounds, strictly ordered
  round 0 closed by all participants
  round 1 closed by all participants

attack — replaying a settled round (§2.5's shape)
  refused — StateViolation: the ceremony is finished

agreement
  all three formed 53hUxmYTwGtR44fhL8f7JLAT…

RELEASED — 500000000 pXMR to a bound party
```

**Precisely what that refusal proves:** the replay arrived *after* the ceremony
closed, so the guard that fired was "finished", not "expected round 0, got 1".
The mid-ceremony case — round *n+1* while *n* is still open, which is the exact
shape of the RetoSwap message — is covered in `core/tests/escrow.rs`. Both are
`RoundTracker` refusing, but they are different arms and it is worth not
overstating which one ran on the wire.

The Monero multisig underneath is the 2-of-3 already formed in `monero-spike/`;
forming a fresh one needs the wallet2 CLI dance (§8.2), which is orthogonal to
what this demonstrates.

### Two harness bugs worth keeping

- **Two nodes raced on Veilid's protected store** and one died with `Could not
  initialize the protected store`. The store is keyed by *program name*, and both
  roles shared one while differing only in namespace — a lock nobody declared.
  Each role now has its own program name, and starts are staggered.
- **A participant never receives its own ceremony contribution over the wire** —
  it generates it. The first version only fed incoming messages to `RoundTracker`,
  so each server saw two of three contributions, never closed a round, and
  refused the next one as out of order. That made `RoundTracker` look broken when
  the model was wrong: a participant records its own contribution locally and
  collects the others.

## Tap latency (§15.3)

The payer times from tap-read to confirm screen. Node startup is excluded — a
phone keeps its node attached, and the three seconds are the *user's* wait.

```
run 1   route import 0 ms + round trip  34 ms  →  0.03 s
run 2   route import 0 ms + round trip 221 ms  →  0.22 s
run 3   route import 0 ms + round trip 297 ms  →  0.30 s
```

All well inside budget. **The spread is the finding, not the best case** — an
order of magnitude across three consecutive runs on identical hardware. A budget
argued from 34 ms would be a budget argued from a sample. Route import costs
nothing because it is local parsing, not a network call.

What this does not cover: a phone, a cold node, cellular, more hops, or a route
that must be re-established. Each is additive and the last is bounded below by a
full round trip. **The protocol is not the problem; that is all this shows.**

## The other direction: customer presents, till scans

    DUCAT_PAYER_WALLET=user_02 DUCAT_PAYER_PORT=28102 ducat-harness --present
    ducat-harness --scan 300000000

Everything above has the *payee* presenting — which is a POS terminal, and §15.2
calls it the normal case. This is the inversion familiar from Alipay and WeChat:
the customer holds out a code, the till reads it and charges.

It is not a curiosity. **§15.3.2 leans on it as the iOS escape hatch** — an
iPhone cannot present over NFC (O19), so an iOS merchant either inverts the roles
or falls back to QR. The variant existed in the enum and nothing had ever built
one.

```
till                                    customer
  tap: payer-presented, amount mine   ← presenting, waiting to be charged
  charging 300000000 pXMR             → confirming (§15.5)
  accept verified                     ← ACCEPT signed
  (polls)                               funded e0df596201b500e0…, propagated
  txid — scanning with my own view key
  ✓ observed 300000000 pXMR
  CLOSED, receipt issued              → CLOSED, receipt received
```

### What inverting taught

The presenter supplies *reachability*, so the reader drives — and here that means
**the till polls**, because the customer holds the route and cannot call out.

- **A presenter's loop must stay responsive while it settles.** The first version
  paid inline, blocking up to forty seconds on propagation retries, so the polls
  went unanswered and the till abandoned a sale for a transaction that had
  already been broadcast. Settlement now runs off the loop.
- **`amount_authority` must be `open`**, and `offer_commit` is necessarily empty:
  the offer does not exist until the till makes one. The till refuses a
  payer-presented tap claiming otherwise.
- **The confirm screen does not move.** `ACCEPT` is still the payer's alone.
  Whoever held out a phone, the party whose money is at risk decides.

A UI written against one direction will assume symmetry and be wrong.

## Attacks

    ducat-harness --payee 300000000
    ducat-harness --attack

Every refusal in this protocol is unit-tested. None had ever been *sent*. That
gap is not academic: the `dest` bug was a check that existed, was tested, and
rejected every real payment — because the fixtures agreed with the mistake.

A hostile payer runs ten attacks against the same honest payee the other modes
use, unmodified. All ten refused, with the codes the spec names:

```
refused  accept_underpays                      price mismatch
refused  accept_overpays                       price mismatch
refused  accept_names_another_offer            ACCEPT names another offer
refused  accept_signed_by_a_stranger           BadSig
refused  accept_signed_in_another_context      BadSig      (§18.3 domain separation)
refused  accept_with_an_unknown_field          UnknownField (§18.8 strictness)
refused  txid_before_any_accept                no ACCEPT on file
refused  txid_for_another_transaction          CommitMismatch
refused  txid_announces_an_underpayment        PriceMismatch
refused  txid_for_a_transaction_that_does_not_exist   never observed
```

### The last one found a denial of service

The payee originally scanned **inside the request**: 30 attempts at 10-second
intervals. So a TXID naming a transaction that does not exist froze the terminal
for **five minutes, for the cost of 40 bytes** — and to the payer, a slow
confirmation and a fabricated TXID arrived as the same `Timeout`, pointing at the
network rather than at the payment.

`TXID` and `RECEIPT` are now two exchanges. Structural checks are cheap and
synchronous; the payee acknowledges immediately, scans off the session, and the
payer collects the receipt separately.

The general rule, which this is the third instance of: **nothing that waits on
the world may hold a session open.** It is the same failure as a server that dies
on malformed input — both turn a counterparty's message into an outage, and both
stay invisible until somebody hostile sends one.

## The paths where nobody co-signs

    ducat-harness --edges

§6.2 calls post-`FUND`/pre-`RECEIPT` the dangerous window — the payer's money is
gone and the co-signed record does not exist. Every other mode runs a flow where
both parties stay. These are the ones where somebody leaves.

The machinery is §6.2's **two unilateral receipts, which assert opposite things**:

- **Payment evidence** — the payer saying *"I paid and hold no co-signature."*
  Emitted when the payee vanishes after funding. 166 bytes, flagged `unilateral`.
  It proves what the payer signed and paid; it cannot prove delivery and does not
  claim to.
- **Debt evidence** — the payee saying *"you owe me and never stopped the
  meter."* Emitted when a payer walks out on a tab.

Conflating them has a merchant filing a payment it never received, or a payer
recording a debt it does not owe — which is why the state machine picks, and not
whoever is writing the UI.

Also confirmed here: `METERING` survives an hour of wall clock (a tab that died
after sixty seconds was a real bug, §18.4.1(8)); a payer cannot abort a live
meter while an operator can void one cleanly (§18.4.1(7)); and a refund is
refused when redirected — BIP-70's published hole — when larger than the payment,
when the payer signed no address, and when late.

**One finding came from the fixture, not the code:** `Terms::default()` grants a
**zero** refund window, so a client shipping default terms has silently made
every sale final. Defensible as a default, easy to ship without noticing. §7.3
now requires the window be shown on the confirm screen — "no refunds" is a term
of the sale, not the absence of one.

## Requirements

`monero-wallet-rpc` on ports 28101 (payer, `user_01`) and 28104 (payee,
`coffee_01`) — `monero-spike/` sets these up. `DUCAT_WAIT_SECS` bounds the wait
for Veilid readiness; `DUCAT_TAP` moves the tap file.
