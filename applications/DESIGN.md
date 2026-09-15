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
| Verification floor (§15.5.1) | zero — every payment | zero — every payment (**corrected**, below) |
| Float sizing (§17.2) | A few payments | Sized to a day's takings |
| Staff terminals (§4.2) | Hidden | Device delegation visible |

**Corrected: the floor is not a mode default, and it is not "low".** This row
said *Low — protects a stolen phone* against *Higher — a queue is real*.
`VerificationPolicy::default()` in `core/src/verify.rs` sets `device_unlock_at:
0`, commented `// every payment`, so **every payment, on every phone, in every
mode** wants a device unlocked in the last two minutes. The module says why
there is no tier below it: what leaves is money in a wallet nobody can reverse,
not a transit fare. A merchant with a queue does not get a laxer floor, because
the gesture is the same one — unlock the phone, open the app, tap.

The number a user *does* set is the tier above it: `app_secret_at`, the amount
at or above which the PIN is asked **every time**, defaulting to 10,000 minor
units — a hundred of whatever they price in. One setting per persona, not a
per-mode default, and it lives in drawer → Profile → Spending (`ui/Drawer.kt`,
`SpendLimitSetting`). Above that sits a rung this table never had: a rolling-hour
cumulative limit, because twenty payments just under a per-payment cap is how a
lifted phone is actually drained.

What survives of the row's premise is the part that was never about modes: all
of this is local policy and none of it goes on the wire, so a payee cannot ask
for a weaker tier.

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

**Correction, 2026-09-15.** That was the plan. What shipped is the `Section`
enum in `ui/Drawer.kt`: Status, Profiles, Contacts, Feed, Library, Sites,
Selling, Publishing (routable but unlisted — the Press room is its door),
Logs, Settings, Modes. The difference is not drift so much as the app finding
out what a settings surface is *for*: Backup, Custody, Verification and
Records all turned out to be pages under Settings or Profiles rather than
top-level rows, personas became **Profiles**, and Feed, Library, Sites,
Selling and Modes are the surfaces the protocol grew afterwards. Markets and
arbiters is the one row that is simply absent, because §9.3's market does not
exist yet — the arbiter is a contact you pick. The paragraph below still
holds, and is the part worth keeping.

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
  **→ Answered, and the count was wrong: it is six, and one of the four is not
  in onboarding at all.** §9 below carries the correction.
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

## 9. Onboarding — planned as four steps, shipped as six

PayPal's "Set up your account 3/4" card is the right pattern. As planned here,
before any of it existed:

1. **Create a persona** — a keypair, instant, nothing to explain.
2. **Create a wallet** — a Monero spend key and its seed.
3. **Set spending limits** — §15.5.1's thresholds, with the defaults already safe.
4. **Export a backup** — §4.3.

**Corrected against `enum class Step` in `ui/Onboarding.kt`.** It is six, in
this order, and planned step 3 is not among them:

1. **Persona** — the keypair, written to its store the moment it is created
   rather than at the end, because a rotation used to throw it away and mint a
   fresh one at Finish.
2. **Profile** — the name that goes on your cards, and whether contacts may pay
   your address directly. Both changeable later; both asked before anybody
   holds a card of yours.
3. **Wallet** — the Monero spend key, with a node asked where the chain is so
   the restore height is not genesis (§10).
4. **PIN** — chosen here, before there is money to lose and while a person is
   still attending to set-up rather than to a customer. This screen used to
   *describe* a PIN the app did not have.
5. **Trust** — how strangers trust each other with no company in the middle.
   Before the first deal, because it is the question every user of an
   operatorless market asks sooner or later.
6. **Backup** — §4.3.

A restore is a different, shorter flow and is counted as one: the file that
brought them here *is* the backup, so what is left is the import and a PIN, two
steps rather than six.

