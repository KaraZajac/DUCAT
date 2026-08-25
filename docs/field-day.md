# The field day

Everything 1.0 still needs that only real hardware can give. One afternoon,
two phones, this laptop. Passes are ordered so the slow chain waits (Monero's
ten-block maturity, ~20 min on stagenet) overlap other work instead of
stalling it.

**Bring:** two Android phones (NFC-capable — that is the point), this laptop
on the same internet, and nothing else. Every pass runs on the live Veilid
network and stagenet; there is no lab setup to carry.

## Before leaving the desk

1. Install the release on both phones — phone browser:
   `https://github.com/KaraZajac/DUCAT/releases/latest/download/app-arm64-v8a-debug.apk`
2. Onboard both; **name them differently** (two contacts with the same
   display name has burned us — threads get opened on the wrong person).
3. Fund phone 1 **from the test bank** (below). It needs roughly 0.05 XMR to
   run every escrow pass with slack. Fund **first** — the ten-block unlock
   runs while you do passes 1–3.

### The test bank

One standing stagenet wallet, so funding never depends on finding a faucet
twice. It lives in `~/.ducat-stagenet-bank` — deliberately outside this
repository, because it holds a spend key and nothing that holds a spend key
belongs in git.

```sh
# Its address, to be given to whoever is sending coin. Creates the wallet on
# first run, at the chain's current tip rather than genesis.
DUCAT_DESK_STATE=~/.ducat-stagenet-bank ./gradlew :desktop:wallet

# What is actually in it (scans first; an idle wallet knows nothing).
DUCAT_WALLET_SCAN=1 DUCAT_DESK_STATE=~/.ducat-stagenet-bank \
  ./gradlew :desktop:wallet

# Top up a phone or a desk from it.
DUCAT_DESK_STATE=~/.ducat-stagenet-bank DUCAT_PAY_TO=<address> \
  DUCAT_PAY_XMR=0.02 ./gradlew :desktop:payout
```

`payout` works on any desk state, which is the other half of the point:
money left behind in an old role's directory can be swept back to the bank
instead of being abandoned. Two earlier runs had 0.0015 XMR stranded that
way before this existed.

It is stagenet, so the state is unencrypted on purpose — a headless top-up
should not need a passphrase, and the coin is worth nothing. Do not point
these tasks at a mainnet state.
4. Start the standing arbiter on the laptop and leave it running all day
   (its DKG machines are in-memory — restarting it mid-ceremony strands
   that ceremony):
   `DUCAT_DESK_STATE=/home/kara/ducat-arbiter ./gradlew :desktop:arbiter`
   Pair BOTH phones to it (`--args="--issue"` prints a card once), and on
   each phone flip the arbiter contact's **Escrow-arbiter** switch.
5. On each phone, open the other's profile and confirm a payto address is
   published (settlement pays the driver only at the address they
   published; unpublished = the propose button errors).

Record as you go: pass, result, txids, and the moment anything surprises
you. The surprise is the data.

## Pass 1 — NFC (never once tested on hardware)

The §15 core gesture. Compile-verified only; assume nothing.

- **Tap-to-contact:** both phones on the contact-exchange screen, back to
  back. Expect the HCE card to cross and a thread to open both ways.
  Record which orientation worked and how many attempts.
- **Tap-to-pay:** phone 2 shows a payment request, phone 1 taps. The §15.5
  confirm screen MUST appear — a tap must never move money by itself. If
  it pays without the confirm, that is a release-blocking bug; stop and
  write down everything.
- **NDEF sticker** (if one was written): tap it, expect the `ducat:` link
  to open the claim flow.
- Record tap-to-read latency by feel (instant / a beat / retries). §8.7.2's
  numbers are a desktop's; these are the real ones.

## Pass 2 — dispatch, phone to phone, no harness

Real GPS at last (the emulator's `geo fix` lied to us; a real phone won't).

1. Phone 1 (rider): hail from where you stand — destination a few blocks
   away, check the quoted fare against the route.
2. Phone 2 (driver): Drive mode, watch the live map, find the notice,
   read the job card (pickup distance, trip, payout), claim it.
3. Rider sees the acceptance with the driver's face/car/plate; driver
   drives (walk it), meter runs, geofenced bill fires on arrival.
4. Pay with tip. Receipt lands on both. Record every message that needed
   a retry — on emulators the boards were slow; real-network numbers are
   wanted here.

## Pass 3 — bonded hail, 2-of-3 (re-prove on hardware)

