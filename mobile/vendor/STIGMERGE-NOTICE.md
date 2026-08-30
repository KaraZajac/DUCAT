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
3. Whatever the adaptation to DUCAT's already-running Veilid node needs
   in `veilnet` (a constructor over an existing `VeilidAPI` rather than
   `api_startup`), kept small and a candidate to offer upstream.

The CLI (`stigmerge` itself), the Docker/Nix packaging, and the examples
are not vendored — only the two library crates and their tests.
