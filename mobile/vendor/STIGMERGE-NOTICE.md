# Vendored: stigmerge and veilnet, by Casey Marshall (cmars)

The swarm layer under §16.20's heavy shipments is **not ours from
scratch**. Two of Casey Marshall's projects are vendored here, with
gratitude — they represent roughly two years of learning how to move
file pieces over Veilid, and that learning is exactly what we wanted to
inherit rather than rediscover:

| directory | upstream | vendored from | license |
|---|---|---|---|
| `stigmerge-fileindex/` | https://github.com/cmars/stigmerge | git `8f26b50ffa058eaa009c61eccabe32b2a756de4c` (2026-08-30) | MPL-2.0 |
| `stigmerge-peer/` | https://github.com/cmars/stigmerge | same commit | MPL-2.0 |
| `veilnet/` | https://codeberg.org/cmars/veilnet | crates.io `veilnet-0.4.5` | MPL-2.0 |

stigmerge ("anonymous file sharing over Veilid; there are no leechers —
all peers are seeders") is the engine: file indexing into pieces, block
transfer, peer gossip, share announcement and resolution over the Veilid
DHT. veilnet is its connection abstraction over `veilid-core`. Upstream
is actively developed and migrating to Codeberg; check there for the
living project.

## License

Both projects are **MPL-2.0** (each directory carries the upstream
`LICENSE`). The MPL is file-level copyleft: these files and our
modifications to them remain MPL-2.0, source-available here, regardless
of the license the rest of this repository uses. Do not copy code from
these directories into BSD-licensed files elsewhere in the tree.

## What we changed, and why

Recorded per the MPL's Exhibit A expectations and plain courtesy:

1. **Cargo.tomls rewritten** from upstream's workspace-inherited form to
   concrete versions, with `veilnet` and `stigmerge_fileindex` resolved
   to the vendored copies, and `veilid-core` widened to `0.5` so cargo
   unifies it with the version the rest of this workspace ships.
2. **Piece hashing converted from SHA-256 to BLAKE3** (the departure
   that made this a vendoring rather than a dependency — see
   `research/post-1.0/REPORT.md` for the reasoning: one hash family
   across the whole stack for the eventual adversarial review, and the
   tree/keyed/derive modes the publication layer already leans on).
   **This makes shares wire-incompatible with upstream stigmerge
   swarms**, deliberately: DUCAT swarms are club-scoped, sealed under
   §16.20 period keys, and never meet the public network's.
3. **`veilnet::Connection::from_api`** — ride an already-running node.
   The host keeps the update callback and forwards into a returned
   feeder; borrowed connections (`owned = false`) cannot detach,
   reattach or shut the node down, pinned by test. Upstream candidate.
4. **A route observer** (`stigmerge-peer/src/route_registry.rs`), called
   by the share announcer on every private-route create/retire, so an
   embedding host can demultiplex inbound `AppCall`s between the seeder
   and its own protocol. A no-op unless installed. Upstream candidate.
5. **The CLI's orchestration layer vendored as a module**
   (`stigmerge-peer/src/share.rs`, from `stigmerge/src/share.rs`), so an
   embedding application drives seeds and fetches exactly as the CLI
   does; its `want_index_digest` takes a `Digest` rather than decoding
   hex.
6. **Multi-file shares (2026-09-01).** Upstream's index format always
   carried `Vec<FileSpec>` but three code paths were single-file
   (`from_wanted`/`index()` `unimplemented!`, the verifier's and
   seeder's payload-global seeks — both marked FIXME upstream). DUCAT
   implements the piece-aligned layout: every file starts on a fresh
   piece (`piece_offset` stays 0 forever), which resolves upstream's
   unaligned-slice TODO by never creating one. `Indexer::from_path`
   walks a directory sorted by path; the wanted side lands each present
   file's pieces at the want index's own global positions so a missing
   earlier file cannot shift the comparison; block writer, verifier and
   seeder all seek file-locally. Single-file shares are byte-identical
   to before, payload digest included; multi-file payload digests are
   the BLAKE3 chain of per-file digests in path order. Pinned by
   `multi_file_tests.rs`. Upstream candidate.
   `from_wanted` canonicalizes the fetch root before rooting anything
   under it (2026-09-01): it canonicalizes each file, and a literal
   root over a symlinked path — Android's `/data/user/0` is a symlink
   to `/data/data` — never prefixes its own canonicalized files, so
   every phone-side fetch failed `strip_prefix` inside "index local
   share" while identical code passed on a desk. Pinned by
   `wanted_root_through_a_symlink_indexes`.
7. **A refused DHT write is an error, not a shrug (2026-09-01).**
   `set_dht_value` returning `Ok(Some(_))` is veilid refusing the write —
   the network holds a value signed at a later sequence than the local
   record state knows, and a plain process restart is enough to cause it.
   Every write in `record.rs` discarded that return, so a restarted
   seeder would re-verify its pieces, "sync" its have-map, and the
   network would keep last session's partial map for ever — fetchers
   then never ask for the pieces it actually holds (found live: a
   two-piece map advertised for a three-piece site, `LeaseRejected(No
   MatchingPieces)` on the missing one from every fetcher). The same
   silence covered share-header reannounces and peers-record writes.
   All owner writes now go through one helper that, on refusal,
   force-reads the subkey to refresh local record state and retries
   once — the retry then signs at the sequence the network expects —
   and a second refusal is a visible error. Upstream candidate.
8. **Peer reputation (2026-09-01).** Upstream's fetcher respawns an
   exited pool immediately and unconditionally (its own TODO says so),
   so one dead peer is redialed hot for ever — and a share whose origin
   died keeps every corpse in its frozen peer list in the piece lottery.
   `peer_reputation.rs` is the standard cure, process-global like
   `route_registry`: libp2p's dial backoff joined with BitTorrent's
   snub and optimistic unchoke. A pool notes its peer's behaviour (a
   delivered piece clears, a failure benches, 30 s doubling capped at
   ten minutes, memory decays after an hour); the fetcher parks benched
   peers instead of respawning them, a five-second revive tick returns
   them when served, and when nobody at all is admissible the least-
   recently-tried is dialed anyway so a recovered peer is rediscovered.
   Roster entries nobody refreshed in an hour start benched one minute
   (`peer_gossip`) — staleness is a priority signal, never a filter,
   because a dead origin freezes the live mirror's timestamp along with
   the corpses'. Pools also re-aim each lease at the peer's CURRENT
   route (`ShareResolver::current_route`): a pool's `RemoteShareInfo`
   is a snapshot, and on a phone the announcer rotates its route every
   few minutes — the live-measured shape was one piece per bootstrap.
   Pinned by `peer_reputation::tests`. Upstream candidate.
   Gossip follows the same rule (2026-09-01): a roster entry that will
   not resolve is skipped and benched, where upstream's `?` aborted the
   whole reannounce — one expired record in a frozen roster silenced a
   live mirror to every other peer. And a peer whose index does not
   match is now actually rejected; upstream logged the rejection and
   recorded the peer anyway.
9. **`Status::Done` is actually sent.** Upstream's fetcher returned its
   internal `State::Done` on `index_complete` without ever emitting
   `Status::Done` on the status channel — so every consumer waiting on
   the documented signal, upstream's own CLI included, waited forever on
   a finished fetch. One line at the completion site. Upstream's issues
   #401/#402 may be this. **The clearest upstream patch of the lot.**
10. **An index off the wire may not name a path outside its root
   (2026-09-01).** A fetcher creates and writes every file the
   publisher's index names, under its own root, before a byte is
   verified — and the path came from the publisher verbatim. An
   absolute path replaced the root outright; a `..` walked out of it.
   On a phone the root sits beside the app's own data, so a hostile
   share could have overwritten it. `FileSpec::is_contained`
   (`stigmerge-fileindex/src/lib.rs`) says whether a path is relative,
   non-empty and never climbs; `Index::from_wanted` refuses to build
   from one that is not, and the wire decoder
   (`stigmerge-peer/src/proto/index.rs`) refuses the whole index with
   `Error::UnsafePath` before anything is created under that name
   (test `escaping_paths_are_refused_at_decode`). **Upstream candidate.**
11. **A finished task's update handler is skipped and swept
   (2026-09-01).** veilnet's `HandlerChain` only ever grows: the share
   resolver, announcer, seeder and peer gossip each register a handler
   on the connection when they start and nothing removes it when they
   stop, so on a connection shared by every seed and fetch a process
   makes (ours; upstream's CLI makes one per process) each dead handler
   kept receiving every update, failing to send it down a closed
   channel, and logging the failure — 587 lines of "failed to send
   route change" in one shutdown, on a phone whose readable log holds
   600. `UpdateHandler::is_done` (default `false`) lets a handler say
   its task is gone; the four stigmerge handlers and the datagram
   listener answer from their channel (`Sender::is_disconnected`), the
   chain skips a done handler on every dispatch and drops it on the
   next `add` (test `done_handlers_are_skipped_and_swept`). **Upstream
   candidate for both crates.**

Proven live 2026-08-30, two DUCAT nodes on real Veilid: 25 MiB in 97.5 s
and 100 MiB in 279.9 s (~3 Mbit/s through private routes), payload
BLAKE3 identical on both ends, clean exits
(`mobile/examples/swarmtest.rs`).

The "~2 cores while serving" cost first observed with that proof was
run to ground on 2026-08-31 and is NOT this engine's doing. Most of it
was veilid volunteering the inbound-capable host as public
infrastructure (relay/DHT/route-hop — shed in `mobile/src/node.rs`,
`capabilities.disable`), and most of the remainder is veilid's
safety-route keepalive, the standing price any DHT-writing DUCAT node
pays for private routing — the mailbox included. Steady-state on a
desk, measured minutes after announce: bare node 13% of one core, any
record-writer 41%, live seeder 48%. Seeding itself costs ~7 points
over what a phone already spends to be a phone.

The CLI (`stigmerge` itself), the Docker/Nix packaging, and the examples
are not vendored — only the two library crates and their tests.
