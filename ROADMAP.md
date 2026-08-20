# Road to 1.0

What stands between draft 0.85 and a release that strangers can trust with
money. Ordered by what blocks 1.0, not by effort.

## Protocol correctness — must fix before 1.0

- ~~**Per-contact prekey partitioning (§16.11).**~~ **Done, 0.86** — each
  thread's head offers a disjoint batch; ids stay globally unique; burns
  prune the owning thread's offer. One global bundle was
  published to every contact's head and senders take the first one-time
  entry — so two contacts holding the same cached bundle seal to the same
  key, the first message burns it, and the second arrives permanently
  unreadable. Partition one-time ids into disjoint per-contact sub-bundles,
  or gate the burn-pen sweep on observed bundle propagation rather than a
  fixed 30 minutes. Found in the 0.85 review (finding M6); the only
  unfixed protocol-level defect on the list.
- ~~**Rider notice migration down the ladder (§15.12).**~~ **Done, 0.86** —
  every tenth claim-poll tick, a notice on an overflow shard looks for a
  lower free slot: post-low-first, verify landing by card, then clear the
  old slot; drivers dedupe by card during the brief double-listing.
- ~~**Density-adaptive cell precision (§15.12).**~~ **Done, 0.87** — a
  deserted 6-cell earns a second copy of the notice on the containing
  5-cell (same card; claim-once referees); drivers watch both precisions'
  neighbourhoods; everything dedupes by card; all copies cleared together.
- ~~**Typed offer/accept ceremony for rides.**~~ **Done, 0.87** — kinds
  RETRACT (5), RIDE_OFFER (6, fare MUST + eta 213), RIDE_ACCEPT (7, names
  the offer, echoes the fare). Claim = applying; full-screen offer
  ceremony on the rider, waiting/confirmed states on the driver; harness
  speaks all three.

## Trust — the stranger problem, named since 0.82

- ~~**Driver bonds through Part IV escrow.**~~ **Built + proven, 0.88** —
  §17.9 ceremony (DKG then FROST over the sealed thread, kinds 8/9/10).
  Trustless DKG→fund→FROST-release proven on stagenet with no dealer
  (escrowtest.rs; release txid da200c13). Version blocker solved by
  vendoring dkg-pedpop against multiexp 0.5 (mobile/vendor). The ceremony
  engine ships in the app (mobile/src/ceremony.rs — machines held by
  ceremony_id, stepped by wire bytes; unit-tested), and the app glue
  (Ceremony.kt) drives it from the poll loop: startBond → DkgRound →
  engine → escrow address. Builds, installs, no regression. The contact
  profile now carries the first bond UI: "Post a bond" plus the live
  ceremony stage, ending in the escrow address.
  ~~Full two-phone live run~~ **Done, 0.88 (2026-08-16)** — two emulated
  phones exchanged cards over the DHT, one tapped Post a bond, and both
  independently derived the same escrow address (59D2GZaC…RLqYe), each
  holding only its own share. The run flushed out a real ceremony bug:
  two rounds sent in one poll cycle reused the same outbox seq (the
  share overwrote the commitment in the ring); onDkgRound now threads
  the updated contact through consecutive sends and re-reads the store
  at entry. Networking got there via emulator-tap v2 (stock guests,
  host-side conntrack marks) plus guest-side DNS DNAT to a public
  resolver — no host resolver dependency at all. **Left:**
  ~~FROST-release glue (kind 9)~~ **Done + proven live, 0.88
  (2026-08-16)** — the escrow the two phones built was funded 0.01 XMR
  (txid ebf1a064…) and, after the ten-block unlock, phone 1 tapped
  "Return the deposit": frost_propose swept the escrow into one
  transaction and sent [tx][preprocess] as FrostRound 0, phone 2
  co-signed in one step over the mailbox (fee it read from the bytes:
  0.00012176 XMR), and phone 1 completed and broadcast — release txid
  fad3de2c…, accepted to the pool, 0.0098 XMR back to phone 1's own
  wallet. Both UIs carry the outcome ("Deposit returned ✓" / "Co-signed
  — the release is theirs to broadcast").
  ~~Arbiter-set role (2-of-3)~~ **Done + proven live, 0.88 (2026-08-17)**
  — a three-party bond ran two phones plus the desktop client as arbiter,
  all three deriving one escrow (58gijsFj…KeJg2Sz), each holding only its
  share; any two can sign, so a lost phone strands nothing. Ceremonies
  carry a roster now (round-0 frames it; a joiner verifies it hashes to
  the ceremony id); the desk is a full participant because Ceremony.kt is
  shared logic. Flushed out a real bug — an early round-1 share dropped at
  stage "committed" deadlocked one party — fixed by recording every round
  before advancing. ~~The arbiter-assisted *dispute* release~~ **Done +
  chain-proven, 0.88 (2026-08-17)** — proposeRideSplit(toArbiter): the
  identical release proposal goes to the third key; the arbiter's
  co-signature is the ruling; a captured arbiter can at worst pick a
  split between the named parties. Proven via escrowtest dkg3/rule:
  driver + arbiter released a funded 2-of-3 with the rider absent (txid
  ec401a91…). Phone: one "Ask the arbiter to rule" button for the
  stranded; desk: a ruling console — requests print, approval is a
  human-written line, the judgment deliberately unautomated. **Left:**
  the rest of the bond UI — co-signer consent needs a payments accessor on
  monero-wallet's SignableTransaction (0.2.0 keeps them private; until then
  the co-signer sees only the fee), plus bond amount and funding flow.