**Spending limits are not an onboarding step, and should not be.** §15.5.1's
floor is zero for everybody and asks the user nothing (§5's correction), and the
one number they do set — the amount above which the PIN is asked every time —
lives in drawer → Profile → Spending, where they can find it again. A threshold
picked on a phone with no money on it is a number chosen with nothing to weigh
it against.

**The last step is still the one users skip and the only one whose absence is
unrecoverable.**
A persona lost with no backup takes its reputation and every persistent contact
with it, and no operator exists to appeal to — that is the same property that
makes the system uncustodied.

So the order is deliberate: the backup step comes **before** the wallet can be
funded. A user with nothing to lose has no reason to skip it and no reason to
resent it; a user with money in the float has both. The one moment when the cost
of doing it is zero is the moment before there is anything to protect.


---

## 10. The restore height, and a fix that was worse than the bug

Recorded because the reasoning was plausible at every step and still arrived
somewhere unrecoverable.

1. The app passed `0` as the restore height for a new wallet, with a comment
   claiming that was correct.
2. `0` is genesis, and §4.3.1 measures genesis at roughly **106 hours** of
   rescan. So it was "fixed" with a sentinel — `ULong.MAX_VALUE` — meaning
   *unknown*.
3. A backup written on a handset was then opened on a desktop, and carried
   `18446744073709551615`.

**That sentinel is the catastrophic direction.** §4.3.1's two failures are not
symmetric and treating them as interchangeable is the mistake:

| | Cost |
|---|---|
| Height too low (genesis) | Slow. ~106 hours. **Recoverable.** |
| Height too high | A wallet scanning forward from after every output it owns. Finds nothing. Reports zero, with no error anywhere. **Unrecoverable by the user.** |

So the app records genesis when no node has been asked, and `export_backup`
**refuses** a height beyond any plausible chain. A slow restore is a bad
afternoon; an empty one is a lost wallet, and the user's conclusion will be that
the software took their money.

The general shape, which is not specific to Monero: **when a placeholder must
stand in for a real value, pick the one that fails loudly and slowly, not the one
that fails silently and fast.** A sentinel chosen for being obviously-not-a-real
value is chosen for a property nothing downstream checks.

---

## 11. Personas need names, and a name is not an identity

On the roadmap: knowing *who* you are paying. Worth writing down now because the
obvious implementations are the dangerous ones.

**§15.9 already taught this lesson in a different shape.** A signed static tag
proves who owns an address and never that the tag is the one the venue put there
— a swapped tag carries the attacker's persona with a perfectly valid signature
over the attacker's own address, and verifies. A self-asserted display name has
exactly the same property: **anyone can call themselves anything**, and a
signature over a name proves only that the holder of a key chose that string.

Two designs to avoid:

- **A global name registry.** That is a directory, and a directory is the thing
  this protocol deletes. It also becomes the chokepoint everything else was
  designed to avoid.
- **Trusting a display name on first contact.** A name shown before a
  relationship exists is a claim with nothing behind it, and putting it beside a
  payment amount lends it authority the protocol cannot support.

The design that fits what already exists is **petnames**:

1. A persona may carry a self-asserted display name in its contact card.
2. §16.3's post-receipt coda is where it is exchanged — after the transaction
   completes anonymously, which is already the ordering.
3. **The receiver stores it locally and may rename it.** What the user sees is
   their own label for a key they have transacted with, not a claim the network
   vouches for.
4. Two contacts may share a display name; they can never share a key.

This is the Zooko trade taken deliberately: names that are secure and meaningful
to *you*, given up as globally unique. It is also how people already reason about
their phone's contact list.

### VeilidChat interop

Attractive because it is where a Veilid identity already exists, and a payment
that lands next to a conversation is a payment with a face on it. It is also a
dependency on another project's contact schema and identity model, which is a
larger commitment than it looks — §11's many-clients goal cuts both ways.

Recorded as **roadmap, not requirement**: implement petnames and the profile
first, since they are needed regardless, and treat interop as an integration to
evaluate once there is something to integrate. Messaging between friends is a
genuinely good reason to want it, and it is not on the path to a working payment.
