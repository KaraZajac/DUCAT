# The field day

Everything 1.0 still needs that only real hardware can give. One afternoon,
two phones, this laptop. Passes are ordered so the slow chain waits (Monero's
ten-block maturity, ~20 min on stagenet) overlap other work instead of
stalling it.

**Bring:** two Android phones (NFC-capable — that is the point), this laptop
on the same internet, and nothing else. Every pass runs on the live Veilid
network and stagenet; there is no lab setup to carry.

## Before leaving the desk

1. **Cut a fresh release first** (`./release.sh` from master) — the latest
   tag predates the draft this document describes, and a phone one draft
   behind cannot read the newer boards at all (the stamp fields are unknown
   to its strict reader; both directions refuse by design). Note that A2 is
   still open: what `release.sh` publishes is the **debug** build, signed
   with a debug key, and the signing question is unsettled — so do not
   re-tag on the strength of this list alone. Then install on both phones
   — phone browser:
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
   `DUCAT_DESK_STATE=/home/kara/ducat-arbiter ./gradlew :desktop:arbiter --args="--name Marta"`
   (the name rides its card; an arbiter that says who it is gets flipped
   on with more confidence)
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
- **The card is served only while unlocked** (A3, since 2026-09-14). With
  phone 1's screen locked, bring phone 2's reader to it: nothing should
  cross. Unlock and retry. A pocketed phone that still hands over its
  standing card is a release-blocking bug.
- **The spend gate** (§15.5.1, since 2026-09-15). Three things to try, in
  this order, because each is a different rung:
  1. A small payment on a phone unlocked seconds ago — **no PIN**. That is
     the whole point of the rung; a gate on every coffee is a gate people
     learn to tap through.
  2. Lock the phone, wait past two minutes, unlock, wait again *without*
     unlocking, then pay — the PIN should be asked. The window is a
     keystore key bound to a recent authentication, so this is the one
     rung that cannot be checked on an emulator with a swipe lock.
  3. A payment over the limit in drawer → Profile → Spending — the PIN,
     **every time**, including twice in a row. Set the limit low (say 1)
     so this does not need real money.
  A phone with no secure lock screen should ask for the PIN on all three.
- **NDEF sticker** (if one was written): tap it, expect the `ducat:` link
  to open the claim flow.
- Record tap-to-read latency by feel (instant / a beat / retries). §8.7.2's
  numbers are a desktop's; these are the real ones.

## Pass 2 — dispatch, phone to phone, no harness

Real GPS at last (the emulator's `geo fix` lied to us; a real phone won't).

1. Phone 1 (rider): hail from where you stand — destination a few blocks
   away, check the quoted fare against the route. Since 0.89 the post is
   **stamped against a recent Monero block**: it needs the node reachable
   (a clear sentence says so if not), and the stamp itself is a second or
   two of mining — an unlucky draw is ten; the button holds a spinner, so
   a pause here is the search, not a hang.
2. Phone 2 (driver): Drive mode, watch the live map, find the notice,
   read the job card (pickup distance, trip, payout), claim it.
   **The notice must carry no name** (§16.17, since 2026-09-15) — the
   board says an area, two cells, a fare and a persona, and nothing else.
   A rider's name visible to a driver who has not claimed is a
   release-blocking bug; so is one visible on the *board* to a third phone
   reading the same cell, which is worth a look if a third handset is
   about.
3. On the claim, three things should arrive in the rider's thread, in
   order: an **introduction** (the rider's name and picture — the driver's
   contact reads "Unnamed contact" until it lands, which is correct, not a
   fault), then the precise pickup, then the precise destination. Proven
   desk to desk on 2026-09-15; this is its first run on handsets. Check
   the driver's contact list afterwards: the rider should be named there.
4. Rider sees the acceptance with the driver's face/car/plate; driver
   drives (walk it), meter runs, geofenced bill fires on arrival.
   **Tap "Share my position" on both** once the ride is accepted — it
   only appears after the accept, by design, and it runs while the chat
   screen is open. Leave one phone's screen on and pocket the other; the
   watching side should say "last seen N seconds ago" rather than draw a
   dot that keeps moving.
5. Pay with tip. Receipt lands on both. Record every message that needed
   a retry — on emulators the boards were slow; real-network numbers are
   wanted here.

## Pass 3 — bonded hail, 2-of-3 (re-prove on hardware)

Proven on emulators end to end; the hardware run should be boring. Hail →
accept (arbiter set) → banner builds the escrow → rider funds → both
sides flip to "fare secured" **by their own scan** → driver Complete →
rider consent tap → paid. If funding is younger than ten blocks the
release refuses with "the fare needs N more confirmation(s)" — that is
maturity, not failure, and since 0.89 the phone retries it **by itself,
pocketed included**, for the hour the banner's sentence is worth. Pocket
the phone and check back; pressing the banner's retry only hurries it.

## Pass 4 — 2-of-2 mutual stakes (proven off-hardware; re-run here)

Done end to end on 2026-08-25 between two emulators on the live Veilid
network and live stagenet — hail, offer, accept, 2-of-2 built, both stakes
and the fare in, complete, release (`112e0983`), and again on 2026-08-26
on the 0.89 build with live position riding along (`3382f6e3`). That
second run also proved the maturity retry **pocketed**: Complete refused
with "needs 5 more confirmations", both phones went to the home screen,
and the release proposed itself ten minutes later with nobody looking.
So this is no longer the first live pass; what it is here is the same flow on two radios, two
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

