# Road to 1.0

What stands between the specification and a release that strangers can trust
with money. Ordered by what blocks 1.0, not by effort. The frozen line is
**1.0.0-rc1**; master is the 1.1.0 branch and reads draft **1.1.0-dev13**
today, which is why several things below landed after a freeze that still
holds.

## The freeze (2026-08-30, spec 1.0.0-rc1)

**What is frozen is the 1.0.0-rc1 line, and it is still frozen**: nothing
1.0.0 will carry has changed on the wire since. Master is not that line.
Master is the **1.1.0 branch**, where the post-1.0 track is built, so the
wire *on this branch* has grown — and the two must not be read as one.
`README.md` says it the same way, in its header and in the repository table:
"1.1.0-dev13 on this branch (1.0.0-rc1 is the frozen line)".

What the branch has added since the freeze, per the changelog at the top of
`ducat-protocol.md`: `BURN_PROOF` (object type 28) and `ATTESTATION` (type
12, registered since 0.47 and finally given a shape) in dev11; `VOUCH` (type
29), `RENTAL_NOTICE.min_burn` (field 320), `SLASH_CLAIM.claimant` (321) and
`INTRODUCTION` (message kind 17) in dev12; §15.5.1's two policy fields on a
backup (keys 28–29, optional on the way in and defaulting to the stricter
reading) in dev13. Every one is an addition rather than a change — a new
object type, a new optional field, a new message kind — which is minor-version
territory for the reason `research/post-1.0/REPORT.md` gives, and is why the
frozen line is untouched by any of it. One thing on the branch is *not*
additive and should be read as what it is: dev13 rewrote §15.5.1 — the tiers
and the four non-relaxable rules stand, but the shape a client SHOULD ship
is inverted, and a terminal with no credential of its own now **MUST NOT**
treat that absence as satisfaction. It touches no wire object, by design
(§15.5.1's last subsection is titled "This never touches the wire"), so the
freeze is intact; but a phone built to rc1's §15.5.1 and one built to
dev13's do not ask the same thing in front of a payment. See the counter
section below.

What remains between rc1 and the number is validation, not construction, and
two of the three gates are calendar-shaped — start them first:

1. **The adversarial review** (the long pole — commission now). Scope as
   §2.5 has always implied: the §17.9 ceremonies, the §16.12 mailbox and
   card surfaces, the boards and the §16.18.1 beacon ~~(newest, least
   reviewed)~~. **Newest and least reviewed is now the trust layer (§9.5,
   §9.2), which has had no pass at all**: the internal five-surface review
   of 2026-09-07 (`research/security/2026-09-07-adversarial-review.md`) was
   read against draft dev9 and predates every line of it. The boards and
   the beacon did get that pass; what it found there is either fixed or
   recorded as a deferral in the ledger.
   `docs/review-brief.md` says all of this itself, scopes the trust layer
   as its section 5, and is the package to hand over.
2. **The field day** (needs two NFC handsets). `docs/field-day.md` is the
   run sheet; the tap, real GPS, a position that actually moves, §15.5.1's
   keystore-backed unlock window, a burn checked against two reachable
   Monero nodes, and the OEM restore picker are the parts of the system no
   emulator has exercised.
3. **O21's reader** — an implementer who builds from the document alone.
   Recruit alongside the reviewer; the same kind of person often fits both.

Decided at the freeze, recorded in the rc1 changelog entry: **refunds** are
a documented limitation for 1.0 (the design question — what the payer's
Activity should show when money comes back — stays open, deliberately
unanswered rather than answered badly). The other limitation recorded there,
the **co-signer's blind payment list**, ~~waits on monero-wallet 0.2.0's
accessor rather than a fork~~ **closed, 2026-09-14** — without the accessor
and without a fork. `read_tx` in `mobile/src/ceremony.rs` walks the crate's
own re-encoding of the very transaction that is about to be signed, so no
second, laxer parser can disagree with `SignableTransaction::read`; a
co-signer now sees each destination, what it is worth, the inputs' total and
count, and the fee. `Ceremony.readRelease` refuses outright any proposal it
could not honestly describe — a third output, a nameless one, the same
address paid twice, arithmetic that does not close. The same pass sized
consent from the transaction rather than from the scanned balance, which is
the defect that mattered (commit `e2ab23c6`). `docs/review-brief.md` still
lists the old state under "what we already know is wrong" and wants the
correction before it is handed to anyone.

~~The post-1.0 track (personas, the sign-in doorway, publications with
period keys, the swarm engine) is sketched in `research/post-1.0/` and stays
off master until rc1 ships.~~ **It landed on master instead**, as the 1.1
branch rather than as a wait: personas (`PersonaStore`, the drawer's
switcher, the desk's Me page; `MAX_PERSONAS` is 4, because few, named and
visible was the whole design),
publications whose period keys are *derived* from one master secret rather
than accumulated in a keyring (`core/src/publish.rs`,
`app/src/publications.rs`), sites and homes (`app/src/sites.rs`, §16.22 and
§16.23), and the swarm engine (`mobile/src/swarm.rs`), which now carries a
listing's gallery bundle (`app/src/listings.rs`) as well as a heavy
publication period. **The one piece still unbuilt is the sign-in doorway**
(`research/post-1.0/REPORT.md` §1.1b): no `login` handshake purpose exists
anywhere in the tree, and nothing serves the HTTP face a site would mint
cards from. What would close it is that server piece — a standing watch that
mints from a pre-minted pool and maps card to session — plus a claim confirm
that names the purpose and the site out loud, which is also the only honest
mitigation for QR relay phishing.

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

There are two answers and for most of this file's life it recorded only one.
Escrow makes cheating unprofitable *inside* a deal; it says nothing about
whether to enter one, and a persona is free to mint from a hash. The second
answer is §9's own and it is built now: make an identity cost real money, and
let the reader weigh what it is shown — never a server, never a score.

- **Costly identity — proof of burn, rated receipts, vouches (§9.5, §9.2).**
  **Built on both clients and walked on the live chain, 2026-09-14.** A
  persona sacrifices XMR and proves it: `BURN_PROOF` (object type 28, fields
  304–309) carries the transaction, the amount, the block and Monero's
  `OutProofV2`, whose **message** binds `"DUCAT-BURN-v1"`, the persona key
  and a purpose — so a proof lifted onto a second persona verifies only if
  the message is rewritten, which breaks the signature. The burn address is
  a constant of the protocol that nobody holds a key for: each half is
  Monero's `hash_to_ec` of a keccak of a fixed label, so anyone can recompute
  it and see that no discrete logarithm was ever chosen. Proofs are made and
  checked in `mobile/src/txproof.rs`, proven both directions against
  `monero-wallet-rpc` on a real stagenet burn, and a wrong message fails both
  ways. The reader's rule is §9.5's three questions in order, each cheaper
  than the next: the message must name the persona presenting it; our own
  node must bear the proof out **for exactly the amount the proof proves**,
  never the amount claimed beside it; a second node must have that
  transaction in a block, where *unknown*, *in the pool* and *unreachable*
  all mean "not yet" and never "yes".

  On top of the burn sit the other two legs. §9.2's `ATTESTATION` is a rated
  receipt after a settled deal — signer, subject, the settled amount, a
  rating from a closed set, the transaction it stands on — and `VOUCH` (type
  29, fields 317–319) is the smallest signed thing in the protocol: *I know
  this persona*, and nothing else, because the less it carries the less it
  leaks. A seller may publish a minimum (`RENTAL_NOTICE.min_burn`, field
  320) and a buyer's client MUST show it before the buyer commits to
  anything. The wire objects and their refusals are `core/src/trust.rs`;
  the bridge signs and opens them in `mobile/src/attest.rs` beside
  `txproof.rs`, so both clients produce the same bytes; the clients are
  `app/src/trust.rs` (desk) and `Trust.kt` with `ui/Burn.kt` and
  `ui/TrustBadge.kt` (phone) — one badge builder, so the thread header, a
  listing's poster, a driver's offer and the till's customer row cannot
  drift apart. The plan all of this closes is `research/post-1.0/TRUST.md`
  §8, whose Phase 3 and 4 rows record each walk; the memos are
  `research/security/phase4-phone-*.md`.

  **Nothing is published and nothing is aggregated**, which is the property
  the whole design turns on. What a reader shows is its own arithmetic over
  envelopes handed to it in a sealed thread: one voice per signer however
  many receipts that signer wrote, counted only where that reader verified
  the signer's burn itself, and a vouch counted only when its signer is
  already a contact the reader holds. One hop, never two — a friend of a
  friend is a stranger with a story — and the answer is worn in words ("2 of
  your contacts know this person", "burned 0.05 XMR, since block N"), never
  as a number out of context. A burn is scored from the largest single burn
  rather than the sum, so many small identities cannot add up to one large
  one, and from the age of its first block, which is a birth certificate
  nobody can backdate.

  **Left:** the adversarial pass (above — this is the newest surface in the
  tree and has had none); the handset walk, which needs two reachable Monero
  nodes and is Pass 8 of the field day; and the forward direction of a rated
  receipt, which waits on a deal that produces a kind-3 receipt. Stated
  plainly rather than glossed: neither a burn nor a vouch is *required* to
  post anything, so a board defence is still a throughput speed bump. What
  changed is that a reader can now price the difference, and the open
  question for the reviewer is the economic one — what does it cost to farm
  a plausible history, and is a seller's minimum a wall or a kerb?

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
  human-written line, the judgment deliberately unautomated.
  ~~**Left:** the rest of the bond UI — co-signer consent needs a payments
  accessor on monero-wallet's SignableTransaction (0.2.0 keeps them private;
  until then the co-signer sees only the fee)~~ **Done, 2026-09-14** — the
  accessor never arrived and was not needed: `read_tx` walks the crate's own
  serialisation of the object about to be signed, so a co-signer is shown
  every destination, what each is worth, the inputs' total and the fee, and
  sizes the residual from the inputs rather than from a scanned balance. A
  proposal this device could not honestly describe is refused instead of
  narrated. **Left:** bond amount and funding flow.
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
  rule drift (O21 doing its job) and both implementations agree across
  the whole suite. **The 2-of-2
  accept flow and the settlement UI both had their live two-client pass on
  2026-08-25** — two emulators, live Veilid, live stagenet: a ride
  (`112e0983`), a marketplace sale (`284eb311`) and a two-day gear hire
  (`709f4d38`), each proposed on one client and signed on the other. The counter-offer ran on the same
  day (`74bc40b9`) and cost three wrong sentences to find. A counter to a counter followed
  (`ba8f17f6`). **Left for the field day:** the whole of it on hardware.
- **Opt-in live location after commitment.** ~~Spec it before building
  it~~ **specified, 0.88 (2026-08-16)** — §15.12 "Live position after the
  accept": gated on RIDE_ACCEPT, consent per ride per direction, off by
  default and never a standing setting; a record not messages (kind 11
  POSITION_REF, fields 218–219), one subkey overwritten in place — a now
  with no past — sealed under a fresh stream key with the record key as
  AAD, monotonic counter, fixed padding and cadence; bounded by client
  stop rules (receipt / RETRACT re_own / expiry) and record TTL; receiver
  MUST NOT retain the track. Position stays display-only. ~~**Left:** the
  build, once a ride to point it at exists on real hardware (field day).~~
  **Built, 2026-08-26** — `core/src/position.rs` seals the frame,
  `position_seal`/`position_open` carry it across the bridge, and
  `ui/PositionCard.kt` exists only once a `RIDE_ACCEPT` is in the thread,
  which is §5.2.3's gate: there is deliberately no standing setting for this
  anywhere in the app, because before the accept the same stream is a
  stranger-tracking primitive. Proven between two emulators — offered, read,
  rendered, aged honestly when the sender left the screen, released when the
  sender stopped, swept off both phones by the poller when the ride settled.
  **Left:** a dot that actually moves. `adb emu geo fix` reports OK and
  changes nothing, so both emulators shared one frozen fix all afternoon;
  whether four seconds is the right cadence on a real radio, whether the
  other phone's dot tracks a walk, and how long a fix takes indoors are Pass
  2 of the field day.

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
  resource files at the time; **1 893 strings and 30 plurals across 48 files
  today**, because every screen since has been born localized rather than
  retrofitted. Plurals used where count-driven; wire sentinels, state
  strings, and Locale.US parse formats deliberately left in code.
- ~~**Translations.**~~ **Done, 0.88** — nineteen languages: es fr de pt
  it nl ru uk pl tr zh ja ko ar fa hi id vi th. One values-<tag>/ mirror
  per screen file, and each mirror currently carries the full 1 893 strings
  and 30 plurals, all mechanically validated (placeholder multisets, key
  sets, sentinel dashes, plural quantities per CLDR) by
  `applications/check_strings.py`. RTL proven twice (ar, fa) with mirrored
  layout and native digits. Untranslated keys fall back per-string, so a new
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

## Small bugs spotted, ~~not yet fixed~~ all cleared

- ~~**"Break a note" card shows on a zero-balance wallet**~~ **Fixed,
  0.88** — the card now fires only when money exists (hasMoney/allLocked
  guards in BalanceCard); an empty wallet says nothing.
- **The pre-1.0 flow sweeps (2026-08-25/26)** cleared the tail of these
  as they surfaced: a hail card showing two contradictory statuses, a
  chat-list preview leaking "ceremony: called off", a consent line
  promising a 2-of-2 while building a 2-of-3, a driver quoted a fee-less
  figure, the marketplace's own back-button and dirty-form paths. Each is
  in git under its own commit; the list is no longer a standing backlog.

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
- **The spend gate** (§15.5.1). ~~A PIN in front of every spend, set during
  onboarding, with the phone's own lock offered where one is enrolled.~~
  **Corrected in 1.1.0-dev13 (2026-09-15)**, because the ladder could express
  the right policy and was set to the wrong one: it asked nothing below
  twenty dollars — a tier an attacker never has to clear — and asked for a
  secret above a hundred *once*, which a shoulder-surfer clears once and then
  spends through. The shipped shape is the one a phone's own payment app
  already has. The floor is **zero**: every payment wants the device unlocked
  inside a two-minute window, asked of Android as a window and not as an age
  (`DeviceLock.authenticatedWithin` — a keystore key bound to a recent
  authentication, which throws once the window lapses), and a device with no
  secure lock screen establishes nothing and falls back to the secret on
  every payment. At or above one threshold the app's PIN is asked **every
  time**, not once. That threshold is the single number a user sets, and it
  lives in drawer → Profile → Spending (`SpendLimitSetting` in `Drawer.kt`),
  denominated in the currency they price in — never piconero, which would
  quietly turn a "$100 limit" into a $70 one the next time the price moved.
  Onboarding still *chooses the PIN*, which is the one thing in setup a
  backup cannot carry; it no longer sets the limit. A rolling hour counts
  too, since a per-payment limit alone does not stop twenty payments just
  under it, and a stale exchange rate escalates to the top tier rather than
  relaxing anything — failing the other way would let anyone able to stall a
  rate feed *lower* the requirement. The arithmetic is `core/src/verify.rs`;
  `SpendGate.kt` supplies only the three things core cannot know. None of it
  touches the wire: a payee never learns which tier was satisfied and cannot
  ask for one, which is the downgrade attack EMV spent years patching.

**Left:** refunds. There is no path to give money back after settlement —
`cancel` withdraws a bill before payment, `markPaidOutside` records
another rail, and neither is a refund. The open question is what the
customer's Activity should show when money comes back, which is a design
question before it is a build.

## The local board — marketplace, hire, and any rental (SHIPPED, 0.88–0.89)

One board, several nouns. A room, a car, a kayak, a bike to sell, an
afternoon of an electrician's time — all the same act: somebody says what
they have, near where they are, and somebody nearby answers.

**All five kinds ship and are chain-proven.** Places and vehicles (the
reservation), gear (`KIND_GEAR`), a sale (`KIND_SALE`), and skills by the
hour (`KIND_SKILL`) — one board name `local:$cell` carrying a kind field.
Marketplace sale `284eb311` and gear hire `709f4d38` ran two-client on
2026-08-25; a full sell→enquire→reserve→settle purchase ran again on
2026-08-26 (release `34ffbcfd`) as part of the pre-1.0 sweep. A listing
carries a per-listing quantity (0.88), an Argon2id + Monero-beacon stamp
(0.89), pictures (dev8/dev9, above), a seller's minimum burn (dev12,
field 320), and survives backup/restore (0.89). What is left for the board
is the field day (real GPS, real boards) and the adversarial review.
~~The beacon surface is the newest and least-reviewed part of it.~~ The
beacon has since had the internal pass and one fix out of it (a beacon
mismatch is `Unknown` until a second node seconds it — W10, 2026-09-14);
the newest and least-reviewed surface in the repository is now the trust
layer above, which has had none.

