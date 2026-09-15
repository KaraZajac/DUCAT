# Adversarial security review — 2026-09-07

Five read-only reviews of the tree at ffd6c7d0 (draft 1.1.0-dev9), one per attack
surface: wire format and spec, desk (Tauri), Android, network layer (Veilid, swarm,
Monero RPC), money flows. Each reviewer traced findings to lines; CONFIRMED means
the path was followed end to end, SUSPECTED means plausible but not executed.
The original reports were lost with the session's scratch directory; this file is
the durable record of what they found and what has been done about it. Line
numbers refer to ffd6c7d0.

Status legend: **fixed** (in the tree or in a named commit), **partial** (some of
the fix landed), **open**, **deferred** (design decision recorded, not scheduled),
**accepted** (by design, stated in the spec).

## Wire format and spec

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| W1 | Critical | core/src/contact.rs, mobile/src/contacts.rs, app/src/mailbox.rs, Mailbox.kt | The claimant's reply in a card's inbox was an unsigned map; any card holder could name someone else's persona (the arbiter, a friend) and have that contact rebound to the writer's outbox and prekeys | **fixed**: `CONTACT_ACCEPT` v2 as a §18.3 envelope under the persona named inside, bound to the inbox (302) and the half (303); version 1 refused; claimant checks subkey 0's persona against the card's; five vectors; draft dev10 |
| W2 | Medium | app/src/groups.rs, Groups.kt, §16.24 | One member can silently exclude another or brick the board by forming a generation with a subset or a bogus owner; the union rule only applies to ties | **fixed 2026-09-14** — both clients follow a generation only from its owner and only if it names everyone already held; the names still merge; §16.24 states it; desk test |
| W3 | Medium | mobile/src/node.rs, §16.24 | A group board's DHT descriptor publishes the full roster as persona keys to every storing node | **deferred**: derive per-group member keys; state the exposure in §16.24 |
| W4 | Medium | §16.9, §15.12 | A public card carries the inbox's decryption key, so every board reader can read the claimant's reply (persona, plate, car photo) | **fixed 2026-09-14** — the claimant seals its half to the bundle the issuer published in subkey 0, HPKE under its own info string with the inbox key as associated data; an unsealed reply is refused; §16.9 states it; proven desk to desk on the live network |
| W5 | Medium | mobile/src/feed.rs, app/src/home.rs, Home.kt | `feed.json`'s `persona` never compared with the home it was fetched from; a hearted persona can attribute posts to anyone, including the reader | **fixed 2026-09-14** — the desk's `feed_of` refuses a document whose persona is not the home it was fetched from; the phone the same (Home.kt) |
| W6 | Medium | §16.9 | First-writer denial: any board reader burns every card on a cell with one free DHT write | **deferred**: stamp on replies to board cards, or K reply slots; stated cost in the spec pending |
| W7 | Medium (S) | SafeImage.kt, §16.18.3 | Board thumbnails are decoded by the platform decoder automatically | **deferred**: decode board images in Rust and hand RGBA to the UI |
| W8 | Low | core/src/board.rs | A card can be lifted into a foreign notice: the poster key is unrelated to the card inside | **deferred** |
| W9 | Low | core/src/hpke.rs | Sealed-message ceiling (8 KiB) unspecified and below what a legal message may carry (255-member roster) | **fixed 2026-09-14** — ceiling 16 KiB in code and stated in §16.10 |
| W10 | Low | core/src/board.rs, Beacons.kt | Beacon freshness trusts one Monero node | **fixed 2026-09-14** — a beacon mismatch is `Unknown` until a second node seconds it; if the second node backs the notice, the node in use is the odd one out and the notice stands |
| W11 | Low | core/src/backup.rs, BackupSettings.kt | Export accepts an 8-character passphrase such as "password" | **fixed 2026-09-14** — core's export refuses `Weak`; both clients disable the button on the same grade |
| W12 | Low | §16.18 | HAIL_NOTICE version drift: spec says 1, code and vectors require 2 | **fixed 2026-09-14** — §16.18 says 2 |
| W13 | Low | core/src/contact.rs | `RentalNotice.features` bypasses the display-hazard filter | **fixed 2026-09-14** — features pass `display_hazard`; vector `listing_feature_with_bidi`; checker agrees |
| W14 | Low | core/src/wire.rs | `wire::open` verifies only object types 1–12 | **fixed 2026-09-14** — `type_from_code` decodes every registered code |
| W15 | Low | core/src/state.rs | The 120 s contact window is not held by the state machine | **open** |
| W16 | Low | §15.3.2 | "offer_commit is necessarily empty" has no wire meaning | **open** |
| W17 | Low (S) | core/src/escrow.rs | `SLASH_CLAIM` does not name the claimant | **open** |
| W18 | Low | app/src/publications.rs | Shelf index may promise any number of chunks; issue assembled in memory | **fixed 2026-09-14** — a shelf index is refused before a byte is fetched when it names more records or promises more chunks than a shelf can have; the writer's own limits are the reader's |
| W19 | Info | core/src/contact.rs | Two encodings of "no deposit" | **fixed 2026-09-14** — the deposit is always written, zero included; an absent field is MALFORMED, with a vector |
| W20 | Info | core/src/board.rs | `beacon_verdict` is dead code; both clients re-implement it | **open** |