- **The bonded hail (0.88, 2026-08-16): every accepted ride can escrow its
  fare.** With an arbiter contact configured (the Escrow-arbiter switch on
  a contact's profile), the rider's accept starts a 2-of-3 DKG with driver
  + arbiter; the round-0 frame self-describes (kind/funder/fare); the ride
  banner in the thread carries the rest — rider funds the derived address,
  both sides' own scans (new escrowBalance bridge fn) flip to "fare
  secured", driver's Complete proposes the FROST release to their own
  wallet, and the rider's release is a consent tap, never an auto-cosign.
  Proven live end to end on two phones with the desk as headless standing
  arbiter (:desktop:arbiter): funding, fare-secured by own scan, consent
  release, broadcast, driver paid +0.001878 XMR. The ladder's 2-of-2 rung
  (no arbiter: fare + rider margin, mutual stakes) and the split release
  under it are engine-proven on-chain (one FROST tx, two destinations,
  txid de818596…); the 2-of-2 accept flow is **proven between two real
  clients (2026-08-18)** — `:desktop:ridetest` runs the whole arc from a
  restartable script: escrow 56RCwMGC…, rider 0.0006 (e364341c…), driver
  stake 0.0002 (f559e4d8…), release 8615c2b2… mined 2187858, two inputs
  to two outputs, rider back 0.000100 and driver 0.000518. Stakes are
  symmetric by default now (10/20/30% by deal kind, Stakes.kt) and the
  exposed side funds second. Two live finds fixed:
  concurrent Mailbox.poll double-joining a ceremony (poll + ceremony
  rounds now synchronized; the race cost one stranded 0.006 stagenet
  escrow), and a repeat thread accepting a stale kind-6 (newest wins).
  Same primitive is the Airbnb/Turo shape: offer terms → accept binds →
  escrow holds → mutual release or ruling. **The reservation shipped
  (0.88, 2026-08-17)**: KIND_RESERVATION — guest initiates from the chat
  tray with rent + both deposits in the frame; the host's acceptance IS
  funding their deposit; secured = rent + both deposits by own scan;
  default checkout splits each deposit home beside the rent; settlement/
  counters/rulings inherited verbatim. Chain-proven with the last
  unproven shape — a TWO-INPUT FROST release (guest tx 4d6de9d8… + host
  tx bef0d57c… → one split 8ccf79ab…). Left: listings/discovery (a Host
  mode is only worth building with it) and the live two-phone pass,
  folded into the field day. **Settlement shipped (0.88,
  2026-08-17)**: either principal proposes a split (one number — what the
  funder gets back), the other's banner states it and offers Sign or
  Counter; fresh proposals supersede, whoever signs ends it; a rider's
  proposal can only pay the driver at the address the driver published;
  near-total refunds flip the fee to the rider's side. FROST_ROUND round 0
  MAY carry the claimed amount now — the second implementation caught the
  rule drift (O21 doing its job) and both agree on 241 vectors. **Left for
  the field day:** the 2-of-2 accept flow and the settlement UI's live
  two-phone pass (the underlying split tx is chain-proven, de818596…).
- **Opt-in live location after commitment.** ~~Spec it before building
  it~~ **specified, 0.88 (2026-08-16)** — §15.12 "Live position after the
  accept": gated on RIDE_ACCEPT, consent per ride per direction, off by
  default and never a standing setting; a record not messages (kind 11
  POSITION_REF, fields 218–219), one subkey overwritten in place — a now
  with no past — sealed under a fresh stream key with the record key as
  AAD, monotonic counter, fixed padding and cadence; bounded by client
  stop rules (receipt / RETRACT re_own / expiry) and record TTL; receiver
  MUST NOT retain the track. Position stays display-only. **Left:** the
  build, once a ride to point it at exists on real hardware (field day).

## Privacy — spend it only where it buys something

- ~~**Monero subaddress per contact.**~~ **Done, 0.87** — every contact
  gets subaddress (0, minor), allocated once (cards pre-allocate; the
  claimant adopts); every request/tab/handshake address is per-contact;
  all three scanners watch every allocated minor; outputs record their
  receiving minor, and tab reconcile refuses an output that landed on
  someone else's — attribution by construction, not by believing a note.
- ~~**Storage encryption for the message store.**~~ **Done, 0.88
  (2026-08-16)** — ducat_contacts (spend key, persona secret, contacts,
  the whole message/receipt history) and ducat_ceremonies (escrow key
  shares) now route through securePrefs(), an EncryptedSharedPreferences
  chokepoint: AES-GCM values, AES-SIV keys, a master key that lives in the
  Android Keystore and never touches disk. A one-time migration copies each
  plaintext file into its _enc twin and deletes the original, ordered
  commit-before-delete so a crash mid-migrate just re-copies next launch;
  DucatApplication names both stores at startup so ceremonies migrate on
  the first post-upgrade launch, not the next escrow. Settings (locale,
  units, ride draft, map cache) stay plaintext by design — no secret, no
  gain. Desktop gets a plaintext delegate of the same signature (no
  Keystore, different threat model). Verified live: installed over real
  plaintext data, 72+1 keys migrated, plaintext gone, _enc ciphertext with
  the wallet address appearing zero times, wallet/contacts/history intact.
- ~~**Profile-wide privacy pass.**~~ **Done, 0.88 (2026-08-16)** — audited
  every field for who needs it, when, over which channel. The profile's only
  transmission surface is the card handshake (issue + claim); a later change
  reissues rather than pushing, and the backup is the user's own. The gap:
  email/phone/signal — real-world locators, the plate's own class — rode
  *every* handshake, so a "sale" till published the owner's Signal to every
  customer and the customer sent theirs back. Fixed by carrying the
  handshake's `purpose` in the record: issuer stamps and scopes, claimant
  reads and scopes its reply, both directions of a transaction now carry no
  reach-me identifiers while a deliberate contact exchange still does; a null
  purpose (older card) is the private default. Car/plate stay scoped to a
  driving claim; name/face/pronouns stay the low-cost introduction gesture;
  the payto address keeps its own §16.12 switch. Verified via
  :desktop:profilescope through the real toWire → build/parse path.
- ~~**Backup hygiene for device-local state.**~~ **Done, 0.88
  (2026-08-17)** — the audit found the scars (`stuck_`/`slotseen_`, prekey
  burn state) are already excluded: backupAppState is an allowlist, so
  transport keys in the same prefs file never enter a backup. The real bug
  was the inverse — claimed_kis_v1, a StringSet that MUST survive (it stops
  a deleted paid tab's output re-matching a still-open bill), was exported
  mangled and dropped on restore because restore only handled Boolean and
  String. Fixed the round-trip (JSONArray both ways), gave the desktop
  shim StringSet parity, and added :desktop:backuptest as a regression.

## App robustness — the 0.85 review's unfixed tail

- ~~**State survives rotation and process death.**~~ **Done, 0.88
  (2026-08-17)** — onboarding persists the persona and wallet at creation
  and resumes from the stores (this also fixed a latent bug: the persona
  was never persisted, so the app ran under a different identity than its
  own backup was signed with); the nav tab, the Send/QR sheet flags, and
  the pay sheet's typed amount/memo/address are rememberSaveable. The
  follow-ups landed too: the POS till is saveable end to end — basket
  (listSaver), tax, quick amount, half-typed line, and on the charging
  screen the card itself, the customer (persona hex, re-resolved) and the
  tab id, issued once per sale so a rotation mid-scan no longer strands
  the customer's claim on an unwatched card; the chat overlay and the pay
  sheet's PayTarget save as the string that names them (persona hex /
  address) and re-resolve from the store, falling back to the shell or
  chooser if the contact is gone. All proven on the emulator across
  recreation and kill -9 process death: the till restores into the same
  charging screen with the same card, the open conversation comes back,
  the pay sheet stays aimed at its contact with the amount intact.
- ~~**Bill cancellation tracking.**~~ **Done, 0.87** — a vendor cancel
  sends RETRACT(re_own) naming the bill; the request bubble renders
  "Cancelled" instead of a live Review payment button.
- ~~**Poller cadence and battery.**~~ **Done, 0.88 (2026-08-16)** — the
  screen-local loops (POS 2 s, hail 3 s/4 s) were already innocent: they
  stop with their screen. The real sink was the background poller running
  a full sweep after every 10 s wait, five a minute, forever. Now the wake
  chunk stays 10 s but the sweep is tiered by visibility (started-activity
  count in the Application): foreground sweeps every wake exactly as
  before; background sweeps only when a watch rang or on a ~3-min
  heartbeat, so an idle pocket does ~20 sweeps an hour instead of ~300.
  Measured live: heartbeat at 18 quiet wakes on the nose; a real message
  sent from the desk's new :desktop:ringtest woke the backgrounded poller
  163 ms after the send. True battery numbers still want hardware — folded
  into the NFC/field-day item.

## Localization — global like the rails it rides (started 2026-08-15)

- ~~**Infrastructure.**~~ **Done, 0.88** — LocaleStore + attachBaseContext
  wrapper (activity and Application), in-app language picker named in each
  language's own name, locales_config for Android 13+, Units (km/mi with
  locale default), currency picker revived; Settings proven in Spanish on
  the emulator, choice survives restarts.
