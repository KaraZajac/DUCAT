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
  before advancing. The arbiter-assisted *dispute* release (arbiter
  co-signs when a principal is gone) is engine-ready, UI later. **Left:**
  the rest of the bond UI — co-signer consent needs a payments accessor on
  monero-wallet's SignableTransaction (0.2.0 keeps them private; until then
  the co-signer sees only the fee), plus bond amount and funding flow.
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

## Validation — before the number says 1.0

- **NFC tap, live.** §15's core gesture has never been tested on
  hardware. One field day: tap-to-pay, tap-to-contact, the §15.5 confirm.
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
   `:desktop:smoke` gates the stack in CI. Next: bills/pay rendering,
   wallet scan loop, notifications, then packageDeb/Msi/Dmg artifacts.
   The shim retires into a real shared module once the surface is known.
3. **iPhone** eventually: uniffi generates Swift bindings natively and
   the Rust stack compiles for iOS — the protocol layer is free; the UI
   and App Store review are the cost. Nothing now forecloses it.

## Explicitly not for 1.0

- Offline OSM routing (fare estimates without the one stated leak).
- Multi-hail per rider; fleets; anything dispatcher-shaped.
- Reputation systems beyond the receipts a relationship accretes.