## Desk (Tauri)

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| D1 | High | app/src/store.rs, wallet.rs, identity.rs | Spend key and persona secrets in plain JSON with umask permissions | **fixed** (owner-only dir 0700 and files 0600); encryption at rest still open |
| D2 | Medium | mobile/src/swarm.rs, home.rs, sites.rs | No size ceiling on fetched bundles; hearted homes fetched unattended | **fixed** (ab3aba7d) |
| D3 | Low | src-tauri/src/lib.rs | Sealed room relies on the per-response CSP alone; `.disable_javascript()` not set | **fixed 2026-09-14** — `.disable_javascript()` on the room's window |
| D4 | Low | mobile/src/monero.rs | Monero RPC over plain http | **accepted for now** — every public stagenet node's TLS is self-signed or CAcert; documented in the node list |
| D5 | Low | lib.rs | Broad main-window command surface (`picture_data_url`, `node_debug`, `log_tail`) | **open** |
| D6 | Low | app/src/log.rs | ducat.log carries names and amounts beside plaintext state | mitigated by D1 |
| D7 | Low (S) | app/src/attachments.rs | Record-road attachment trusts `att_len` up to 512 MB | **fixed 2026-09-14** — the record road checks its own bound before it sizes an allocation |
| D8–D10 | Info | | `shrink_picture` without the 24 MP guard; main-window CSP omissions; passphrase floor | **D8 fixed 2026-09-14** — `shrink_picture` reads the header and refuses over 24 MP, as every other picture on the desk already did; **D9 fixed** — the main window names object-src, frame-src, base-uri and form-action, the four a `default-src` does not cover; **D10 closed by W11** |