Proven on emulators end to end; the hardware run should be boring. Hail →
accept (arbiter set) → banner builds the escrow → rider funds → both
sides flip to "fare secured" **by their own scan** → driver Complete →
rider consent tap → paid. If funding is younger than ten blocks the
release refuses with "the fare needs N more confirmation(s)" — that is
maturity, not failure; wait and retry with the banner's retry.

## Pass 4 — 2-of-2 mutual stakes (proven off-hardware; re-run here)

Done end to end on 2026-08-25 between two emulators on the live Veilid
network and live stagenet — hail, offer, accept, 2-of-2 built, both stakes
and the fare in, complete, release (`112e0983`). So this is no longer the
first live pass; what it is here is the same flow on two radios, two
batteries and two real clocks, which is the part an emulator cannot answer.

Turn the Escrow-arbiter switch OFF on both phones' contact profiles first.

1. Hail, accept: banner should build a 2-of-2 and quote the rider
   fare + fare/5 (the margin — both sides hostage, honestly).
2. Fund, wait secured, drive, Complete: default release splits margin →
   rider's refund address, fare − fee → driver.
3. Verify the numbers on both banners match, then on chain.

## Pass 5 — settlement (proven off-hardware, counter included; re-run here)

Propose-and-sign ran three times on 2026-08-25 off-hardware — a ride, a
marketplace sale and a two-day gear hire, each proposed on one client and
signed on the other (`112e0983`, `284eb311`, `709f4d38`). **The counter ran
the same day** and was worth every minute: the rider countered, the driver
signed, and the money was right both times — while three separate sentences
were wrong, because a counter swaps the roles and nothing that named them
swapped with it (`4c9a027d`, then `74bc40b9` on the fixed build).

One thing here is still untested anywhere: a counter to a **counter**. Both
passes ended on the first one.

On a fresh 2-of-2 ride (or the same one before releasing):

1. Driver proposes a partial refund (say a third back to the rider).
2. Rider's banner must state the exact split with a **Sign** and a
   **Counter** field. Counter with a different number.
3. Driver's banner now states the counter (a fresh proposal supersedes —
   whoever signs ends it). Sign it. Verify on chain both slices.

## Pass 6 — the ruling (desk arbiter UI pass)

On a 2-of-3 ride, complete it but have the rider "vanish" (pocket the
phone). Driver taps **Ask the arbiter to rule**. On the laptop the console
prints `ARBITER_RULING_REQUESTED <id> riderBack=…`; a human types the
judgment: `echo 'approve <id8>' >> /home/kara/ducat-arbiter/rulings.txt`.
Driver's banner completes the release without the rider. This is the
lost-phone story working in front of you.

## Pass 7 — the reservation (proven off-hardware; re-run here)

Ran twice on 2026-08-25 between two emulators: a marketplace sale
(`284eb311`) and a two-day gear hire (`709f4d38`), both through
propose → accept-is-funding → both secured → checkout split. The steps below
are unchanged; what hardware adds is two independent clocks and two radios.

1. Phone 1 (guest): chat tray → the Lock icon → rent + both deposits.
2. Phone 2 (host): banner shows the terms; **accepting IS funding** their
   deposit — one button, no separate agree step.
3. Both flip to secured only when their own scan sees rent + both
   deposits. Checkout: guest deposit comes home, rent + host deposit −
   fee to the host. Verify all three numbers on chain.

## Pass 8 — battery (runs all afternoon by itself)

Note both phones' battery % when you leave the desk and each hour after.
The poller backgrounds to ~20 sweeps/hour; the claim to verify is that an
idle pocketed phone is not visibly warmer or hungrier than its neighbors.
Screen-on time will dominate — note it so the number is honest.

## What the field day unblocks

- **Live position after the accept** (§15.12): spec'd and waiting for a
  real ride on real GPS to build against.
- True §8.7.2 latency figures from a handset.
- The README's "what is not proven" paragraph loses four entries.

## Known traps, so they don't cost daylight

- **Ten-block maturity** presents as a refusal with a countdown, by
  design. Stagenet blocks are slow (~4 min, sometimes much worse). Fund
  early, do other passes during the wait.
- **The arbiter process must stay up** across any ceremony it is part of.
- **Release/broadcast can fail transiently** ("no relay took the
  release") — the banner's retry re-proposes with fresh nonces; it is
  safe to mash.
- **Same-name contacts** open wrong threads. Name the phones differently
  at onboarding.
- If a ceremony wedges (a phone died mid-DKG), abandon it and start a
  fresh ride — ceremonies are cheap; debugging one in the field is not.
