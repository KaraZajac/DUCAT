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
  — the release is theirs to broadcast"). **Left:** (1) the arbiter-set
  role (2-of-3); (2) the rest of the bond UI — co-signer consent needs a
  payments accessor on monero-wallet's SignableTransaction (0.2.0 keeps
  them private; until then the co-signer sees only the fee), plus bond
  amount and funding flow.
- **Opt-in live location after commitment.** Rider and driver may share
  positions *after* mutual acceptance, never before, off by default,
  clearly bounded to the ride. Presence streaming is a different threat
  model than one-shot fixes — spec it before building it.

## Privacy — spend it only where it buys something

- ~~**Monero subaddress per contact.**~~ **Done, 0.87** — every contact
  gets subaddress (0, minor), allocated once (cards pre-allocate; the
  claimant adopts); every request/tab/handshake address is per-contact;
  all three scanners watch every allocated minor; outputs record their
  receiving minor, and tab reconcile refuses an output that landed on
  someone else's — attribution by construction, not by believing a note.
- **Storage encryption for the message store.** Threads, receipts, and
  contact state sit in SharedPreferences plaintext; a lost phone is a
  transcript. Android Keystore-wrapped encryption for the store files.
- **Profile-wide privacy pass.** For every profile field: who needs it,
  at what moment, over which channel. Car/plate scoped to the hail claim
  was the pattern; apply it everywhere.
- **Backup hygiene for device-local state.** `stuck_`/`slotseen_` slot
  memory and the burn pen are *this device's* transport state — restoring
  them to a new device would mislead its reader. Audit `appStateKeys`
  so backups carry identity and history, never transport scars.

## App robustness — the 0.85 review's unfixed tail

- **State survives rotation and process death everywhere.** The review
  found zero `rememberSaveable` outside the camera fix: a rotation
  mid-sale abandons a POS card a customer may have scanned; PaySheet
  loses target/amount/memo; onboarding can regenerate the wallet being
  backed up. Sweep the screens, persist in-flight sale state next to the
  tab record.
- ~~**Bill cancellation tracking.**~~ **Done, 0.87** — a vendor cancel
  sends RETRACT(re_own) naming the bill; the request bubble renders
  "Cancelled" instead of a live Review payment button.
- **Poller cadence and battery.** The 3 s claim-poll and 4 s board sweep
  are field-test numbers. Measure, then tier: hot when the screen is on
  and a hail is standing, slow otherwise.

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
- **Known gaps.** Pronoun labels come from the bridge's pronounOptions()
  and need a mapping layer; notification text keeps the process-start
  language until restart (attachBaseContext runs once per process);
  outbound chat bodies ("Meter started…") localize to the *sender's*
  language by design — the receiver sees the sender's words, like any
  message.

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
