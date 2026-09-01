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
6. **`Status::Done` is actually sent.** Upstream's fetcher returned its
   internal `State::Done` on `index_complete` without ever emitting
   `Status::Done` on the status channel — so every consumer waiting on
   the documented signal, upstream's own CLI included, waited forever on
   a finished fetch. One line at the completion site. Upstream's issues
   #401/#402 may be this. **The clearest upstream patch of the lot.**

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