- ~~**Extraction.**~~ **Done, 0.88** — ~848 entries across 31 per-screen
  resource files; plurals used where count-driven; wire sentinels, state
  strings, and Locale.US parse formats deliberately left in code.
- ~~**Translations.**~~ **Done, 0.88** — nineteen languages: es fr de pt
  it nl ru uk pl tr zh ja ko ar fa hi id vi th. One values-<tag>/ mirror
  per screen file, 845 strings + 15 plurals each, all mechanically
  validated (placeholder multisets, key sets, sentinel dashes, plural
  quantities per CLDR). RTL proven twice (ar, fa) with mirrored layout
  and native digits. Untranslated keys fall back per-string, so a new
  language can land partially and still ship.
- **Known gaps.** ~~Pronoun labels come from the bridge's
  pronounOptions() and need a mapping layer~~ **done, 0.88** — a
  pronoun_labels array per locale, indexed by the wire code; Romance
  locales use their live neopronouns (elle/iel/elu), Chinese uses TA, and
  genderless-pronoun languages (tr fa hi id) keep the English sets by
  intent. Still open: notification text keeps the process-start language
  until restart (attachBaseContext runs once per process); outbound chat
  bodies ("Meter started…") localize to the *sender's* language by design
  — the receiver sees the sender's words, like any message.