The design constraints below held; they are kept as the record of what the
shape forbids, not as open questions.

**What this is for, and what that forbids.** This is meant to put people
in front of each other — a village notice board, not a feed. That is a
design constraint with teeth, and these follow from it rather than from
taste:

- No infinite scroll, no ranking, no algorithmic order. A board is a
  place with things on it, in the order they were posted.
- No engagement metrics, no "people also viewed", no notification whose
  purpose is to bring somebody back. A notification here means a person
  answered you.
- Nothing that rewards posting more. A listing is a thing somebody has,
  not content.
- The transaction ends in a meeting. In-person handover is the design,
  not a fallback for when shipping fails.

The read costs happen to enforce this. A populated board read is ~1.1 s and
an empty one a flat 21 s — Veilid giving up rather than searching, two
chained internal timeouts. ~~Boards do not parallelise.~~ **They do, since
2026-09-01**: `mobile/src/node.rs` raises veilid's
`network.dht.max_concurrent_operations` off its default of 16 (`DUCAT_DHT_OPS`,
24 today, measured as high as 72), which took a nine-board empty ring from
66 s to 42 s, dead-repeatable. What is left is a floor of two chained 21 s
verdicts inside veilid's own get, reachable only by patching veilid-core —
which is now vendored (`mobile/vendor/veilid-core`, carried for the fanout
crash) and therefore possible rather than hypothetical. The conclusion is
unchanged and is the point: a neighbourhood sweep is tens of seconds, so an
endless feed is not available even if somebody wanted one. What is available
is "what is near me, one board, cached, refreshed behind the screen" — which
is the thing being aimed at anyway.

