# Dispatch: strangers meeting by place, with no operator

*Research toward a p2p ride-hail ("Uber without Uber") on Veilid. Evidence,
not product. 2026-08-14, veilid-core 0.5.7, run against the live network.*

## The problem, stated against our own rules

Everything DUCAT connects today is people who **met**: a card crossed a table
or a screen. That is a stance, not an accident — no directory means no lookup
by strangers, no spam, no enumeration (§16.12). Ride dispatch needs the one
thing that stance refuses: a rider and a driver who have never met, converging
with nothing in common but *where they are*. Uber solves this with a server
that knows everyone's location. The question was whether Veilid can solve it
with nobody knowing anything.

## What the source says, and what running it adds

Verified in veilid-core 0.5.7 source, then exercised against the live network
(`harness/src/stand.rs`):

1. **Record keys are computable, not just learnable.**
   `get_dht_record_key(schema, owner_public_key, encryption_key)` derives a
   record key locally — no round trip. So a keypair derived from a *public
   string* (`seed = SHA-256("DUCAT-STAND-v0" ‖ cell)`) gives everyone who
   knows the string the same record key. The DHT becomes a map from **names**
   to **bulletin boards**. A geohash is such a name. So is "the taxi rank at
   the airport".

2. **`create_dht_record` accepts a supplied owner keypair** — so the derived
   keypair can own the record, and anyone who can derive it can write.

3. **The trap the source hid until runtime: values are encrypted, and the key
   never touches the network.** 0.5 encrypts every record's values; the
   encryption key rides in the RecordKey *handle*, and `create` always draws
   a random one ("Always create a new encryption key", create_record.rs).
   First run: the reader computed the right record and got
   `cannot decrypt value data: missing encryption key`. The fix is part of
   the convention: derive the encryption key from the cell name too
   (`SHA-256("DUCAT-STAND-v0-ENC" ‖ cell)`), have both sides construct the
   full key locally, and never use the create-time key. "Encrypted under a
   public secret" is a public board, which is the point.

4. **Watches on unowned records work** (the app already ships them), with
   per-replica limits worth designing around: **32 public watchers, 8 member
   watchers** per node holding the record (veilid_config.rs). A
   neighborhood's drivers fit; a city's do not — cells must stay small.

5. **SMPL schemas fix their member list at creation** (≤256 members). There
   is no open multi-writer record — which is why the board uses a shared
   owner key rather than membership.

**The spike, run for real:** one process posted a notice at
`"stand:us-dc-natl-airport-rank-2"`; a separate process, handed only that
string, derived the keypair, computed the record key, opened it cold on the
live DHT and read the notice back. Two processes met at a name. Post → read,
including two full node startups, under a minute; the read itself ~2s.

## The design this supports: stands

A **stand** is a named cell — a geohash prefix, or a human name for a place —
whose board anyone can compute. The register:

- **A rider posts a notice**: a freshly minted **claim-once card** (the §16.9
  machinery, purpose `"hail"`), a coarse area ("terminal B", never a
  coordinate), an expiry. Small, and worthless to scrape.
- **Drivers watch the cell** they are actually in. A notice arrives; a driver
  **claims the card**. Claim-once means exactly one driver wins the claim —
  the race is settled by the DHT, not by a matchmaker.
- **Everything else leaves the board immediately** for the claimed card's
  sealed thread: precise pickup, quote, ETA, haggling — all §16.11-encrypted,
  all off the public surface. The ride itself is the existing taxi mode
  unchanged: rate disclosed in-thread when the meter starts, one itemised
  bill, notice-nominated tip, receipt on chain settlement (§15.11).

Nothing new on the wire. The board carries card URIs; the cards carry
everything else; the money is the money we already have.

### The board is honest about what it is

The seed is public, therefore the secret is public, therefore **anyone can
write or wipe the board**. This is a bulletin board in the literal sense —
pinned in a public square, erasable by anyone with hands. What keeps a real
one useful keeps this one useful: notices are tiny and expire; the valuable
part (the conversation, the money) is sealed elsewhere; a wiped board is
re-pinned by the next person who needs it. A vandal buys minutes of nuisance
in one cell, not a network outage. This is a weaker guarantee than Uber's
dispatcher and it is the honest price of having no dispatcher; §18.7's
stewardship rules apply to boards doubly (short TTLs, delete local state when
the hail is spent).

### Privacy accounting

- On the board: a coarse cell name and a one-use card. Watching a cell tells
  you "rides are wanted here" — exactly what standing at the taxi rank tells
  you.
- Rider location precision is disclosed *after* mutual commitment (the
  claim), inside the sealed thread, to exactly one driver.
- The rider's persona on the board is fresh per hail if the client wants —
  cards are cheap, and an unlinkable hail should be the default.
- What this does not hide: a global observer of a cell's replicas sees write
  timing. Same class of exposure as §16.6 already accounts for.

## Proven / not proven

Proven: rendezvous by convention over the live DHT, cold, cross-process; the
full derived-key convention including the encryption half; descriptor
create/reopen dance.

Not proven: write contention on a busy board (last-write-wins on subkey 0 —
a real register needs per-notice subkeys and a compaction rule); vandalism
dynamics outside a lab; watch latency and battery cost on a phone parked in
a cell; the 32-watcher ceiling under load; cell naming (geohash level vs
named stands — probably both, stands for density, geohash for sparsity);
driver authenticity (a bond posted via the §8 escrow machinery is the
natural answer and ties into the parked bonded-regulars work, #4); and every
regulatory question, which this document does not pretend away.

## The other growth path, for contrast

Before the board existed, the design already had one dispatch mechanism:
**introductions** — cards passed through chats and scanned off screens. A
hotel bartender who knows three drivers *is* a dispatcher, with consent built
in. Stands and introductions compose: the board bootstraps strangers in
anonymous density (airports, downtowns); introductions carry the trust
network everywhere else. Uber needed one global machine because it had no way
to let a bartender vouch. We do.

## Postscript: the second ride (2026-08-14, afternoon)

The first ride worked and left scars — eight bugs, three receipt
attempts, placeholder holes in the thread. Every fix shipped the same
day, the rider deleted the thread, and the loop ran again from nothing:
hail posted at the stand, claimed in under a minute, quote sealed to a
one-time key from the *newborn* head (the step that failed cold last
time), acceptance, bill correct on the first send, payment with a 50%
tip confirmed on chain at block 2185465, and a receipt — one-time key,
full forward secrecy — rendering clean twelve minutes after the hail.

Zero placeholders. Zero fallbacks. No operator anywhere in the chain.
The second ride is the evidence that the first ride's lessons were
real: same code path, same strangers, nothing left to apologise for in
the thread.