## Small bugs spotted, not yet fixed

- ~~**"Break a note" card shows on a zero-balance wallet**~~ **Fixed,
  0.88** — the card now fires only when money exists (hasMoney/allLocked
  guards in BalanceCard); an empty wallet says nothing.

## The counter — modes that take money (0.88, 2026-08-19)

Not on this roadmap when it was written, and load-bearing now: these are
payment paths with money in them.

- **A saved menu** (Catalogue): items priced in the seller's own currency,
  converted at the moment of the sale; shared by the till, the bar tab and
  the kiosk so nobody types their menu twice. Sold-out is its own state,
  distinct from archived.
- **Kiosk mode**: a screen facing the customer. They tap what they want,
  tap or scan once, and the bill arrives on their own phone itemised —
  the counter speaks DUCAT rather than showing a bare `monero:` code, so
  the payment is identified by the transaction the payer names and the
  receipt lands beside it in their Activity. Leaving needs the PIN.
  Proven end to end over live Veilid and stagenet (`:desktop:kiosktest`).
  Tips, and calling an order ready, ride the conversation the card opened.
- **A PIN** in front of every spend, set during onboarding, with the
  phone's own lock offered where one is enrolled.

**Left:** refunds. There is no path to give money back after settlement —
`cancel` withdraws a bill before payment, `markPaidOutside` records
another rail, and neither is a refund. The open question is what the
customer's Activity should show when money comes back, which is a design
question before it is a build.