- ~~**Any rental — gear (`KIND_GEAR`).**~~ **Shipped.** Reused
  `Stakes.Deal.Vehicle` (its docstring already read "a vehicle **or
  equipment**"), a third button on the form minus year and seats. Proven
  `709f4d38`.

- ~~**Local marketplace — selling a thing (`KIND_SALE`).**~~ **Shipped.**
  The one with a genuinely new escrow leg: a sale has no deposit and
  nothing comes back, so it got its own shape — price plus a stake each
  side, released on handover — and its own `Stakes.Deal`. Proven
  `284eb311`, and again `34ffbcfd` on 2026-08-26.

- ~~**Hire help — skills by the hour (`KIND_SKILL`).**~~ **Shipped.**
  Priced hourly, released on both saying the work is done, reusing the
  ride's split release. Skill listings carry no quantity (an hour of one
  person's time cannot be stocked — core refuses it).

**Decided:** one board name (`local:$cell`) carrying a kind field, not one
board per noun — every extra board name multiplies the read cost above,
and the reads are the binding constraint.

**Settled in the building:**

- *Categories.* Landed as a small flat set per noun plus free text —
  §16.18's `RN_SUBTYPE`, with per-kind tops pinned in the vectors and
  `audit_spec.py` holding the §16.18 category table to the code. A coarse
  filter, not a directory, exactly as leaned toward.
- *Modes or listings.* Listings, as argued — Marketplace/Renting/Hire are
  browsing shells over one board, and a listing stays true while its owner
  sleeps. The browsing-vs-shift distinction that fell out of this is its
  own robustness fix (0.88, the leave-arrow shells).
- *The sale escrow's abandonment rule.* A sale where nobody turns up has
  no asset to point at and neither party at fault, so it leans on the same
  mutual-stake burn as a ride: releasing beats sulking because both stakes
  are hostage, and an unanswered build goes stale on its own (30 min) with
  nothing at risk. Still the softest corner of the shape, and named for the
  reviewer.

**Sequencing note (met).** Placed before 1.0 by decision (2026-08-19), on
the recorded risk that each new money surface had carried a real defect in
the 0.88 sweep. That risk paid out exactly as feared and exactly as
intended: the 2026-08-25/26 sweeps found and fixed a dozen defects across
these flows (counter role-swaps, the fee attribution, the fare-arrival
race, the settled-without-us gap, the backup omission) *before* the review
rather than during it.

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
  harness. (The software half is proven on emulators end to end; what the
  day adds is real GPS and two radios — see `docs/field-day.md`.)
- ~~**Backup / restore round trip.**~~ **Proven off-hardware, 0.89
  (2026-08-26)** — full wipe-and-restore on two emulators: persona,
  contacts, threads, listings, till, wallet address and post-rescan
  balance all returned, and the restored phone sent and received once its
  fresh Veilid identity attached. The hardware re-run is folded into the
  field day (**Pass 9** — it is run last because it wipes a phone) because
  the OEM file picker and share sheet are the only untested part. One thing
  to set before the export and check after the import: the §15.5.1 spending
  limit, carried in a backup since dev13. It is the failure that hides — the
  default is *stricter*, so a restore that dropped it looks like nothing
  until a payment that never used to ask for a PIN asks for one, days later,
  with nothing on screen connecting the two.
- **The §15.5.1 spend gate on hardware.** The two-minute rung is a keystore
  key bound to a recent authentication, so it cannot be exercised on an
  emulator with a swipe lock — the one rung of the ladder no test here has
  ever really run. `docs/field-day.md` carries the three rungs as part of
  **Pass 1**: a small payment on a phone unlocked seconds ago must ask for
  nothing, a payment after the window lapses must ask, and a payment over
  the limit must ask every time including twice in a row.
- **The trust layer's own adversarial pass.** §9.5's burns and §9.2's rated
  receipts and vouches are the newest surface in the tree and the only one
  with no review of any kind: the internal five-surface pass was read
  against draft dev9 and predates all of it, which
  `docs/review-brief.md` records under what is already known to be unproven.
  The handset half is **Pass 8** of the field day — a burn, a stranger's
  check against two nodes, a rated receipt, a vouch. The half that needs a
  person rather than a day out is the economics: what it costs to farm a
  plausible history, and whether a seller's `min_burn` is a wall or a kerb.
- **External adversarial review.** §2.5 still says "no adversarial review
  whatsoever," deliberately, and that is still literally true — the
  2026-09-07 pass was internal, by the people who wrote the thing, which
  tells you what they thought to look at and nothing more. 1.0 is the moment
  the deferral becomes a gap. Scope: the spec's crypto ceremonies, the
  board/mailbox surfaces, and the trust layer above.
- **O21's last gap.** An implementer who has never read `core/` builds
  from the document alone. Everything accidental is cleared; what remains
  is finding that person.

## The desktop client (parallel track, kara 2026-08-15)

An Electrum-shaped DUCAT client for Linux/Windows/Mac. The protocol stack
is already cross-platform Rust. ~~The path, cheapest first:~~ **It arrived
on 2026-09-05, and it is not the module this section was written about.**

**The desk is `applications/desk/`: a Tauri window over `app/`.** The logic
is `ducat-app` — identity, contacts and the mailbox, wallet, till, kiosk,
library, groups, boards, listings, the ledger, backup, sites — ordinary Rust
tested with `cargo test -p ducat-app`, with the window a thin set of
commands over it (one call each, `src-tauri/src/lib.rs`). Chat, Wallet,
Till, Kiosk, Activity, Library, Market, Files, Sites, Feed, Status and Me
are the pages.
It speaks the phone's nineteen languages out of the phone's own resources,
plus `desk_` keys of its own that can never shadow a phone key. It carries
the trust surface: burning, checking somebody's proof, rating them after a
deal, vouching, and a listing's minimum. Calls use the machine's real
microphone and speaker through the sound server's own tools. A site opens
in a sealed room — `script-src 'none'`, no IPC, every request answered from
the fetched bundle. `release.yml` packages it on four targets (linux-x64,
windows-x64, macos-arm64, macos-x64) beside the phone's APKs, which is the
packaging item 1 below wanted and never got on its own terms.
`src-tauri` is deliberately its own Cargo workspace, because
it needs webkit to compile and a machine without webkit must still be able
to `cargo test --workspace` on everything else. The live-network exercises
ship as examples of the crate (`app/examples/`), and `DUCAT_DESK_DRIVE`
evaluates JS inside the page — which exists because a Wayland session
ignores pointer warps, so nothing outside the window can click it. Not on
the desk: rides in any seat, which need a phone's position. The path that
got here is kept below as the record.

1. **Harness → CLI client.** Multi-contact state landed (--contacts,
   --contact-save, DUCAT_CONTACT selects the thread; --geo for board
   names), and `--card-watch` / `--hail-watch` are the standing watches.
   ~~Still wanted: a card-issue flow with a QR on the terminal, a
   persistent watch daemon (one process, all threads), and packaging
   (static binaries for the three OSes).~~ **Overtaken by the desk** — the
   harness stayed what it is best at, an end-to-end check between two real
   nodes over real routes ending in real settlement, and the client work
   went where somebody would actually use it. Packaging is the desk's now.
   The watch daemon survives as a want, but as the sign-in doorway's server
   piece rather than as a CLI.
2. **GUI — the Compose `:desktop` module ("DUCAT Desk" as it then was).**
   ~~Building.~~ **Superseded as the client, 2026-09-05, and kept as the
   harness-and-field-day runner** — seventy-six runnable `:desktop:*` tasks
   are how this repository proves things against the live network and a
   real phone (`taptest`, `kiosktest`, `ridetest`, `tilltest`,
   `backuptest`, `boardbench`, `arbiter`, `rendertest`, `shimtest`,
   `exiftest` …), and they are cited all over this file. The history below
   is why they can be: it is the same protocol sources the phone runs. v2
   compiles the
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
   `ducat-linux-x64.tar.gz` builds today. Deb/Rpm need host
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

## What else this shape is for

Filed ~~(after 1.0)~~ when the machinery only mostly supported these, and
all three have since been built. The reasoning below is kept because it is
what the shapes must not become, not because the work is outstanding. The
marketplace, hire and gear rental that were also here moved above: they are
pre-1.0 by decision.

- ~~**Subscriptions with no card on file.**~~ **Done** — recurring bills
  2026-08-27, publication subscribers 2026-09-05. A weekly box, a monthly
  dues: the seller bills the thread on a schedule, the buyer taps approve,
  money moves. What is *absent* is the point — there is no stored payment
  credential, so nothing to leak in a breach, nothing to charge after a
  cancellation, and nothing that makes cancelling harder than subscribing.
  Every recurring relationship today rests on the merchant holding a key
  to the customer's money; this is recurring billing with no recurring
  authority, which has no equivalent anywhere. ~~Needs a schedule on a tab
  and a standing thread; it needs no new protocol.~~ It needed no new
  protocol and got none: each due date the poller re-mints the same
  powerless §16.13 bill and the payer approves that one
  (`app/src/recurring.rs`, `Recurring.kt`, cadence advancing from the old
  due date and monthly by calendar), and a publication bills every
  subscriber one tab each per period (`app/src/publications.rs`), the
  period's key riding the reconcile loop when the tab settles.

- **Group messaging.** See below; the mechanism is already proven here.

- ~~**Pictures on a listing — the Uber/Airbnb/eBay shape.**~~ **Shipped —
  §16.18.3, draft dev8 and dev9 (2026-09-06).** The thing every comparable
  marketplace has and this one did not, and the last big gap between "a
  board that works" and a flow a stranger recognises. The transport was
  already built: §16.15's second road carries an attachment
  over a swarm share, which is how a photograph outgrows a record without
  outgrowing the thread, and `Enquiries.About` already kept `listingId`
  for exactly this class of thing — "the address and the key handover,
  which live on the listing and never on a board, offered once there is a
  booking to give them to". Photographs went in that same slot.

  **The prerequisite is done (2026-09-02).** Images now leave as pixels:
  `SafeImage.stripped` decodes and re-encodes so no EXIF survives, with
  the orientation baked in first, and `exiftest` guards it in CI. That had
  to land before any of this, because a phone writes GPS into a photo to
  within a few metres and §16.18 spends its whole design putting a listing
  on a board at about five kilometres.

  **What was decided.** Both, split by who pays for them. **The thumbnail
  rides the notice** (field 287, at most 10 KiB, PNG/JPEG/WebP, checked the
  way an avatar is because a decoder handed bytes whose format it must guess
  is how a picture becomes an exploit): it arrives with the read the browser
  was already doing, costs no second network operation, and still shows when
  the seller's phone is off. The cap is sized against the *sweep* and not the
  picture — eighteen boards of eight slots is 144 notices, so 10 KiB apiece
  is ~1.4 MB a lap against 45 kB of words — and it is refused rather than
  trimmed at both edges, because a slot is signed over its bytes and a reader
  that quietly accepted an oversized one would be verifying something no
  other reader would. **The full-size gallery rides a swarm share** (288–289,
  share key and index digest, both halves or neither), fetched only when
  somebody opens the listing; since dev9 that share is a bundle, and
  `listing.json` carries a description of at most 8 000 characters in
  §16.23's text subset, **at most 24 pictures**, 8 files and 32 spec pairs,
  and the price as the seller typed it. Nothing in the bundle can change the
  price, the area or the card, which stay on the signed notice. Two
  consequences a client must say out loud, and both do: a swarm serves only
  while somebody seeds it, so a seller whose device is off has a gallery that
  does not load — the thumbnail still does, which is why it is on the board;
  and fetching a gallery is a peer connection to the seller's node, so
  **opening a listing's pictures tells them somebody is looking** where
  reading the board tells them nothing. A client MUST NOT fetch galleries
  while browsing, and SHOULD NOT prefetch on the reader's behalf.

  **One loose end, in the document rather than the code.** §16.18 still
  carries the unqualified sentence "A client MUST NOT put an exact location,
  a registration plate, or a photograph on a board", which §16.18.3 now
  contradicts by design. The rule that was meant is the one the research
  below argues for — the *object*, never the interior and never the face —
  and §16.18's sentence wants rewriting to say that, with the plate and the
  exact location left exactly as absolute as they are. `audit_spec.py` cannot
  catch this one: it holds tables against code, not prose against prose.

  The research behind that, so it does not have to be done twice
  (2026-09-02): **Airbnb** shows listing photographs publicly and hides
  only the address until booking, for §16.18's own stated reason — an
  empty home whose address is known is a target. **Uber and Lyft** show
  the driver's photograph, car and plate *after a driver accepts*, never
  publicly, and expire them (photo at 48 hours, plate at 30 days) — which
  is what this app already does with fields 210–212 on `CONTACT_ACCEPT`,
  so rides are aligned already. Against putting them on a board: interior
  photographs are a documented burglary vector — rings researching targets
  through Zillow, investigators routinely finding those searches on
  suspects' phones, listing photographs showing every room, every entry
  point and the location of the cameras, and staying online for years. A DUCAT
  board is worse placed than Zillow for that, not better: no operator to
  take anything down, and mirrors keep serving. For it: in peer-to-peer
  resale the photograph is the primary trust signal, and the research is
  specific about why — *"the photograph carries more weight because the
  seller is often unknown"*, which is this app's permanent condition,
  with no reviews and no operator to fall back on.

  It was opened up, and in the shape that paragraph pointed at: the *object*
  goes on the board at thumbnail size for goods, gear and vehicles, the rest
  of the pictures sit behind a fetch the reader chooses, and interiors, faces
  and plates still travel in the thread — fields 210–212 on `CONTACT_ACCEPT`
  for the driver's plate, car and photograph, which is Uber's own answer
  arrived at independently. Person photographs do not yet expire the way
  Uber's do; that remains the one part of the recommendation unbuilt.

## The everyday-money tail — five features one Ask surfaced (built 2026-08-27)

All client-side, none touched the wire; each earned its shape from a rule
already in the spec. **Chat search** — bodies and attachment names across
every thread and group (receipt item lines live in bill bodies, so "where's
that address / what did I pay for the cortado" both resolve); name matches
rank first and carry no timestamp — a place, not a moment. **Sales tax** —
one rate in basis points behind a Settings switch; POS computes the line
per basket, a standing tab picks it up at settle, rounding DOWN (the till
may undercharge a piconero, never overcharge). **CSV statement** — the
settled ledger as a file (ISO-8601 UTC, plain-ASCII decimals, oldest-first
so the running balance sums; deliberately no fiat column — no historical
rates are stored and today's rate would print numbers never true on the
day). **Split a bill** — the group screen mints N ordinary pairwise
requests plus the sentence announcing the arithmetic; money stays pairwise,
shares round DOWN (splitter eats the dust), and the asker's bubble flips
Paid ✓ on the §16.14 reference only — never the amount heuristic, which
errs safe on a spend button and lies on the asker's screen. **Recurring
bills** — a schedule, not a mandate: each due date the poller re-mints the
same powerless kind-1 (§16.13), payer approves every one; cadence advances
from the old due date, monthly by calendar; store in securePrefs.

## Group messaging — the roster pattern, generalised

**Built, 0.90 (2026-08-27) — pulled forward once replies existed.** §16.19:
fan-out as sketched below, four wire fields (253–256), kind 12 for the
roster. The two decisions that made it buildable without an operator:
the roster is a **grow-only set** (anyone adds, nobody is ever removed —
removal needs a consensus a p2p group cannot have; a grow-only set
converges by union in any order), and the **mesh is checked edge by edge**
(contact edges are mutual, each end checks its own, every local check
passing *is* the mesh being complete — so receiving is never gated,
sending refuses while your own mesh has a hole, and partial delivery is
structurally impossible). Reactions, replies and unsend work in-group via
the group reference; money stays pairwise; the disclosure states the
shape plainly once per group per phone. Proven live between the two
emulators and the desk's shared poll loop.

**And then the shared record after all — §16.24 (2026-09-06). Fan-out is
the fallback now.** A group's words live on one DHT record per generation
with the SMPL schema: every member writes only their own ring of pages,
each write signed by their own key. So a message is one write instead of
N−1, a lap is one inspection instead of N−1 reads, a newcomer reads the
recent pages instead of nothing, and a sender can no longer say different
things to different members. `PAGES` is 4, a board holds at most 255
members, a page is bounded by the lesser of 32 KiB and 1 MiB ÷ subkeys
(`core/src/group.rs`), and the first member entry is a **nameplate** — the
SHA-256 of `"ducat group board"`, the group id and the generation — that
nobody holds a key for, so two groups of the same people are two records
and one group's two generations are two records even when the list did not
change. Readers merge by `(sender, GB_SEQ)` and order by `GB_TS` then
sender key, so two phones show one order and §16.19's arrival-order caveat
is gone.

The objections below were not wrong. They were **paid** rather than dodged,
and §16.24 writes down the price: there *is* a group key, so forward secrecy
becomes per-generation, and deniability goes, because the record signs each
page with the persona key a card already binds. What is bought is
consistency and history. Membership change is a new generation — a fresh
record, a fresh key — and the roster gains the generation, the owner, the
key and `PAGES` as payload keys 3–6; a roster without them describes a
§16.19 group with no board, and those keep working exactly as sketched
below, which is why the fan-out paragraphs stay rather than being deleted.
Money stays pairwise either way. Nobody is removed and leaving is still
local. One rule the security pass added afterwards: a reader MUST refuse a
generation whose member list is not a superset of the one it holds, whose
owner is not the sender, or whose number is not the successor of the one it
holds — otherwise one member can silently exclude another or brick the
board, which the union rule alone did not prevent.

The paragraphs below are the original sketch, kept because the reasoning
still governs what a group must not become — and, in the shared-record
case, because it is the list of what that choice cost.

- **Small groups over pairwise threads.** A ceremony is already a group:
  §17.9's roster of two or three personas, coordinated entirely over
  pairwise threads, with round 0 carrying the roster because "a pairwise
  thread only names two of the parties and the third has to learn who
  else is in the room from the invitation itself." Group chat is that
  pattern carrying words instead of DKG rounds — a roster message, then
  fan-out: the sender writes the same body into each member's existing
  thread.

  **Why fan-out rather than a shared record** (*answered by §16.24 above:
  the record was built, and each cost below was accepted and written down
  rather than disproved*). A shared DHT record is one
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
  the two should be read together; ~~§15.12's live position (specified, not
  built) is the piece that makes a driver's screen worth looking at, and
  belongs first~~ — **live position is built** (2026-08-26, above), so the
  piece that makes a driver's screen worth looking at is already there and
  this is additive rather than a prerequisite.

## Explicitly not for 1.0

- Offline OSM routing (fare estimates without the one stated leak).
- Multi-hail per rider; fleets; anything dispatcher-shaped.
- Reputation *systems* — a published score, an aggregate anybody else
  computed, a ranking, a reader who is handed the answer instead of working
  it out. §9.2's rated receipts and §9.5's burns arrived after this line was
  written and do not contradict it; what keeps it true is worth stating,
  because otherwise it reads as contradicted. Nothing is published and
  nothing is aggregated: a burn proof, an attestation and a vouch are
  envelopes handed to one reader in a sealed thread, and every verdict is
  that reader's own arithmetic — one voice per signer however many receipts
  they wrote, weighted only by signers whose burn that reader verified
  itself, a vouch counted only from contacts it already holds, one hop and
  never two, and the answer worn in words rather than as a number. A record
  is therefore never a score somebody else computed; it is envelopes, and
  every reader weighs them again.
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