## Android

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| A1 | High | ui/Drawer.kt, ui/BackupSettings.kt | Backup export and import not behind the PIN | **fixed** in tree (PinGate on both) |
| A2 | High | build.gradle.kts | Shipped builds are debuggable and debug-signed | **before mainnet**: release build type, real keystore, R8 |
| A3 | Medium | res/xml/apduservice.xml, nfc/ | Standing card served over NFC from the lock screen; claims adopted automatically | **open** |
| A4 | Medium | ui/Hail.kt, Hailing.kt | A hail publishes name, ~1 km cell and a street-level destination unsealed | **open** — destination text moves into the sealed reply |
| A5 | Medium | DucatLog.kt | Native-crash tombstones written to public Downloads | **fixed** in tree (app-private) |
| A6 | Medium | (none) | No FLAG_SECURE; passphrase in cleartext and saved state | **fixed** in tree (`SecureScreen`, password fields, `remember`) |
| A7 | Medium | mobile/src/monero.rs | Plain-http nodes; the Rust stack ignores Android's cleartext policy | see D4 |
| A8 | Medium | Geo.kt, HailMap.kt | Exact coordinates to OSRM and Nominatim; tile cache | **accepted** (stated in UI); project-run front later |
| A9 | Medium (S) | SiteViewerActivity.kt, Galleries.kt, Home.kt | NUL byte in a bundle path throws outside any catch | **fixed** in tree (`Sites.insideRoot`) |
| A10 | Medium | MainActivity.kt, ui/Library.kt | `ducat:file/` links filed without confirmation | **fixed** in tree |
| A11 | Low/Med | Home.kt, Publications.kt | Unbounded work on the sweep (home bundles, shelf record lists) | **open** (with D2) |
| A12 | Low | RideStore.kt | Hail state in plain prefs | **fixed** in tree |
| A13 | Low | Pin.kt, ui/PinGate.kt | Missing PIN file reads as "no PIN yet" | **fixed** in tree (wallet present + no verifier = tampered, device credential required) |
| A14 | Low | ui/QrHub.kt, ui/NfcReader.kt | NFC card on a scan screen claimed with no name shown | **fixed** in tree (confirm with the name) |
| A15 | Low | ui/Chat.kt | Bubbles open any URL on tap; attachments open with sender-chosen MIME | **fixed** in tree (confirm with the full URL; MIME allow-list, share sheet otherwise) |
| A16 | Low | AndroidManifest.xml, MainActivity.kt | Unused BLUETOOTH permissions; `open_chat`/`open_group` extras from any app | **fixed** in tree (permissions removed; per-process token on notification intents) |
| A17 | Low | AndroidManifest.xml | `singleTask` with default task affinity | **fixed** in tree (`taskAffinity=""`) |
| A18 | Low | Poller.kt | Rate fetches fingerprint the phone every ~30 min | **accepted** (off switch exists) |
| A19–A20 | Info | | Device credential as spend gate; hearting silently mirrors | **open** |

