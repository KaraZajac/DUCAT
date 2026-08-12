# The Android client — design notes

Written before any Kotlin, because two of these decisions constrain everything
after them.

The information architecture follows PayPal's, deliberately: a hamburger for the
long tail, a bottom bar with an elevated centre action, Accounts, Activity. That
shape is familiar to hundreds of millions of people and there is no advantage in
being novel about navigation.

What follows is where the familiar shape meets facts the protocol will not bend
on.

---

## 1. Send / Request is not a UI invention — it is `presenter_role`

PayPal's centre button is Send/Request. Ours is the same two verbs, and they map
onto §15.2's field rather than onto a mode we made up:

| Button | `presenter_role` | What happens |
|---|---|---|
| **Request** | `payee` | I present; you tap me. The POS direction. |
| **Send** | `payer` | I read your tap — or I present and you charge me. |

Both directions run end to end (0.56). Worth knowing when building the screens:
**they are not symmetric.** The presenter supplies reachability, so the *reader*
drives every round trip — which means in the Send direction the till polls, and
in the Request direction nobody polls. A screen written against one and reused
for the other will hang.

---

## 2. Accounts holds three things, and §17.2 forbids blurring them

PayPal's Accounts tab lists balance, savings, cards. Ours lists three things that
are genuinely different, and §17.2 is explicit that a client must not present
them as one number:

| | What it is | Spendable? |
|---|---|---|
| **Float** | Pre-split outputs on this phone | Yes — this is the money |
| **Reserve** | Behind a hardware wallet (§4.4) | No, not from the phone |
| **Bond** | Posted collateral backing `fast/1` capacity (§17.2) | No — locked until withdrawn |

§17.2's words: *"a client that tells the user 'this is just spending money' has
described one half and mislabelled the other."* Say plainly which is which.

---

## 3. The balance problem, which has no PayPal equivalent

This is the screen to get right before anything else, because it constrains the
home layout and it is the thing most likely to lose a user.

PayPal shows a number you can spend. We cannot, honestly. §17.2, measured:

- Capacity is a **count of unlocked outputs**, not a balance.
- A payment consumes at least one **whole** output; change returns **locked for
  ten blocks**.
- Six unlocked outputs bought **four** consecutive payments in the drain test, so
  the count is an upper bound.
- §17.2 therefore **forbids** promising an exact number: *"about 4 more
  payments"* is honest, *"4 more payments"* is not.

A user who sees `$40`, taps, and is declined will not forgive it — and will be
right not to.

**The framing that is both accurate and ordinary: notes in a wallet.**

> You have $40, as four $10 notes. You can make four purchases. Getting change
> back takes about twenty minutes.

That is not a simplification of Monero's output model — it *is* the output
model, and people already understand it because physical cash works the same way.
It makes the ten-block lock legible ("waiting for change") instead of mysterious,
and it makes re-splitting legible too ("breaking a note").

Three requirements fall out, all from §17.2:

1. The home screen shows **spendable now**, not total. Locked change is shown
   separately, as pending, with the time remaining.
2. Capacity is shown as an approximation and never as a promise.
3. The client **warns before the count reaches zero**, not at the counter, and
   offers to re-split. §17.2: *a client that funds a float and immediately offers
   to transact will fail at the curb with a full balance on screen.*

---

## 4. The activity log will be mostly nameless, and that is correct

PayPal shows *Andrew Sievert* because there is an account behind every payment.
We are pseudonymous by default: a name exists only where a persistent contact was
established in §16.3's post-receipt coda. Most rows will have no counterparty
name, ever.

Twelve unnamed rows will *feel* broken unless it is designed for. Three things
help, none of which touch the wire:

- **Local annotation.** Let the user title a transaction after the fact — "coffee
  by the station". Stored locally, never transmitted, never in a receipt.
- **Profile and amount carry the recognition** where a name would: a `pos/1` at
  a time and place is usually identifiable to the person who was there.
- **Make the contact coda visible.** After a receipt, offering "keep this
  contact?" is the moment a name can exist at all. §16.3 puts it after the
  transaction completes, deliberately — the deal closes anonymously first.

Retention differs by mode, per §7.4: a consumer holding four years of coffee
receipts has built the dossier the protocol went to some trouble to avoid
creating, so consumer-side transcripts expire on a default and merchant-side ones
do not.

