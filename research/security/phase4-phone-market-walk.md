# Phase 4 on a real Android build — the seller's minimum, and the Pay gate

*2026-09-14, 20:02–22:41 UTC, in two windows (the first was cut short; the
second re-proved every screen live). The last two §9.5 behaviours from
[phase4-phone-minimum.md](phase4-phone-minimum.md)'s "what is left" — a
seller's minimum shown on a found listing with the buyer's shortfall warning,
and the gate under Pay — walked on the x86_64 debug APK on the `ducat` AVD
against a headless desk from `app/examples/mailbox.rs` over the live network
and the live stagenet chain. Nothing was committed; no source was edited. Times
are UTC; the phone's clock shows them as 4:xx–6:xx PM.*

**Result: both behaviours pass on the screen, and the payment completes — but
not in the shape the brief expected.** The phone found the room, read "The
seller asks that buyers have burned at least 0.010000 XMR" and "This persona
has burned nothing yet. They may not deal with you." with Ask still enabled;
asked, and the desk answered. Under Pay, with 0.005 XMR typed, the gate said
"They have burned nothing you have checked. If they vanish, nothing holds
them." — and **Send was dark, for a reason that has nothing to do with the
gate**: a listing's card never carries a payment address, so the enquiry thread
cannot pay the seller at all until the seller bills. Once the desk sent a bill
(the same desk state, run in the exercise's `host` mode, which answers "bill
me"), the gate sentence stayed, Send lit, and 0.005 XMR went through on the
first try: `e5051077…` in stagenet block 2,207,762, the desk's own log
`received payment note seq 2 from Market Walk` and `Notify: Market Walk —
Payment · 0.005000 XMR`.

Six items below: four findings — three in `app/examples/mailbox.rs`'s new
`lister` arm (cc18ad75), one in the road that arm assumes — and two
observations. None is in the §9.5 screens themselves.

## The clock

| UTC | What |
|---|---|
| 20:01 | emulator relaunched inside a rootless `pasta` namespace (pid 34029, still up) |
| 20:02:10 | fresh install of `app-x86_64-debug.apk` — `1.1.0.1062-1026a261` |
| 20:02:41 → 20:02:53 | app first start → `transport AttachedFull — 93 peer(s)` (12 s) |
| 20:02–20:07 | onboarding to Home, name **Market Walk**, PIN set 20:06:19, backup 329 B 20:07 |
| 20:03:46 | first board search: `search near u33dc: 0 listing(s) from 9 board(s) in 52s` |
| 20:06:19 → 20:09:09 | bank `wallet sync` — spendable 0.011777 XMR at tip 2207695 |
| 20:10:52 → 20:11:23 | bank sends **0.010 XMR** to the phone (`7d03e655…`, fee 0.000176, accepted by 3) |
| 20:12:25 | phone: `received 0.010000 XMR at block 2207698` |
| 20:03 / 20:16 | lister desk fails twice — see findings 1 and 2 |
| 20:18:33 → 20:21:37 | patched lister up (51 peers), `MB_LISTED … in u33dc` on `local:u33dc@2958/0` |
| 20:23:10 | phone: `search near u33dc: 1 listing(s) from 9 board(s) in 45s` |
| 20:23:59 → 20:24:02 | "Ask about it" → card claimed, hello sealed and delivered |
| 20:26:10 / 20:26:12 | desk: `card (rental) answered by Market Walk` / the hello arrives (2 min 11 s after Ask) |
| 20:26:54 | desk's hello lands on the phone; `MB_CLAIM_SEEN by Market Walk; said hello` |
| 20:29 | Pay opened, 0.005 typed: the gate sentence renders, **Send dark**, "Up to 0.000000 XMR after fees" (funds still locking) |
| *(session interrupted; the lister ran out its hour at 21:21 and exited `MB_OK lister stayed up an hour`)* | |
| 22:21:08 → 22:22:36 | lister restarted on the same state dir; a second notice posted to `local:u33dc@2958/1` |
| 22:22:47 | desk says hello again in the standing thread; on the phone at 6:22 PM |
| 22:26:37 | phone: `search near u33dc: 2 listing(s) from 9 board(s) in 35s` |
| 22:27:30 | listing opened, **read live** (below); "Ask about it" tapped |
| 22:28:33 | Pay, 0.005 XMR typed: gate sentence present, Send dark, address hint |
| 22:28:57 / 22:29:14 | lister killed by pid / same state dir restarted in `host` mode (224 peers) |
| 22:29:46 → 22:29:55 | phone sends "bill me" → desk answers with a bill (kind 1, `payto`) in 9 s |
| 22:29:57 | bill on the phone (2 s later — the thread was watched); full-screen "asks you for" prompt |
| 22:33:08 | Pay reopened, 0.005 XMR: **gate sentence still there, Send enabled** |
| 22:33:47 / 22:33:56 | PIN 2468 approved / `sending 0.005000 XMR using 1 note(s) to 5AH9gkCDUTpL…` |
| 22:34:05 | `sent e5051077a0b1894e… fee 0.000121 XMR, accepted by 3 node(s)` — 9 s |
| 22:34:08 | payment note seq 2 delivered; the phone draws "You sent · 0.005000 XMR · Payment ✓" |
| 22:38:18 | desk: `received payment note seq 2 from Market Walk`, `Notify: Payment · 0.005000 XMR`, `MB_GOT … kind 2 'Payment'` (4 min 10 s after delivery) |
| 22:36 | the phone's own row: **2 of 10 confirmations**; a public node puts it in block 2,207,762 |
| 22:41:34 | desk killed by pid; emulator left running |

## What was running

- **Phone:** `ducat` AVD (x86_64) inside the rootless `pasta` namespace of
  [[emulator-rootless-tap]] — qemu pid 34029, adb only via `nsenter … -t 34029`
  (`walk2/adbn`). Status page at 22:35, live: **attached AttachedFull ·
  route-capable yes · peers 168 live, 165 reliable · routing table 167
  answering, 42 not, of 210 known**, node `VLD0:KLX5rvjNDHu3sgc0CXwDuKprIZqLc7SxqDz7hEqJnNU`;
  Monero `http://node2.monerodevs.org:38089`, height 2,207,762, synced, 180 ms.
  Wallet `54qXNWdJhoCfjikdZ77GSwfQ2h6duL2XM76WTgcYgXzYCEqU8uFQgPCHEyiPE7tra3YCK6wGe7aebMbfH4DWnoRnJGLkAMw`.
- **Desk:** `walk2/lister-bin/lister2` — a scratchpad copy of
  `app/examples/mailbox.rs` with the `lister` arm patched (findings 1–2), state
  `walk2/lister`, persona `1bfe87994d2d…`, wallet
  `5AH9gkCDUTpLjYKDciudjC3yDd3JXYDQ6SihS48F5MqC6sVJmFKJo4pAMuKZSjdY1D1VCLYma83vqHwZimvZpSQZDTWLqXT`.
  The room: "A spare room, quiet street", 0.05 XMR/night, stake 0.01 XMR,
  1 bedroom / sleeps 2, cell `u33dc`, **min_burn 10000000000 pXMR = 0.01 XMR**.
- **The cell:** no location was injected. The phone's own last fix already put
  it in `u33dc`, and the desk was pointed at that cell instead — the fallback
  the location note recommends ("point the *other* side at the phone's actual
  location"). Nine boards each search, 35–52 s a sweep.

### The build under test, and why it was not the newest APK

The installed APK is **`1.1.0.1062-1026a261`**. The hash names HEAD at build
time; the phone half of the seller's-minimum work was still uncommitted then,
so the *content* under test is what
[phase4-phone-minimum.md](phase4-phone-minimum.md) describes and what later
landed as `37626de1` — proven by the screens themselves, since
`rent_min_burn_x`, `rent_min_burn_none` and `trust_no_burn_known` exist in no
earlier commit and all three render.

The APK now on disk is `1.1.0.1065-3b2267d9`, rebuilt at 20:34 by a concurrent
session. **It was deliberately not installed.** A fresh install wipes the
wallet, and the bank could not refund it: it held 0.011777 XMR and 0.010176 of
that went to this phone, leaving ~0.0016. Of the four commits between the two,
only `3b2267d9` touches the phone at all (backup passphrase, `Home.kt`,
`Onboarding.kt`); none touch the market, the listing sheet or Pay. One
consequence is visible: this build still accepts an eight-character backup
passphrase (see step 1).

## Step 1 — a fresh, unburned buyer: PASS

`App|started — v1.1.0.1062-1026a261, sdk_gphone64_x86_64, Android 14` at
20:02:41, `transport Detached — 0 peer(s)` → `transport AttachedFull — 93
peer(s)` **twelve seconds later** (the guest keeps the tap from the earlier
launch; on plain SLIRP this never happens). Onboarding ran to Home with name
**Market Walk**, `DucatPin|a PIN is set on this device` at 20:06:19, and a
329-byte `files/backups/ducat-backup.ducatbak` at 20:07. The phone was funded
at 20:12:25 and **never burned**: the Accounts tab still carries the "Costly
identity" card, and in the thread tray **"Show my burn" and "Show my record"
are both `enabled=false`** — exactly the buyer §9.5 is about.

Two honest deviations from the brief here, both from the first window:

- **The backup passphrase was "walkwalk", not "correct horse battery staple
  ocean".** Verified after the fact by importing the phone's own bundle into a
  throwaway desk state with `app/examples/backup.rs`: the brief's phrase and
  two variants give `Refused("BadSig")`; `walkwalk` gives `BK_OK restored 0
  contact(s), 1 persona(s), wallet from height 2207693; name "Market Walk",
  address 54qXNWdJhoCf…`. So the bundle is this phone's, and **this build's
  export still accepts a Weak passphrase** — W11 of `3b2267d9` is not in it.
  The brief's note that "the export now refuses a weak passphrase" is true of
  the tree, not of the binary that was walked.
- The PIN is 2468 as asked (it opened the spend below on the first try).

## Step 2 — the seller's minimum on a found listing: PASS

Renting → Find a place → "Boards are about five kilometres across, and this
looks at yours and the ring around it." → `Places 2` after 35 s. The row itself
carries title, price, stake and specs and **not** the minimum — as the desk's
row does not either. Opened (22:27:30, `w2-02-listing-sheet.png`), the sheet
reads, in order:

> Photos are coming from the seller's phone.
> **⁨A spare room, quiet street⁩**
> **No burn of theirs checked here**
> USD 25.67 / night
> stake USD 5.13 each
> **The seller asks that buyers have burned at least USD 5.13**
> **This persona has burned nothing yet. They may not deal with you.**  *(error colour)*
> Bedrooms 1 · Sleeps 2 · ⁨north side⁩
> Asking opens a conversation with the seller.
> **[ Ask about it ]**

The button's own node reports `clickable=true enabled=true` — read off the
clickable parent, not the text node ([[uiautomator-reading-compose]]).
**Warn, never refuse: confirmed.**

The figures read in USD because the price preference is on. Turning "Show what
a balance is worth" off and reopening the sheet (`w2-11-listing-xmr.png`) gives
the brief's exact sentence:

> 0.050000 XMR / night
> stake 0.010000 XMR each
> **The seller asks that buyers have burned at least 0.010000 XMR**
> This persona has burned nothing yet. They may not deal with you.

— the desk's `min_burn=10000000000 pXMR`, to the piconero. The preference was
put back afterwards. This is deviation 3 of the minimum memo behaving as
designed (`Amounts.show(…).primary`), and the same cosmetic wrinkle the burn
screen has: the *seller* types a burn floor in XMR and the *buyer* may read it
in dollars.

"Ask about it" opened the enquiry thread on the same contact (the listing's
card belongs to the same desk persona, so the second window's claim reused the
standing thread rather than opening a second one — the desk still lists one
"Market Walk"), sent "Hello — I saw your listing, A spare room, quiet street.
Is it available?" with a delivered ✓ at 6:27 PM, and the desk answered in
both windows: `MB_CLAIM_SEEN by Market Walk; said hello`, then "Hello — the
room is free this week. Ask me anything." on the phone. The thread header says
**Lister Desk** with no badge line under it — nothing of the desk's has been
verified here, and the phone says so by saying nothing.

Claim → desk in the first window: **2 min 11 s** (20:23:59 → 20:26:10), on a
desk polling `collect_claims` every 10 s.

## Step 3 — the Pay gate: PASS on the gate; the send needed an address first

### 3a. The gate

Thread → "+" → the **Money** icon → unit XMR → `0.005`. Under the amount
(`w2-03-pay-gate.png`, 22:28:33):

> 0.005000 XMR
> **They have burned nothing you have checked. If they vanish, nothing holds them.**  *(error colour)*
> Up to 0.009882 XMR after fees · Max
> Amount USD 2.57 · Network fee (estimated) USD 0.06 · Total USD 2.63 · Left after USD 2.51
> Uses 1 note(s) · usually confirmed in about 6 minutes
> Converted at 1 XMR = USD 513.40 · CoinGecko + CoinPaprika

That is `trust_gate_no_burn`, verbatim, exactly where the brief says it should
be. **It is the whole of the gate: nothing else on the screen changes because
of it.**

### 3b. Send was dark, and the gate was not why

With the gate showing, the big Send button read `enabled=false`, and beneath it:

> ⁨Lister Desk⁩ has not shared an address, so you can ask but not send yet. Switch to Request — it carries one back.

The rule is `applications/android/.../ui/Pay.kt:1096` — `payable = target !is
ToContact || contact.theirAddress != null || prefillAmountPxmr > 0`. `their_address` is
learned from a card's `payto` or from an incoming message's `payto`
(`app/src/mailbox.rs:1766`), and **a card never carries one**: the issuer
passes `None` at `app/src/mailbox.rs:368`, commented "§16.12 makes publishing
an address a choice; the desk has no wallet yet, so the choice is not
offered". A listing's card is
no exception. So the enquiry thread a minimum invites you into cannot pay the
seller until the seller says something with an address in it. See finding 4.

### 3c. The payment

The same desk state was restarted in the exercise's own `host` mode — the only
committed mode that shares an address: it answers the text "bill me" with a
kind-1 bill carrying `payto`. Phone → "bill me" (22:29:46) → desk `sending bill
seq 1` (22:29:55) → on the phone 2 s later, as a full-screen prompt:

> ⁨Lister Desk⁩ **asks you for** USD 0.05 / 0.000100 XMR / "⁨A small bill⁩" / A thing USD 0.05
> [ Accept & pay ] [ Decline ]
> Accept opens the confirm screen — nothing moves until you approve it there.

That prompt was closed unpaid; Pay was reopened from the tray and `0.005` typed
again (`w2-05-pay-gate-enabled.png`, 22:33:08). **The gate sentence is still
there, word for word, and Send is now `enabled=true`.** Warn, never refuse —
proven on the screen where money moves.

Send → the confirm sheet: "Send USD 2.57? / 0.005000 XMR / To ⁨Lister Desk⁩ /
5AH9gkCDUTpL… / Plus about USD 0.06 in fees — USD 2.63 in total. / If the built
fee comes back more than a quarter above this, nothing is sent. / Monero
payments cannot be reversed or cancelled. Check the address — there is nobody
to appeal to if it is wrong." → **"Enter your PIN — Money is about to leave
this phone."** → 2468 → Approve (22:33:47).

```
1789425236660|I|DucatWallet|sending 0.005000 XMR using 1 note(s) to 5AH9gkCDUTpL…
1789425245079|I|DucatWallet|sent e5051077a0b1894e… fee 0.000121 XMR, accepted by 3 node(s)
1789425245535|I|DucatMailbox|sending payment note seq 2 to Lister Desk
1789425248622|I|DucatMailbox|delivered seq 2 to Lister Desk
```

PIN → accepted took **9 s** (the previous walk's burn took 45 s; the decoy
download was warm). The phone drew "USD 2.57 / Paid · ⁨Lister Desk⁩ ✓", then the
thread bubble **"You sent / USD 2.57 / 0.005000 XMR / ⁨Payment⁩ ✓ 6:34 PM"**,
and Home's Recent row "To Lister Desk · −USD 2.57". The tapped row
(`w2-12-tx-detail.png`) reads "2 of 10 confirmations", address `5AH9gkCDUTpL…`,
fee 0.000121 XMR, change back 0.004878 XMR, transaction
`e5051077a0b1894ee5d66c1eac5a1e0c9654c467a5f583c9a271e887a99ec7a3` — which a
public stagenet node independently puts in **block 2,207,762, not in the pool**.

The desk saw it 4 min 10 s later:

```
1789425498759|I|Mailbox|received payment note seq 2 from Market Walk
1789425498769|I|Notify|Market Walk — Payment · 0.005000 XMR
1789425498785|I|Mailbox|read 1 log(s) in 5376 ms, 0 waiting
MB_GOT Market Walk seq 2 kind 2 'Payment' fs=true re=None/false att=-
```

**No `MB_RECEIPT`, and no receipt bubble on the phone** — finding 3. The
incoming bill in the thread now shows "Paid ✓": that is `billPaid`'s documented
rule (an outgoing payment of at least the billed amount, later, settles an
unreferenced incoming bill on the payer's copy, erring toward "paid" so a spend
button cannot be tapped twice), not a mis-attribution.

## Findings

1. **`mailbox lister` cannot post a listing at all (real, certain).**
   `MB_FAIL the board refused the listing`, `walk2/lister-run1-broken.log`.
   `draft_listing` (`app/src/listings.rs:532`) *returns* a `Listing` and never
   stores it; `post_locked` does `let Some(l) = self.listing(id) else { return
   Ok(false) }`. The desk's own road is draft → `put_draft` → `post_listing`;
   the exercise skips the middle. One line — `app.put_draft(l.clone())?` before
   the post — fixes it. In `cc18ad75`, so the exercise has never posted.
2. **`mailbox lister` posts before the wallet has a tip (real, certain).**
   With 1 fixed: `MB_FAIL post: no recent Monero block to stamp a notice
   against — the wallet's node has not answered yet`
   (`walk2/lister-run2-nostamp.log`). `ready()` waits for the *Veilid* node
   only; the notice's stamp needs a Monero tip, which arrives on the first
   wallet lap. The scratchpad copy retries the post with a lap between, and
   MB_LISTED came on the second attempt, ~1 s after
   `Wallet|picked node http://node3.monerodevs.org:38089 at height 2207702`.
   Post-1.0 shape: `post_listing` could return a distinguishable "not yet"
   rather than an error string, since a phone hits the same window.
3. **`MB_RECEIPT` can never print (real, certain).** The arm scans for
   `!outgoing && kind == 3`. A phone paying a contact sends **kind 2**
   (`Pay.kt:1225`); kind 3 is the *payee's* receipt. Unless the exercise
   answers a payment with a receipt of its own, "prints MB_RECEIPT if paid" is
   unreachable — as it was here, with the payment demonstrably delivered.
   The same gap is why the phone shows no receipt bubble: nothing in the desk
   exercises receipts an unprompted payment.
4. **A listing's thread cannot pay its seller (real; a product question, not a
   test artefact).** §9.5 lets a seller demand a burn of buyers, and §16.18
   puts that minimum on the notice — but the card the notice carries has
   `payto: None` by construction, so the buyer who obeys the minimum arrives in
   a thread whose Pay button is dark and whose only advice is "Switch to
   Request". Nothing is broken in the code; the two features do not meet. The
   deal is presumably meant to go through a reservation (the sheet's "Propose a
   reservation") or a bill from the seller, and that may be the whole answer —
   but a buyer who taps Money first is told the seller has not shared an
   address, which reads like the seller's fault. Worth a decision before 1.0:
   either a rental card carries the address, or the Pay screen in an enquiry
   thread says "ask them for a bill" instead of "switch to Request".
5. **No badge and no gate on the two screens where the money is actually
   agreed (observation).** The full-screen bill prompt ("asks you for …
   Accept & pay") and the send confirm sheet both say nothing about the
   counterparty's burn — the gate lives only on the amount screen behind them.
   §9.5 asks for the badge "wherever a decision is made"; `Accept & pay` is
   one. Low severity (the gate is one screen back on the path this walk took),
   but a bill notification tapped from outside the app reaches the prompt
   without passing the gate.
6. **Cosmetic, already known:** with prices on, a seller's XMR minimum is read
   back to the buyer in dollars ("at least USD 5.13"), the same wrinkle the
   burn screen has. The listing sheet opened from the older of the two board
   notices showed no description line while the newer one did — its gallery
   bundle had been fetched and the older one's had not; not chased.

Not bugs, recorded so the next walk does not chase them: a desk lap of 38 s
when a listing is being refreshed; `collect_claims` on a desk holding several
rotated cards making laps minutes long (the 4 min 10 s for the payment note);
two notices for one room after the lister restart, which is the scratchpad
binary predating its own reuse patch, not a store problem — `ducat_listings.json`
holds both rows with `board: local:u33dc@2958`, subkeys 0 and 1, and each with
`minBurnPxmr: 10000000000`, the serde name the minimum memo specifies;
and Home reading "Ready to spend USD 0.00" right after the payment, which is
the change locking for ten blocks, not a lost balance.

## Against §8 / §9.5

- **A seller MAY publish a minimum; a buyer's client MUST show it before the
  buyer commits** — shown, on the opened listing, in the buyer's own unit,
  with the shortfall warning under it and Ask still enabled. Live, end to end,
  from a desk that wrote field 320 to a phone that read it.
- **The gate under Pay warns and never refuses** — the sentence renders under
  the typed amount, and with an address in hand the same screen sent
  0.005 XMR to a counterparty it had verified nothing about.
- **Still open from the minimum memo:** the poster's badge is the none-line
  until a listing can be tied to a persona (deviation 1 there — unchanged and
  visible here as "No burn of theirs checked here"); and the badge outside the
  thread on the ride offer and the tab's customer picker was not exercised.

Artefacts: the session scratchpad's `walk2/` — `lister.log`, `lister2.log`,
`host.log`, `lister-run1-broken.log`, `lister-run2-nostamp.log`, the desk state
`lister/` with its `ducat.log`, `phone-full.log`, `phone-backup.ducatbak`
(passphrase `walkwalk`), `adbn`, `ns-emulator.sh`, `ui.py`, and screenshots
`s01`–`s09` (first window) and `w2-01`–`w2-12` (second).