## Network layer

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| N1 | High | mobile/src/monero.rs, ceremony.rs | Fee rate taken from the node with no cap (`max_per_weight = u64::MAX`) | **fixed** (e2ab23c6): named ceiling, built fee refused over the quote by a quarter or a twentieth of the amount, escrow proposer and co-signer alike |
| N2 | High | monero.rs, opinion.rs, SecondOpinion.kt | Second opinion defeated: returned tx hash unchecked, plain http, "unreachable" settles, pool presence counts as known | **fixed** on both clients (b53efabc, 6f3d4e9c): only `InBlock` elsewhere settles, the rest defer, ten-minute stall → "settle anyway", small-sale floor, confirmations by amount, own node honoured, returned tx hashed; nodes stay http because no public stagenet node has a publicly trusted certificate |
| N3 | High | contacts.rs, mailbox.rs, Hailing.kt | Public cards reveal the poster's persona and the claimant's identity to every board reader | **half fixed 2026-09-14** (W4): the claimant's identity is sealed to the issuer and a board reader now finds noise in subkey 1. The poster's own persona is still named by the card it publishes, so every listing and hail that persona posts is linkable to it and to its threads. The fix is a per-listing pseudonymous persona, and it is **a decision, not an implementation**: a throwaway identity has no burn, no receipts and no vouches, so it would post a listing that §9.5's own gate tells buyers to distrust. Whatever resolves it has to say how a per-listing name inherits — or proves — what its owner paid for. |
| N4 | High | stigmerge index.rs, fileindex, swarm.rs, Mailbox.kt | A share's index declares any size and the fetcher believes it | **fixed** (ab3aba7d): index shape checked at decode, byte ceiling per kind before any file is created |
| N5 | High (S) | stigmerge seeder.rs | Seeder spawns an unbounded task per block request behind a lock | **fixed** (ab3aba7d): bounded queue, eight in flight, reply outside the lock, per-route pacing |
| N6 | Medium | groups.rs, Groups.kt | Any member can wedge or partition a group with one roster | see W2 |
| N7 | Medium | mailbox.rs, Mailbox.kt, contacts.rs | A stale record holder makes the reader dead-letter the current message | **mostly in place, reviewed 2026-09-14** — both clients already hold a patience window per sequence (`STUCK_PATIENCE_MS`), recognise a slot still holding its previous tenant by content hash, and break rather than advance; an empty read waits. What is left is the narrow case the bridge cannot yet name: a read that failed for a transient reason reaches the catch-all and is dead-lettered at once. That wants a distinct `Stale` from `node_dht_get`, and it wants a way to reproduce a stale holder before anybody edits the delivery path. |
| N8 | Medium | mailbox.rs, Mailbox.kt, node.rs | Public cards claimed, burned or silently killed by anyone | **partial**: an unparsable reply is now treated as contested on both clients; K slots / stamped replies deferred |
| N9 | Medium | Mailbox.kt, ContactStore.kt, mailbox.rs | A hostile contact forces 1024 reads and thread rewrites per lap | **open** |
| N10 | Medium | Ceremony.kt, ceremony.rs | Escrow co-signer never bounds the fee in the proposed transaction | **fixed** (e2ab23c6) |
| N11 | Medium | stigmerge block_fetcher.rs, piece_verifier.rs | One hostile mirror poisons a tail piece and is scored a success | **fixed** (ab3aba7d): exact block length, file sized on open, credit on verification, three strikes |
| N12 | Medium | stigmerge header.rs, node.rs | A share key with a secret re-opens our own records with writer None | **open** |
| N13 | Medium | stigmerge peer_gossip.rs | Gossip amplification; unbounded peer tables | **open** |
| N14 | Medium | stigmerge share.rs, piece_verifier.rs | Two panics reachable from a publisher's bytes | **fixed** (ab3aba7d) |
| N15 | Medium | node.rs, geo.rs | Cell boards brickable for a week with writes at seq u32::MAX; future weeks computable | **deferred** |
| N16 | Medium | monero.rs, opinion.rs, SecondOpinion.kt | Sends broadcast to five clearnet nodes even with an own node | **partial** (desk honours the own node; relay list still fans out) |
| N17 | Medium | app/src/publications.rs | Shelf index drives unbounded reads and memory | see W18 |
| N18 | Medium | stigmerge fetcher.rs, swarm.rs | No strike cap; a moving attempt resets the stall budget | **fixed** (ab3aba7d): strikes, and a minimum-throughput rule instead of the reset |
| N19 | Low/Med | Wallet2.kt, wallet.rs | One node's `is_key_image_spent` is final | **fixed 2026-09-14** — our own sends explain their own spends; an unexplained one needs a second node before the note is written off, and when none answers the first node stands with a line in the log |
| N20–N27 | Low/Info | | Beacon over one node; one-hop safety route; settlement at one confirmation (desk now scales confirmations); sender-named record deletion; group timestamps; stigmerge block bounds; bundle budgets; open-once registry ignores the writer | **open** |

