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
- **Density-adaptive cell precision (§15.12).** "6 where busy, 5 where
  empty" is a convention with no mechanism. A rule clients can compute
  alone (e.g. ladder height ≥ N sustained → post and watch at precision
  +1) so a stadium crowd converges without anyone coordinating.
- **Typed offer/accept ceremony for rides.** Today claiming a hail *is*
  the deal. The intended shape: claim = applying; rider sees the
  driver's offer (or counter-fare) and accepts; cards exchange on accept.
  Needs a small wire object and both UIs.

## Trust — the stranger problem, named since 0.82

- **Driver bonds through Part IV escrow.** The durable answer to no-shows
  between strangers. Deliberately unspecified so far; 1.0 should at least
  pin the ceremony shape even if the app ships without it.
- **Opt-in live location after commitment.** Rider and driver may share
  positions *after* mutual acceptance, never before, off by default,
  clearly bounded to the ride. Presence streaming is a different threat
  model than one-shot fixes — spec it before building it.

## Privacy — spend it only where it buys something

- **Monero subaddress per contact.** A single receive address across all
  contacts lets any two counterparties who compare notes link their
  payments to the same person. Subaddresses are the designed fix and the
  wallet layer already exists.
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
- **Bill cancellation tracking.** A cancelled bill's "Review payment"
  button stays live (the cancel is just text today). Needs a marker the
  request bubble checks — pairs naturally with the offer/accept work.
- **Poller cadence and battery.** The 3 s claim-poll and 4 s board sweep
  are field-test numbers. Measure, then tier: hot when the screen is on
  and a hail is standing, slow otherwise.

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

## Explicitly not for 1.0

- Offline OSM routing (fare estimates without the one stated leak).
- Multi-hail per rider; fleets; anything dispatcher-shaped.
- Reputation systems beyond the receipts a relationship accretes.
- Localization (the strings are English; the money already isn't).