## Validation — before the number says 1.0

- **NFC tap, live.** §15's core gesture has never been tested on
  hardware. One field day: tap-to-pay, tap-to-contact, the §15.5 confirm.
  The *wire* is covered as of 0.88 (`:desktop:taptest` runs the reader
  against the card service: chunk boundaries, multi-byte payloads, a peer
  that is not us, a field dropping mid-walk) and that found one real
  defect — Type 4 NDEF stickers could not be read at all, because the
  ISO-DEP branch returned before the NDEF branch was reachable. What is
  left is the radio itself, which needs two handsets.
- **Two-phone field day for dispatch.** Post, sweep, claim, offer/accept,
  drive, geofenced bill-on-arrival, receipt — all phone-to-phone, no
  harness.
- **External adversarial review.** §2.5 still says "no adversarial review
  whatsoever," deliberately. 1.0 is the moment that stops being a
  deferral and becomes a gap. Scope: the spec's crypto ceremonies and the
  board/mailbox surfaces.
- **O21's last gap.** An implementer who has never read `core/` builds
  from the document alone. Everything accidental is cleared; what remains
  is finding that person.

## The desktop client (parallel track, kara 2026-08-15)

An Electrum-shaped DUCAT client for Linux/Windows/Mac. The protocol stack
is already cross-platform Rust; the path, cheapest first:

1. **Harness → CLI client.** Multi-contact state landed (--contacts,
   --contact-save, DUCAT_CONTACT selects the thread; --geo for board
   names). Still wanted: a card-issue flow with a QR on the terminal,
   a persistent watch daemon (one process, all threads), and packaging
   (static binaries for the three OSes).
