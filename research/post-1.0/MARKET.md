# Digital goods on the market (research, 2026-08-31)

*Kara's prompt: Publishing/Library should meet the Marketplace — boards
are geohashes, but digital goods have no "near". Near-me and worldwide
boards, both carrying digital assets?*

## What exists, read before opining

- A board name is a **string** the protocol stamps and shards:
  `geo:u4pruy@3021-3`, `local:<cell>@epoch-shard`. The spec pins the
  generation grammar and the shard ladder — **not the prefix**. A
  `topic:<category>` board is protocol-legal today, inheriting weekly
  epochs, the shard ladder, and tombstones unchanged.
- **The worldwide-spam problem is already priced.** Since 0.89 every
  notice costs Argon2id work (~0.7 s/notice honest, memory-hard, GPU
  does not help) and is stamped against a recent Monero block, so
  precomputation is dead and re-poisoning is re-paid weekly. Readers
  verify signature first, so junk is refused at Ed25519 price. A
  worldwide category board inherits all of this — the analysis that
  usually kills "global boards" was done here in advance.
- The listing notice (§16.18, RENTAL_NOTICE) already carries the exact
  mechanic a digital good needs: a **claim-once card** plus typed
  fields, where the mismatched half is MALFORMED. And on the other
  side, **claim-a-publish-card = subscribe** already works end to end
  (auto-enroll, billing by TabStore origin "pub", shelf/swarm
  delivery, Library filing).

**The insight: this is not a merge, it is a bridge.** A digital
listing is a publication's claim-once card riding a board notice.
Discovery is the only missing piece; everything after the claim ships
today.

## Proposed shape

1. **`topic:` boards** — category replaces geography. A small pinned
   slug set (spec appendix, like deal kinds): `news`, `serials`,
   `sound`, `software`, `art`, `other` — deliberately few; a taxonomy
   argument is a place communities die. Language as an optional
   suffix from day one (`topic:news.es@epoch`): publications are
   language-bound in a way kayaks are not, and a worldwide board that
   mixes scripts is a wall of elsewhere. Readers browse category, then
   language defaults to the phone's.
2. **A `PUB_NOTICE`** (spec dev6, new field family): publication
   title, period price (zero = free), the claim-once publish card,
   optional blurb — typed like place/car, mismatched halves MALFORMED,
   same Argon2id + beacon stamp as every notice. No address-shaped
   fields exist to leak: the notice is the advertisement, the sealed
   thread remains the relationship.
3. **Local boards can carry them too.** The Riverside Gazette is a
   local paper: a publisher may post the same notice to `local:<cell>`
   and to `topic:news.<lang>` — cross-posting is two work-stamps, paid
   honestly. "Near me" and "worldwide" both carrying digital goods is
   exactly kara's instinct, and it needs no extra design.
4. **UX**: Marketplace gains a scope — *Near me* (existing walk) /
   *Worldwide* (topic browse) — digital listings render with
   Subscribe, which is the claim flow verbatim. Publishing gains
   "List it on the market" (pick category + language; rides the
   existing repost/refresh cycle with a fresh bound card each
   generation). **Library does not change**: it is the bookshelf, and
   subscriptions arrive there regardless of which door introduced
   the publisher.

## What it costs

Wire work this time (unlike calls' media layer): a new notice family +
vectors both implementations + ducat_check rules — a dev6. Then
Listings gains topic-board posting/reading beside cells (the stand
machinery is name-agnostic), the two screens grow their affordances,
and a desk pair proves stranger-discovers-subscribes-receives with no
prior contact.

## Open choices (kara's)

- The slug set and whether language sharding lands in v1 (recommend:
  yes, `.lang` optional, default = phone's).
- PUB_NOTICE as its own family (recommended — a publication is not a
  rental with a gearbox) vs. stretching RENTAL_NOTICE.
- Whether the Marketplace scope is two chips (recommended) or a
  separate "Digital" surface.