## Money flows

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| M1 | High | ui/Kiosk.kt, Orders.kt, orders.rs | Kiosk hands over goods on a mempool sighting with no bond (§15.11 says MUST NOT) | **fixed** (6f3d4e9c): `Seen` renders as settling on both clients; paid panel and Ready need `Confirmed` or an operator cap, default off |
| M2 | High | Ceremony.kt, ceremony.rs | Release consent sized from the scanned balance, not the transaction's inputs; a partial sweep takes the co-signer's stake | **fixed** (e2ab23c6): consent from inputs, outputs and fee; a partial sweep and an unscanned escrow are refused |
| M3 | High | SecondOpinion.kt, opinion.rs, monero.rs | Second opinion defeated by an on-path attacker and by mempool presence | **fixed**, see N2 |
| M4 | High | ui/Chat.kt, Ceremony.kt | The arbiter signs the proposer's payload while shown only the proposer's claim | **fixed** (e2ab23c6): every parsed output listed with its attribution, approval bound to what was printed; an unplaceable payout warns rather than refuses until the payee's address rides the round-0 frame |
| M5 | Medium | Ceremony.kt | The joining party adopts the inviter's fare, stakes and funder index unchecked | **fixed** (e2ab23c6) |
| M6 | Medium | Ledger.kt, ContactStore.kt, ledger.rs | Any contact can relabel, itemise and "tax" a row of the merchant's statement | **fixed 2026-09-14** — a receipt this client wrote wins the transaction id, and a counterparty's receipt for money we *received* is shown and credited by name but writes neither the items nor the tax; both clients, with a desk test |
| M7 | Medium | Donations.kt, donations.rs | A donation receipt is issued for any transaction the wallet received | **fixed 2026-09-14** — a donation is receipted only when the money landed on the subaddress that donate card allocated; a card with none (issued before addresses were published) is left as it was rather than silently breaking a working charity |
| M8 | Medium | ContactStore.kt, backup.rs, Ledger.kt | Restore loses send records; spends become epoch-dated "unexplained" rows | **fixed 2026-09-14** — `wallet_sends` rides the bundle on both clients; round-tripped by the desk test and the shim's backuptest |
| M9 | Medium | ui/Pay.kt, Pin.kt | Stale-rate rule and payer verification policy not applied | **half fixed 2026-09-14**: the stale-rate half is done — a payment typed in fiat says how old the rate behind it is, where the amount is, once it is past half an hour. The payer-verification half is **a decision, not a defect**: `core/src/verify.rs` holds a complete, tested ladder that nothing calls, and both clients are currently *stricter* than it — the phone asks for the PIN on every send. Applying the policy would ask for less on small payments, which is a choice about friction, not a fix. |
| M10 | Low | Orders.kt, orders.rs | Pool-sighted orders promoted without the second opinion | **fixed** (6f3d4e9c) |
| M11 | Low | Ceremony.kt | Consent TOCTOU on a superseding proposal | **fixed** (e2ab23c6): the tap carries the displayed figure and a digest |
| M12 | Low | Publications.kt | An ask can be billed twice by two polls | **fixed 2026-09-14** — a subscriber's bill is claimed inside one edit before it goes and released if it does not, and the desk holds a lock per publication and period; the send-intent contract, applied to a bill |
| M13 | Low | Orders.kt | A code paid after expiry lands unmatched | **fixed 2026-09-14** — an abandoned order stays matchable for a day, so a late payer's money finds what they ordered; the match is still the exact amount on the order's own subaddress |
| M14 | Info | core/ | Part IV (`fast/1`, bonds, slash claims, market arbiter set) exists only in core | **open** — see research/post-1.0/TRUST.md |
| M15 | Info | pay.rs vs ui/Pay.kt; contacts.rs vs ContactStore.kt | Client divergences: bill payto vs contact address; receipt dedupe order | **open** |
| M16 | Info | catalogue.rs | Unchecked multiply in `parse_money` | **fixed** in tree |

## Checked and found sound (across the five reviews)

CBOR decoder bounds and canonical form; domain-separated signing over received
bytes; strict readers with every pairing rule; board seal, stamp and beacon
window; group page AAD and probe field; position frames; publication chunk
sealing; escrow round ordering, arbiter-set argument, destination confinement;
send-intent ledger contract; bills-to-receipts arithmetic and short-payment
refusal; DHT value validation in Veilid before use; safety routes by default;
bounded inbound queues; uniffi panic containment; swarm path containment at
decode and before every write; attachment hashing before decrypting; SafeImage
bounds; notification visibility; log redaction; PIN KDF and lockout; site viewer
walls on both clients; sealed-room IPC isolation and CSP on the desk; no XSS
sinks in the desk pages; backup AEAD and KDF; rate oracle agreement rule.