2. **GUI — building, `:desktop` module ("DUCAT Desk").** v2 compiles the
   phone's protocol sources verbatim against a four-class Android shim:
   one implementation of Mailbox/ContactStore on every screen. Window:
   contacts, chat, claimable card QR, the phone's poll loop; headless
   `:desktop:smoke` gates the stack in CI. ~~Next: bills/pay rendering,
   wallet scan loop, notifications, then packageDeb/Msi/Dmg artifacts.~~
   **v3, 0.88 (2026-08-17)** — the desk earns its till: a wallet born at
   first run (creation height from a live node, same as onboarding), the
   scan loop folded beside the mailbox sweep, balance + fiat in the top
   bar, a Receive QR; bills render their lines (already proven to sum),
   an incoming request's Pay quotes fee/total/remaining before the one
   button that spends (§5's review, desk-shaped) and sends the §16.13
   notice after; incoming payments offer the receipt the desk owes, once;
   tray notifications ride the shared announce funnel (DeskGlue's Notify
   grew a sink — headless desks stay quiet by construction). Packaging:
   the app image bundles the host Rust library and jdk.unsupported, and
   the distribution's own jars + bundled .so passed the live smoke gate;
   `ducat-desk-1.0.0-linux-x64.tar.gz` builds today. Deb/Rpm need host
   tools (`dnf install rpm-build fakeroot dpkg`), Msi/Dmg their own OS or
   CI. The arbiter takes `--name` now. **The UX pass + cross-client till
   run (2026-08-17)**: the window speaks shopkeeper — one status word
   (peers/heights behind a click), fiat beside amounts, unread dots,
   Copy buttons over base64 walls, no transaction hex in the thread —
   and `:desktop:tilltest` proved the whole till story against a real
   phone over the live network: deep-link claim → itemised bill → the
   phone's takeover/review → stagenet payment (tx 91319291…, mined
   2187457) → notice → tray funnel → receipt on the phone, the till's
   own scan holding 0.0005 locked. Found and fixed along the way: the
   manifest registered ducat: links since the beginning and readIntent
   never read them — a tapped card opened the app to Home; it now claims
   behind one confirm, in all twenty locales. The shim retires into a
   real shared module once the surface is known. **Feature parity, 0.88
   (2026-08-17)**: the desk now runs the phone's *screens*, not just its
   protocol — a build-time resource bridge (generateDeskRes → R.kt + one
   JSON table per locale; android/Resources.kt with per-string fallback
   and real CLDR plural classes) makes `stringResource(R.string.…)`
   resolve, so chat, the till, the bar tab, donations, the wallet,
   activity, contacts, profiles, the profile editor, backup, settings,
   the code hub and hailing are the phone's own source running here, in
   all twenty languages. Six phone files stay phone-side — camera, NFC,
   osmdroid, GPS, the Android inset flags, first-run — and each has a
   named desk half: a paste field, a Compose-drawn route and driver net
   that touches no tile server, a position typed once, WAV voice memos
   (the JVM has no AAC encoder, so the sender labels by what was actually
   recorded). Smoke-tested headlessly: :desktop:shimtest (902 ids × 20
   languages, Slavic plurals, the avatar encoder's 12 KB ceiling) and
   :desktop:rendertest, which draws every hosted screen through
   ImageComposeScene with no display and caught two rooms that crashed on
   first composition.
3. **iPhone** eventually: uniffi generates Swift bindings natively and
   the Rust stack compiles for iOS — the protocol layer is free; the UI
   and App Store review are the cost. Nothing now forecloses it.

## What else this shape is for (after 1.0)

Four things the machinery already mostly supports. None are 1.0; all are
reasons the 1.0 primitives are worth getting right.

- **Selling a thing to a stranger.** The listings shape with a different
  noun: post to a geocell board, somebody claims, you meet, escrow
  releases on handover. What makes it worth building is not the payment —
  it is that escrow answers the specific fear that makes this category
  miserable. Meeting a stranger to buy a bike is frightening in both
  directions, and every platform answers that with ratings and a support
  queue. A 2-of-2 with a stake each side means neither party can walk.
  Reuses listings, boards, claim-once cards and the bond almost entirely.

- **Subscriptions with no card on file.** A weekly box, a monthly dues:
  the seller bills the thread on a schedule, the buyer taps approve, money
  moves. What is *absent* is the point — there is no stored payment
  credential, so nothing to leak in a breach, nothing to charge after a
  cancellation, and nothing that makes cancelling harder than subscribing.
  Every recurring relationship today rests on the merchant holding a key
  to the customer's money; this is recurring billing with no recurring
  authority, which has no equivalent anywhere. Needs a schedule on a tab
  and a standing thread; it needs no new protocol.

- **Day labour by the hour.** The hail shape, again with a different noun:
  two hours of help moving, claimed by somebody nearby, escrowed, released
  on both saying it is done. Worth naming separately from selling because
  the economics differ — for day labour the platform's cut *is* the
  margin, so removing the operator is the whole product rather than a
  nicety.

- **Group messaging.** See below; the mechanism is already proven here.

## Group messaging — the roster pattern, generalised (after 1.0)

- **Small groups over pairwise threads.** A ceremony is already a group:
  §17.9's roster of two or three personas, coordinated entirely over
  pairwise threads, with round 0 carrying the roster because "a pairwise
  thread only names two of the parties and the third has to learn who
  else is in the room from the invitation itself." Group chat is that
  pattern carrying words instead of DKG rounds — a roster message, then
  fan-out: the sender writes the same body into each member's existing
  thread.

  **Why fan-out rather than a shared record.** A shared DHT record is one
  write instead of N and is the obvious design, and it costs three things
  that matter more. It needs a group key, which means key rotation on
  every membership change and no good answer for removal — the removed
  member keeps the key and the record's location. It needs a writer
  secret shared among members, so any member can overwrite any other's
  subkey. And it is a new object on the network that says *these N people
  are a group*, where fan-out adds no metadata at all: the pairwise
  threads already exist and already carry traffic. Fan-out also keeps
  every property the thread already has — the prekey partitioning of
  §16.11, forward secrecy per pair, deniability — for free, because it
  changes nothing about how a message is sealed. Signal reached the same
  answer for the same reasons.

  **What it costs, stated plainly.** N writes per message, and N² across
  a group if every member is talking. That bounds this at *small*: a
  household, a stall's two staff phones, the three people organising a
  thing. Not a channel with five hundred people in it — that is a
  broadcast medium and wants different properties than a conversation.
  Worth deciding whether that bound is a limitation or the right shape.

  **The hard parts, none of them cryptographic.** Ordering: pairwise
  chains give per-sender sequence and there is no global clock, so
  concurrent replies arrive in different orders on different phones and
  something has to decide whether that matters. Membership: who may add,
  and what removal means when nobody can un-tell a person something.
  History: somebody added on Tuesday has no record of Monday, and the
  honest answer may be that they simply do not get one. Consistency: two
  members disagreeing about the roster silently drop each other's
  messages, which is the failure mode to design against first.

  **The commerce shapes may matter more than the social one.** A tab split
  across three people, a shop whose two phones share a till, a household
  paying one subscription. Those are groups with money in them, which is
  where this differs from every other group chat.

## Driver mode — navigation, from OsmAnd (after 1.0)

- **Turn-by-turn for the driver, lifted from OsmAnd.** A driver who has
  accepted a fare needs the route, not a map with a pin on it. OsmAnd is
  GPL-3.0 and Android, so its routing and guidance can be taken rather
  than rebuilt — which is the only reason this is tractable at all.
  Deliberately *after* 1.0: it is a large dependency to absorb, none of it
  is protocol, and every part of the ride that involves money already
  works without it. Sequenced behind the field day, because what the field
  day teaches about a real driver's hands is what should shape this.
  Notes for whoever picks it up: check the licence direction (GPL-3.0 is
  stronger copyleft than this repo currently carries — this may force the
  app's own licence, and that is a decision, not a detail); offline
  routing is on the not-for-1.0 list below and this would supply it, so
  the two should be read together; §15.12's live position (specified, not
  built) is the piece that makes a driver's screen worth looking at, and
  belongs first.

## Explicitly not for 1.0

- Offline OSM routing (fare estimates without the one stated leak).
- Multi-hail per rider; fleets; anything dispatcher-shaped.
- Reputation systems beyond the receipts a relationship accretes.
- **A second settlement chain. Considered and declined, 2026-08-19.**
  Bitcoin and Ethereum alongside Monero, three wallets from first launch —
  weighed for reach and turned down. The engineering was the small part:
  only twelve functions cross into the Monero bridge, so the seam is
  narrow. Four things decided it, none of them the code.

  Privacy is not an implementation detail in this design, it is the
  product. §15.10's subaddress-per-contact gives attribution *by
  construction*; on a transparent ledger that inverts and everyone can
  attribute — a shop's takings and a rider's fare history become public,
  and the receipt stops being the record of a payment and becomes a
  private annotation on a public one. Worse, the app's privacy story would
  become "depends which button you pressed", which is the hardest kind to
  tell honestly and the kind people get wrong exactly when it costs them.

  Fees kill the counter modes structurally: a £3.20 coffee on either L1
  can cost more in fee than the coffee. Lightning and L2s answer that and
  each is a different protocol with different escrow primitives —
  Lightning in particular wants both parties online with funded channels,
  which is the opposite of the mailbox's whole reason to exist (§16.12).

  Escrow would triple in the part where mistakes are unrecoverable. And
  node access reintroduces an operator: Monero light scanning through a
  public node leaks comparatively little, where an Ethereum RPC is asked
  "what is the balance of 0xabc" and in practice is a company.

  If this is ever revisited: Bitcoin, not Ethereum — it shares the UTXO
  model and has native multisig, where Ethereum is account-shaped (no
  outputs, no notes, no `SendPlan`) and drags a contract platform behind
  it. And chains should be created lazily, never three at first launch:
  §4.3's backup is already the most frightening part of onboarding and
  nobody should be asked to keep a seed for a chain they never used.
