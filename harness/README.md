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

## Requirements

`monero-wallet-rpc` on ports 28101 (payer, `user_01`) and 28104 (payee,
`coffee_01`) — `monero-spike/` sets these up. `DUCAT_WAIT_SECS` bounds the wait
for Veilid readiness; `DUCAT_TAP` moves the tap file.
