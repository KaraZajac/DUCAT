# The post-1.0 track: compartments, doorways, publications, and the truck

*Design written down before code. 2026-08-30, at the 1.0.0-rc1 freeze —
nothing here touches the wire until 1.0.0 ships. Four features, one
dependency chain, decided in conversation and recorded here so the
decisions survive it.*

## Why these four, and why this order

Each one unblocks the next. Personas answer the identity question the
sign-in doorway raises; the doorway opens the thread the publication
bills on; the publication is what eventually wants the swarm's bandwidth.
Every piece is **additive** — new purposes are open text, new kinds are
ignored by old readers, new record schemas live beside old ones — so under
the spec's own strictness rules this whole track is minor-version
territory: personas + doorway are 1.1, publications 1.2, the swarm 1.3.
2.0 is reserved for the day something breaks compatibility, and planning
a 2.0 nobody needs is how version numbers start lying.

## 1.1a — Personas: few, named, visible compartments

The protocol has no opinion on how many personas a phone holds — every
wire object is pairwise between persona keys. Multi-persona is therefore
client storage and UX, zero wire change.

**The doorway rule** (the design's one load-bearing idea): a relationship
binds to a persona exactly once, at its beginning — card minted, card
claimed, listing posted, hail posted — and everything after inherits the
binding with no ongoing choice to get wrong. The persona picker exists in
exactly four places; every other screen only *displays* whose thread it
is.

Decisions made:

- **Modes bind to personas.** Enabling POS/Bar/Kiosk asks once "who is
  the till?", offering to mint a Shop persona. A mode is what the device
  is for; this extends it to who the device is being. Switching modes is
  switching hats, which mostly dissolves the account-switcher problem.
- **One wallet, per-persona views — never seed-per-persona.** On-chain a
  shared wallet is safe (subaddresses mutually unlinkable, ring
  signatures hide spends); the ledger already records receiving minor +
  contact, and contacts gain an owner, so per-persona takings are
  derivable. §4.3's backup-fright argument against multiple chains
  applies verbatim to multiple seeds. Stated caveat: a counterparty
  talking to two of your personas can attempt timing correlation —
  inherent to any shared wallet, weak, and written down rather than
  wished away.
- **Cap at three or four.** Compartments work when they fit on one hand
  (Personal, Shop, Browsing). No per-site or per-contact personas — the
  payments are already unlinkable via Monero; personas are for *social*
  compartments, and many compartments are the same as none.
- **Worn color everywhere.** Name, avatar, accent per persona; the accent
  tints the bar. Notifications prefix the persona name.

The build order within the feature: owner column on ContactStore with a
one-time migration (securePrefs pattern: commit-before-delete), stores
and poller iterate personas, backup format grows a personas array (old
backups map to one) — then, only after a two-emulator wipe-and-restore
pass, the mode binding. The migration + restore surface is where this
feature's bodies would get buried; it gets the groups-restore treatment.

## 1.1b — The doorway: sign in with DUCAT

A website's "Sign in with DUCAT" button is a claim-once card with purpose
`login` (open text, not a wire change): the site mints one card per
browser session, renders QR + `ducat:` link, and the visitor's claim —
verified against their persona key, bound to exactly that session by
claim-once — *is* the authentication. No password, no email, nothing to
breach. The purpose scoping already keeps reach-me identifiers off a
login handshake by defaulting private.

What distinguishes this from LNURL-auth/Telegram-login: the sign-in comes
with a standing pairwise thread attached — the billing channel. That is
the whole reason to prefer the card over a bare signature challenge.

Decisions and named edges:

- **The server piece is the CLI roadmap's watch daemon wearing a job**:
  mint-on-demand from a pre-minted pool (publishing takes seconds; the
  pool hides it), watch claims, map card→session, tiny HTTP face for the
  site. Headless desk precedent applies.
- **The app's claim confirm must name the purpose and the site** ("Sign
  in to Ana's?") — this is also the main mitigation for QR relay
  phishing, which every QR-login system has and which gets a plain
  sentence in the spec section, not a pretense of absence.
- **Cross-site linkability is answered by personas**, not by the doorway:
  a Browsing persona is the standing answer, which is why personas ship
  in the same minor version.

## 1.2 — Publications: membership is the paid thread

"Patreon with no Patreon." A publisher's content is sealed into DHT
records; membership is nothing but the pairwise thread; the recurring
bill (already shipping) goes out on schedule; **when a payment settles,
the period's content key rides the same reconcile loop that already
auto-sends the receipt** (SecondOpinion-gated, mark-before-send — the
donate rail's exact discipline). Miss a payment: no new key, nothing
retried against your will, and everything already paid for stays
readable. Cancelling is stopping.

Decisions:

- **Period keys derive, they are not stored**: BLAKE3
  `derive_key("DUCAT pub v1 · <period>", master_secret)`. One secret in
  securePrefs; any month's key — back catalog included — is a
  re-derivation. The master rides the normal backup.
- **The thread-sufficiency invariant** (the rule that keeps the model
  honest as it grows): a paying member must always be servable through
  the thread and the DHT alone. A website — clearnet, onion, or both —
  is a shop window holding no secrets and may be added, seized, or
  dropped at any time without any member losing what they paid for. The
  site is the nicest door, never the only one. Centralize-later works
  precisely where decentralize-later does not, because the relationships
  are born in the protocol and the central thing can never capture them.
- Scale shape, stated once: per-subscriber traffic is two tiny messages a
  month (linear on the publisher's device, self-staggered by due-date
  anniversary); content distribution is on the network (flat for the
  publisher). Hundreds of members on a desk is loafing; ~10k on one node
  is the honest ceiling and somebody else's milestone.

## 1.3 — The swarm: read stigmerge, build the truck ourselves

For months that ship heavy (albums, archives), a torrent-shaped layer:
manifest as a thread message (authenticated — our "magnet link" problem
does not exist), swarm roster as a board (ladders, generations,
tombstones — machinery already proven under fire), pieces over private
routes (IP-private swarming; I2P proved the concept viable), everything
ciphertext under the period key so non-members can donate bandwidth for
content they cannot read.

**Decision revised 2026-08-30 (kara):** vendor stigmerge's code with
full credit and convert it to BLAKE3, rather than rebuilding from a
reading of it. Done the same day: `mobile/vendor/{stigmerge-fileindex,
stigmerge-peer, veilnet}` at upstream git `8f26b50f` / veilnet 0.4.5,
MPL-2.0 preserved, `mobile/vendor/STIGMERGE-NOTICE.md` records origin
and modifications. Three changes so far: piece/payload/index hashing is
BLAKE3 (shares deliberately wire-incompatible with public stigmerge
swarms — ours are club-scoped and sealed), `veilnet::Connection` grew
`from_api` (ride the app's running node; borrowed connections cannot
detach, reattach or shut it down — pinned by test), and Cargo.tomls
went concrete. All 91 vendored tests pass converted. Next: the node
update-feeder hookup in mobile/src/node.rs (route-keyed AppCall demux),
then seed/fetch bridge functions, then Kotlin. When the bridge links
these crates, grow the two nativeFreshness guards' rustDirs to include
mobile/vendor.

**Prior art evaluated 2026-08-29:** `stigmerge` (né distrans) by cmars —
MPL-2.0, veilid-core 0.5.6-compatible (ours resolves 0.5.7), engine
generic over a `veilnet::Connection` trait (five methods, injectable),
migrating GitHub→Codeberg, one maintainer, an open concurrent-fetch race
(#400). Verdict: **mine the code, not the crates.** What to take: the
Veilid scar tissue (seeder reset/reannounce on network loss, backoff
shapes, fetch-task coordination, 1MB pieces as a tested constant) and
the warning that concurrent fetch coordination is where the testing
budget goes. What we build differently, which is the requirements list —
none of it on their roadmap: content encryption, membership, keyed
BLAKE3 addressing (a public content hash of a known file is a
confession; a club-keyed one is not), verified ranges, multi-file as the
native shape (a month is a bundle), N standing shares per node, and a
mobile runtime that seeds only charging + unmetered. BLAKE3 throughout —
the tree gives verified ranges, `derive_key` gives the period schedule,
keyed mode gives private addressing and (someday) seeder spot-checks;
one primitive for the reviewer to read instead of a museum. Paid seeding
stays shelved: fair exchange for bandwidth has eaten every attempt at
it; the desk is the guaranteed seed and club spirit does the rest at
club scale.

Gate to start: the first time someone actually needs to ship ~100MB to
~100 people. This feature gets easier to justify the longer it waits,
because the demand signal is unambiguous.

## What is deliberately not decided here

Field numbers and kind assignments (the registry is spec-time work, with
audit_spec holding it honest); the doorway's exact HTTP face; whether the
publication's sealed records reuse the attachment chunker or grow a
sibling; swarm piece/block sizes beyond stigmerge's tested 1MB. These are
building decisions and will be made against running code, the way
everything above 0.85 was.