The counter to a counter ran too (`ba8f17f6`), and cost two more wrong
sentences to find. What is still untested is the whole of it on hardware.

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

## Pass 8 — costly identity on a handset (§9.5)

The trust layer has been walked desk-to-desk and once on a phone; what no
emulator gives it is a handset's own view of the chain. It needs a reachable
Monero node for the burn and a *second* one for the block, which is exactly
the field condition this sheet exists for.

1. Phone 1: drawer → money screen → **Burn**. Spend the floor (0.01 XMR).
   The proof is made at send time and the envelope is signed later, on the
   sweep that first sees the block — so expect a gap of minutes, and expect
   the screen to say which of the two it is waiting on.
2. Phone 1 → phone 2: **Show my burn** in the thread. Phone 2 taps **Check**.
   It must check the proof against its *own* node for exactly the proven
   amount, and get the block from a different node. Record how long, and
   what it says when the second node cannot be reached — "not yet" is the
   only correct answer there, never "yes".
3. After any settled deal on the day: **Rate them**, then **Show my record**
   the other way. A record from somebody whose burn phone 2 has not checked
   must read as *0 weighted*, not as a number.
4. **I know them** on both phones, then a third party asks: the answer is
   words ("Pat knows them"), counted only from contacts that phone already
   holds. If it ever draws a score, stop and write it down.

## Pass 9 — the phone swap (restore on real hardware; run LAST, it wipes)

Proven twice on emulators (2026-08-26), shop and till included — what
hardware adds is the OEM's own file picker and share sheet, which is where
a restore meets the real world. **Run it at the end of the day**: it wipes
a phone, and every pass after it would pay the fresh node's attach.

1. Phone 2: Settings → Backup → passphrase → Export; save the `.ducatbak`
   somewhere findable (Drive/Files — note where the OEM sheet puts it).
2. Clear DUCAT's data (or uninstall/reinstall).
3. Onboard → "I already have a backup" → pick the file → passphrase. The
   confirm screen shows the restored wallet address — check it.
4. Expect back: name, contacts, threads (attachments re-fetch), **your
   listings and till items** (new in 0.89), and the balance after a
   rescan (~20 min stagenet; the note says it is partial until done).
   Also **your spending limit** (drawer → Profile → Spending, new
   2026-09-15): set it to something unusual before the export and check
   the number that comes back is yours and not the default. This is the
   failure that hides — the default is *stricter*, so a restore that
   dropped it would look like nothing until a payment that never used to
   ask for a PIN asks for one, days later, with nothing on any screen
   connecting the two.
5. Messaging resumes when the fresh Veilid identity attaches. If it sits
   at "Attaching" for more than ~10 min, **cycle airplane mode** — that
   re-runs network detection, which an app restart does not, and took an
   emulator from zero to 55 peers in a minute.

## Pass 10 — battery (runs all afternoon by itself)

Note both phones' battery % when you leave the desk and each hour after.
The poller backgrounds to ~20 sweeps/hour; the claim to verify is that an
idle pocketed phone is not visibly warmer or hungrier than its neighbors.
Screen-on time will dominate — note it so the number is honest.

## What the field day unblocks

- **A position that actually moves** (§15.12). The stream itself is built
  and proven between two emulators on 2026-08-26 — offered, read,
  rendered, aged honestly when the sender left the screen, released when
  the sender stopped, and swept off both phones by the poller when the
  ride settled. What no emulator can judge is a *moving* dot: `adb emu
  geo fix` reports OK and changes nothing, so both phones shared one
  frozen fix all afternoon. Share in both directions during Pass 2 and
  watch whether the other phone's dot tracks the walk, whether four
  seconds feels like the right cadence on a real radio, and how long a
  fix takes indoors.
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
- **A ceremony that stalls now heals itself** (0.89): a lost round is
  re-sent every three minutes, so give a quiet build five minutes before
  judging it. What still strands one is a phone *dying* mid-DKG — the
  machine is in-memory — and for that the building banner now has **Call
  it off**; press it and start a fresh ride rather than debugging in the
  field.
- **Posting needs the Monero node; reading does not.** A phone that
  cannot reach a node cannot put a hail or listing up (it says so), but
  boards still read — marked with a small "could not be checked against
  the chain" note. That note appearing on a healthy afternoon means the
  node picker is struggling, not that the board is compromised.
- **A freshly onboarded or restored phone has no exchange rate yet** —
  fiat-priced posting sits disabled until the first rate fetch, a minute
  or so after the network attaches. It looks like a dead button and is a
  cold start.
- If read receipts are wanted for the day, flip **Send read receipts** on
  both phones at setup — ✓ is delivered, ✓✓ is read, and the second tick
  is the §16.16 watermark doing its job across the DHT.
- **A card cut for a board carries no name** (§16.17, 2026-09-15). That is
  a hail's card today. It means a newly claimed hail thread shows
  "Unnamed contact" on the driver's phone for a few seconds until the
  rider's introduction lands — which looks exactly like a bug and is the
  feature. If it never becomes a name, *that* is the bug.
- **The spend gate can look like a regression.** Payments under the
  limit no longer ask for the PIN (§15.5.1, 2026-09-15); that is the
  intended shape, not a gate that stopped working. Above the limit it
  asks every time, including twice in a row for the same amount.
