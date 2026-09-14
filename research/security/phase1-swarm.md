# Phase 1.5 — the fetch byte cap, index validation, seeder back-pressure

*2026-09-14. TRUST.md §8 step 1.5. Closes N4/D2, N5, N11, N14, N18, N25, N26
of research/security/2026-09-07-adversarial-review.md.*

The shape of all seven findings is one sentence: **the share's index is the
publisher's claim, and the engine believed it.** It created and sized every
file the index named before a byte was verified, trusted every length in it,
served every block anybody asked for, and scored a peer for answering rather
than for answering correctly. What follows is per fix: what changed, where,
the constants and why, and what is left.

Everything in `mobile/vendor/` carries a `// DUCAT modification` comment at
the change and an entry (item 13) in `mobile/vendor/STIGMERGE-NOTICE.md`, as
the MPL and plain courtesy ask. All five are upstream candidates.

---

## 1. Index validation at decode (N4, N14, N25)

**Files.** `mobile/vendor/stigmerge-fileindex/src/lib.rs` (the rule),
`mobile/vendor/stigmerge-peer/src/proto/index.rs` (at decode),
`mobile/vendor/stigmerge-peer/src/proto/mod.rs` (the error),
`mobile/vendor/stigmerge-peer/src/record.rs` (again, with the header),
`mobile/vendor/stigmerge-peer/src/{share,piece_verifier,seeder,block_fetcher,fetcher,types}.rs`
(the panics and the arithmetic around them).

**What changed.** `check_index_shape(pieces, files, declared_length)` refuses
an index that could not describe content anybody indexed:

| rule | why |
|---|---|
| a piece longer than `PIECE_SIZE_BYTES`, or of length 0 | a long piece overflows the block maths and the 32-bit shifts; an empty one is free padding for a piece list |
| a file's slice outside the pieces list | this is the panic at `piece_verifier.rs::empty_pieces` |
| a file's slice starting inside a piece (`piece_offset != 0`) | DUCAT shares are piece-aligned for ever (notice item 6); a non-zero offset only pushes a write past the end of a file |
| a file's own pieces not summing to its declared length | the seek arithmetic and the have/want diff both read the two as one number |
| a piece claimed by two files, or by none | the sums can still balance while the shape lies |
| Σ pieces ≠ Σ files ≠ the header's payload length | **the number the byte ceiling is weighed against** |
| more than 16 GiB, 65 536 files, 81 920 pieces | absolute ceilings; nothing real reaches them |

It is called twice: in the wire decoder, before anything is created, and in
`record::read_index`, where the header's payload length joins the index — the
second call is what makes the ceiling in §2 meaningful, because otherwise the
header could declare 256 MB and the pieces deliver 900 GB.

**Constants** (`stigmerge_fileindex`): `MAX_PAYLOAD_BYTES = 16 GiB` (a u64, so
the check itself cannot overflow on a 32-bit target), `MAX_FILES = 65 536`,
`MAX_PIECES = 81 920` (16 GiB is at most 16 384 whole pieces, plus at most one
short tail piece per file). They are ceilings on the *format*, not budgets:
the per-fetch budget is §2 and lives in the application.

**The two panics (N14).** `share.rs:206`'s `header.have_map().unwrap()` is now
an error — a header off the wire need not carry the reference.
`piece_verifier.rs`'s `empty_pieces` skips a piece the index does not carry
instead of indexing past the end, and `verify_piece` bounds both lookups.
`fetcher`, `block_fetcher` and `seeder` use `.get()` wherever they used `[i]`
on a list a stranger sized.