---

## 5. Modes — one app, two defaults

The PayPal screenshot has `Personal account ▾` at the top of the profile screen.
That is precisely how one app serves a shopper and a till without being two apps,
and it is why the one-package decision holds.

A mode is a **bundle of defaults**, not a different build. Each item below is a
spec-level difference, not a cosmetic one:

| | Personal | Merchant |
|---|---|---|
| Default centre action | Send | Request |
| Receipt retention (§7.4) | Expires on a default | Retained until deleted |
| Verification floor (§15.5.1) | Low — protects a stolen phone | Higher — a queue is real |
| Float sizing (§17.2) | A few payments | Sized to a day's takings |
| Staff terminals (§4.2) | Hidden | Device delegation visible |

The user can switch, and a vendor who buys their own coffee is not doing anything
unusual — in the market run, `coffee_01` paid `shopkeep_01`.

---

## 6. What the hamburger holds

Everything PayPal puts there is a financial product they sell. Ours is the
settings surface the spec already requires:

- **Personas** — create, switch, delegate a device (§4.2)
- **Backup** — export and import, with the passphrase warning §4.3.4 requires
- **Custody** — software, hardware reserve, hardware only (§4.4)
- **Verification** — the thresholds and windows of §15.5.1
- **Markets and arbiters** — the signed set a client accepts (§10.1)
- **Relays** — which nodes to submit through (§8.7.2)
- **Records** — retention, verifiable export, accounting export (§7.4)

There is no Offers, no Rewards, no Pay Later, no credit. That is most of PayPal's
surface area and all of it is the operator monetizing. The app will feel emptier;
that is the product, not a gap to fill.

---

## 7. What has no answer yet

- **"Log out" has no meaning.** There is no account and no session. The profile
  screen is persona management; there is nothing to sign out of.
- **Onboarding is four steps** — persona, float, thresholds, backup — and the
  fourth is the one users skip and the one that costs them everything (§4.3).
  PayPal's `3/4` progress card is the right pattern; the ordering needs thought.
- **The tap budget is unmeasured on a handset.** §8.7.2's 34/221/297 ms are a
  desktop with an attached node. A cold node, cellular, and route
  re-establishment are all additive, and the last is bounded below by a full
  round trip.

---

## 8. Both dependencies cross-compile — checked, not assumed

Before designing anything on top of them:

| | `aarch64-linux-android` |
|---|---|
| `veilid-core` 0.5.7 | **builds** |
| `monero-wallet` 0.2.0 with `multisig` (FROSTLASS) | **builds** |

Neither is free. `veilid-core` pulls `libsqlite3-sys`, whose build script needs
the NDK's C compiler — `CC_*`, `CXX_*`, `AR_*` for the target, not just a linker.
That is a real setup step and it fails with an unhelpful "custom build command
failed" if it is missing, so `mobile/build-android.sh` exports them.

This settles §8.2's open question for Android specifically: **there is no
`monero-wallet-rpc` on a phone**, so the choice was never embedded-versus-RPC. It
was embedded or nothing, and embedded builds.

What it does **not** settle, and both are recorded elsewhere rather than
rediscovered here: `dkg-pedpop` still does not link alongside `monero-wallet`
(§8.2 — a `multiexp` major-version conflict), so key generation for a threshold
group has no crates.io path yet. And FROSTLASS groups cannot co-sign with
wallet2 groups, so a market's declared scheme is load-bearing (§10.1).

## 9. Onboarding is four steps, and the fourth is the one that matters

PayPal's "Set up your account 3/4" card is the right pattern. Ours:

1. **Create a persona** — a keypair, instant, nothing to explain.
2. **Create a wallet** — a Monero spend key and its seed.
3. **Set spending limits** — §15.5.1's thresholds, with the defaults already safe.
4. **Export a backup** — §4.3.

**Step 4 is the one users skip and the only one whose absence is unrecoverable.**
A persona lost with no backup takes its reputation and every persistent contact
with it, and no operator exists to appeal to — that is the same property that
makes the system uncustodied.

So the order is deliberate: the backup step comes **before** the wallet can be
funded. A user with nothing to lose has no reason to skip it and no reason to
resent it; a user with money in the float has both. The one moment when the cost
of doing it is zero is the moment before there is anything to protect.