**Checked arithmetic (N25).** `types.rs::block_offset_in_file` returns
`Option<u64>` (it underflowed when a file's `starting_piece` was past the
block's piece, and overflowed the multiply on 32-bit); `PieceState::new` and
`is_complete` use `checked_shl` (a block count over 32 shifted off the end of
the word). `Index::declared_length()` is the new public accessor the ceiling
reads.

**Tests.** `stigmerge-fileindex/src/tests.rs`: eight cases, one per rule, plus
`an_honest_shape_passes` over one-file, multi-file and empty indexes.
`proto/index.rs::hostile_shapes_are_refused_at_decode`: "one file, 900 GB" on
one piece, a 64 MiB piece, and a slice past the pieces list — all
`Error::UnsafeIndex`, and the honest index beside them still decodes.
`types.rs::tests`: the offsets and shifts.

---

## 2. The byte ceiling (N4/D2, N26)

**Files.** `mobile/vendor/stigmerge-peer/src/share.rs`, `mobile/src/swarm.rs`,
`app/src/{attachments,sites,releases,publications,listings}.rs`, Android
`Swarm.kt` and the callers below.

**What changed.** `Mode::Fetch` gained `max_bytes: Option<u64>`;
`share::check_budget` refuses an index declaring more, inside `start`, *before*
`Indexer::from_wanted` creates or sizes anything. The refusal is its own type
(`share::TooLarge { declared, max }`, found in a chain by `share::too_large`),
so the client can say both numbers to the person rather than showing them a
failure.

The bridge is a **new** function — `swarm_fetch_capped(share_key, digest, root,
stay_seeding, max_bytes)` — with `swarm_fetch` kept as a delegate at
`caps::DEFAULT`, so no caller was left uncompiled and none is left uncapped.
`SwarmError` gained `TooLarge(String)` (a whole human sentence, e.g. "this
bundle says it is 3.2 GB; the most this will take is 256.0 MB" — `saidWhy()`
strips uniffi's `v1=`) and `TooSlow(String)` (see §4).

**The table** (`swarm::caps` in Rust, `Swarm.Caps` in Kotlin — same numbers,
kept together on purpose):

| kind | cap | why |
|---|---|---|
| listing gallery | 64 MiB | photographs of a thing for sale, thumbnailed |
| home | 256 MiB | fetched **unattended** on the lap when hearted |
| site | 256 MiB | fetched **unattended** when kept |
| publication issue | 256 MiB | |
| release | what the entry declares, else 4 GiB | a release names its bytes once anybody has held it; it is the one thing here that is legitimately huge |
| swarm attachment | the sealed message's `att_len` + 1 MiB slack | slack covers the AEAD tag and the blob's wrapper; the existing room check (budget + free-space floor) still does the real work and is unchanged |
| anything else | 4 GiB | a ceiling, not a budget: the "any size at all" shape cannot come back through a caller nobody updated |

**Callers moved.** Desk: `attachments.rs` ×2, `releases.rs` ×2, `sites.rs` ×2,
`publications.rs` ×3, `listings.rs` ×2. Phone: `Galleries.kt`, `Sites.kt` ×2
(hearted homes reach the swarm through `Sites.fetchBundle`, so the home cap is
the site cap at that call site), `Releases.kt` ×2, `Listings.kt`,
`Mailbox.kt`'s swarm attachment. Re-parks of *our own* published content
(outbox blobs, our own issues) take `DEFAULT` — the bytes are already on this
disk and we chose them; re-parks of *somebody else's* (kept sites, mirrored
issues) take the per-kind cap, which is the "hardest" case the review asked
about.

**Test.** `share.rs::a_share_bigger_than_the_budget_is_refused` — exactly the
budget is inside it, one byte over is refused with both figures, and the type
survives being carried as an anyhow error, which is how `start` hands it back.

---

## 3. Seeder back-pressure (N5)

**File.** `mobile/vendor/stigmerge-peer/src/seeder.rs`.

**What changed.** Serving was unbounded in four ways at once: an unbounded
`flume` queue, one spawned task per block request each holding a 32 KiB stack
buffer, all of them serialized behind `inner.lock().await` **held across the
network reply**, and a have-not answered inline so the loop itself waited on a
peer's round trip. Now:

- **a token bucket per inbound private route** — `ROUTE_BUCKET_CAPACITY = 64`
  requests per `ROUTE_BUCKET_WINDOW = 10 s`. A request with no token **waits**
  for one (up to `MAX_SHAPE_WAIT = 5 s`) rather than being dropped, so an
  honest fetcher asking faster is *paced*; a flood outruns the wait and is
  dropped. The route is all a seeder can meter on — a call over a private
  route carries no sender, by design — so this is a rate per swarm, not per
  asker.
- **a bounded queue** — `MAX_QUEUED_BLOCK_REQUESTS = 256`, `try_send`, and a
  full queue drops rather than cancelling the seeder (only a disconnected
  channel means the task is gone).
- **a semaphore** — `MAX_INFLIGHT_REPLIES = 8`, `try_acquire_owned`, excess
  dropped. Each reply holds one block, so this is also the seeder's memory
  ceiling: 256 KiB, from unbounded.
- **the read under the lock, the reply outside it** — the reply goes through a
  cloned connection handle (`Seeder::reply_conn`; `Connection` is `Clone` and
  shares the node), so one peer's round trip no longer blocks every other
  request on the share. The have-not path takes the same route, off the loop.
- **finished reply tasks are reaped** in the select loop; upstream never
  joined them, so a long-lived seeder's `JoinSet` grew with every block it ever
  served and a failed read was never noticed.
- **a block index at or past the piece's block count is refused** rather than
  seeking past the end of the file (`SeederInner::read_block`), and the read is
  `read_exact` of exactly the block's declared length.

**Worth knowing when tuning the rate.** The live proof runs moved 25 MiB in
97.5 s (~80 requests per 10 s) and 100 MiB in 279.9 s (~114). Both are above
64, so this rate does cap a single fast transfer — roughly 205 KiB/s per
inbound route. That is the trade the review asked for and the reason the
bucket paces instead of dropping: a seeder is a background service on somebody
else's device, and without a rate the peer that asks hardest decides how much
of it they get. **If a field run shows honest transfers dragging, this
constant is the one to raise** (it is named, in one place, with this note
beside it).

**Tests.** `seeder::bucket_tests` — the burst is spendable, the next request
gets a wait rather than a refusal, and the budget is per route.

---

## 4. Tail-piece poisoning and strikes (N11, N18)

**Files.** `mobile/vendor/stigmerge-peer/src/{block_fetcher,piece_verifier,fetcher,types}.rs`,
`mobile/vendor/stigmerge-peer/src/piece_leases/manager.rs`, `mobile/src/swarm.rs`.

**The poisoning.** `block_fetcher` clamped a reply to `BLOCK_SIZE_BYTES`
instead of the block's expected length and opened with `truncate(false)`, so a
long garbage reply to the **last** block of a piece — the short one — grew the
file past the length its own index declares; `piece_verifier` then read to end
of file and that piece could never verify again, from anybody. One hostile
mirror, one reply, and the tail of the download was dead.

- `types::expected_block_len(piece_length, block_index)` is now the one
  arithmetic both ends ask (fetcher and seeder), and a reply of any other
  length is refused.
- the file is `set_len` to its declared length on first open — every real byte
  is kept (the declared length is the sum of its pieces) and anything past it
  is dropped. `truncate(false)` stays, because a resumed fetch must keep what
  is on disk.
- a write that would land past the end of the file is refused before the
  request is even sent.
- `piece_verifier::verify_piece` hashes **exactly** the piece's declared
  length; a short file reads as "not here yet", not as corrupt.

**The strikes.** `FetchPool` called `note_success` the moment blocks landed —
delivery, not correctness — so a mirror answering with garbage cleared its own
record every time it poisoned a piece, and only `InvalidPiece` was ever
counted (against nobody).

- credit moved to the fetcher's `ValidPiece` branch: a peer is scored for a
  piece that **verified**.
- `PieceLeaseManager::lease_holder(piece_index)` names who delivered a piece,
  asked *before* the lease is released; `InvalidPiece` now calls
  `peer_reputation::note_failure` on them.
- `MAX_BAD_PIECES = 3` and that peer is dropped from the fetch: its pool
  aborted, not respawned, not revived from the bench, and ignored if discovery
  offers it again. Three rather than one because a piece can fail honestly (a
  truncated write, two peers racing a lease), and one strike would partition a
  swarm on ordinary noise.

**The stall budget (N18).** `mobile/src/swarm.rs` used to reset `stalls` to 0
whenever an attempt moved any bytes, so a peer serving one block per
re-bootstrap could keep an unattended fetch bootstrapping for ever. That reset
is gone: six attempts is the whole budget. In its place, a minimum-throughput
rule inside `fetch_once` — `THROUGHPUT_WINDOW = 180 s`,
`MIN_PIECES_PER_WINDOW = 3` (one verified piece a minute, 1 MiB, ≈ 5.7 KiB/s)
— fails the attempt with `SwarmError::TooSlow`, which the retry loop counts
against the same six. The window opens at the fetcher's first status, not at
birth: before that the share is still being resolved, which is DHT work and can
honestly take minutes.

---

## Gates

- `cargo test -p stigmerge_peer` — 60 passed (53 before this step, plus seven new).
- `cargo test -p stigmerge_fileindex` — 20 passed.
- `cargo test -p ducat-mobile --lib` — 63 passed, 1 ignored.
- `cargo test -p ducat-app` — 67 passed.
- `applications/gradlew -p applications :android:assembleDebug -x :android:nativeFreshness` — **BUILD SUCCESSFUL**.
- Bindings regenerated with `DUCAT_ABIS="x86_64-linux-android:x86_64" bash mobile/build-android.sh` (the STALE arm64 warning at the end is expected for a one-ABI run).

## What is left

- **`:desktop:compileKotlin` fails, for reasons that are not this step.**
  `applications/desktop/src/main/kotlin/org/ducatproject/desk/Arbiter.kt` and
  `ReleaseReadTest.kt` are written against `frostDestinations` and the old
  `frostCosign` arity. The working tree's in-flight step 1.4 replaced those in
  `mobile/src/ceremony.rs` (`frost_view`, `frost_cosign` with
  `fundedPxmr`/`localEstimatePxmr`) and updated the phone's `Ceremony.kt` but
  not the desk's two files, and the committed bindings still described the old
  shape — regenerating the bindings for this step is what surfaced it. With
  those two files set aside, `:desktop:compileKotlin` succeeds. Nothing
  swarm-related fails; the fix belongs to 1.4.
- **`ui/Library.kt` (phone) still fetches publication issues at
  `Caps.DEFAULT`** (4 GiB) rather than `Caps.ISSUE` (256 MiB) — three call
  sites, outside this step's file list. `Poller.kt`'s outbox re-park is also
  `DEFAULT`, which is correct (it is our own blob).
- **`app/src/lib.rs`'s `From<SwarmError>` renders with `{e:?}`**, so a desk
  caller that uses `?` sees `TooLarge("this bundle says it is …")` rather than
  the bare sentence. The log lines that print the error directly read
  correctly. One line to change, in a file outside this step's list.
- **The seeder's rate is the one number to watch in the field** (§3).
- Not attempted here, still open from the review: N12 (a share key with a
  secret re-opening our own records), N13 (gossip amplification, unbounded
  peer tables), N9, N7.
