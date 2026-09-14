# Phase 4.2 + 4.3 on the phone — the badge, the gate, show/check a burn, rated receipts, the record

Date: 2026-09-14. Built on 969d290b (phone burn + bridge exports). Plan rows: TRUST.md §8, 4.2 and 4.3. Spec: ducat-protocol.md §9.2, §9.5 ("What a client shows and does", "How a proof travels", "How a receipt travels"). Reference: the desk's `app/src/trust.rs`, ported rule for rule. Not committed.

## What was built

### Trust.kt (`applications/android/src/main/java/org/ducatproject/ducat/Trust.kt`) — the §9.2 section

Same object, same style: `securePrefs("ducat_trust")`, whole-list writes under `lock`, re-read inside the lock.

- **Three shelves**, `given` / `received` / `about`, rows of `AttestationRecord(signerHex, subjectHex, amountPxmr, rating, ts, txidHex?, note?, envelopeHex)`. JSON names are the ones both clients now write: `signer, subject, amount, rating, ts, txid (omitted when null), note (omitted when null), envelope`. The burn shelves keep their existing names (`persona/txid/amount/purpose/proof/height/envelope/made`, `persona/txid/amount/height/purpose/checked`).
- **Links**: `ducat:burn/<hex>`, `ducat:attest/<hex>`, `ducat:record/<hex>.<hex>…`. `Trust.linkIn(body)` recognises them under the desk's ingest rule — trimmed body, prefix at the start, payload to the end, no whitespace inside — and that one parser serves the shelf, the bubble, the chat-list preview and the notification, so the two clients agree about which messages were receipts.
- `attest(context, personaHex, subjectHex, amountPxmr, rating, note?, txidHex?)` → signs through `attestationSign` (one record, arm64-safe), refuses rating outside 1..5, a note over 140 characters, subject == signer, no key for the persona; reads the envelope back through `attestationOpen` so the shelf holds what the envelope says; stores on `given`; returns the link.
- `receiveAttestation(context, fromHex, envelopeHex)`: opens; **signer must equal the sender** and **subject must be one of this phone's personas** (`PersonaStore.allHexes`), else dropped with the reason in the log; dedupe on (signer, ts); stored on `received`.
- `myRecordLink(context, personaHex)`: the `received` envelopes whose subject is that persona, as `ducat:record/…`; null when none.
- `readRecord(context, fromHex, dotted)`: split on `.`, at most 64, skip what does not open or whose subject ≠ the sender; dedupe on (signer, subject, ts); stored on `about`; returns `recordOf(sender)`.
- `recordOf(context, personaHex)` → `RecordSummary(receipts, weighted, ratingX10)`: receipts = every `about` row for the subject; weighted = distinct signers with a burn this phone verified (`verifiedBurns`), one voice per signer, rated by that signer's **latest** receipt; ratingX10 = integer mean × 10 (the desk's sum, the desk's division); zero when none.
- `ingestTrustLinks(context, fromHex, body)`: attest → receive, record → read, burn → nothing (checked by the button, since the check costs two node round trips). Never throws.
- **Pure pieces for tests**: `linkIn`, `splitRecord`, `summarize(about, burned)`, `recordLinkOf(received, personaHex)`.
- **Backup**: `BACKUP_KEYS`, `backupEntries(context)` (bundle name → JSON text of the shelf, only shelves with anything on them) and `restoreEntry(context, name, json)` (whole-list replace under the lock, through secure prefs; refuses a name it does not know or text that is not a JSON array). Names: `burns_raw`, `verified_burns_raw`, `attestations_given_raw`, `attestations_received_raw`, `attestations_about_raw`.

### The inbox hook

`ContactStore.appendAndAdvance` (`ContactStore.kt`, ~line 675) is the phone's one funnel for an inbound row — the twin of `app/src/contacts.rs::append_and_advance`. Its body is now `appendAndAdvanceLocked(...)` (the old synchronized commit, unchanged) followed, **outside the lock**, by `Trust.ingestTrustLinks(appContext, personaHex, m.body)` for `!m.outgoing && m.kind == 0`, wrapped in `runCatching`. Outside the lock because Trust holds its own lock and bumps the store; after the commit because the row must be durable before anything is concluded from it. Every inbound path in `Mailbox.kt` (nine `appendAndAdvance` callers, including the pairwise copy of a group row) passes through it; group rows via `appendGroupRow`/`append` do not, same as the desk.

### The backup

`ContactStore` export (after `sites_raw`) writes the five `*_raw` entries; import (after `enquiries_raw`) restores each present entry through `Trust.restoreEntry`. A bundle without an entry leaves that shelf alone. With the desk's serde renames landing beside this, a phone bundle restores on a desk and back.

### The chat screen (`ui/Chat.kt`)

- **Header badge**: the top bar title is now the name with, under it, `trustBadge(TrustView)` — "Burned 0.01 XMR, since block N" from `Trust.burnOf`, and "3 receipts, 1 from burned personas · 4.7 ★" from `recordOf`, the star average only when `weighted > 0`. Nothing drawn when nothing is known. Read on the same IO beat as the thread (`LaunchedEffect(version)`), so a verified burn or a read record refreshes it through the store bump.
- **Bubbles** (`TrustBubble`, before the card branch so the card regex cannot draw a QR of a proof): `ducat:burn/` → "A burn proof" + a **Check it** button on incoming ones (runs `Trust.verifyBurn(app, contactHex, hex)` on IO; "Checking…" while it runs; then "Checked: burned X in block N" or the refusal sentence `verifyBurn` throws, which is already in the reader's language; the plain "Could not check it." for anything else). `ducat:attest/` → "Your rating for them" / "A rating for you, kept on your record". `ducat:record/` → "Your record, shown to them" / "Their record: N receipts read". Never the hex; long-press still copies the body verbatim.
- **Actions**: a second row in the composer tray (the `+` panel): **Show my burn** (sends `ducat:burn/<envelope>` of the persona's finished burn; dark when none or no keys yet), **Show my record** (sends `myRecordLink`; dark when none), **Rate them** (only when the thread holds a kind-3 receipt) → `RatingDialog`: five stars, a note capped at 140 characters as typed, the amount named ("For the X that settled between you" — the latest kind-3 receipt's amount), Send. All three go through the screen's one send door (`ThreadSends`), so a failure lands like any other failed send.
- Chat-list preview (`ChatList.previewOf`) and the notification body (`Notify.kt`) say the same words for a link body instead of hex.

### The gate (`ui/Pay.kt`, `AmountStep`)

Under the amount, for a **send to a contact** (not a request, not a bare address): when the amount about to be paid exceeds the contact's verified burn, "More than they have burned (X). If they vanish, nothing holds them."; when none was ever checked, "They have burned nothing you have checked. If they vanish, nothing holds them." Warn, never refuse. A bill being paid is gated the same as a typed amount.

### Strings

New per-screen file `strings_trust.xml` in all 20 locale folders (28 keys, `trust_*`), wording mirrored from the desk's `desk_*` keys for the shared sentences and translated for the new ones. No desk key reused.

### Tests

`applications/android/src/test/java/org/ducatproject/ducat/TrustReceiptTest.kt` (8 tests, JVM, no Context, no bridge): the three prefixes and the whole-body rule; `splitRecord`'s empties and 64-cap; one voice per signer rated by the latest; only verified signers weighed (padding by an unverified signer changes nothing); integer tenths (140/3 = 46, as the desk); the record link carries only this persona's receipts newest first; packing to one message and the 64 cap.

## Deviations from the brief, and why

1. **The signing persona is the thread's owner, not the worn one.** `Mailbox.send` (line ~472) sends to an existing contact under `PersonaStore.ownerHexOf(contact)`, falling back to the worn persona only for a brand-new contact. A receipt signed by the worn persona in a thread another persona owns would arrive with signer ≠ sender and be discarded by the very rule this pass implements. So `attest`, `myRecordLink` and the "Show my burn" link all take `personaHex` and the screen passes `ownerHexOf(c)` — identical to "worn" on a phone with one persona, and the only correct choice on one with several. The desk uses `worn()` because its thread and its worn persona coincide.
2. **`myRecordLink` packs to one message.** The wire caps a text body at `MAX_MESSAGE_CHARS = 2000` (core/src/contact.rs), and an attestation envelope is ~600 hex characters, so 64 envelopes cannot ride one message; the desk joins everything and the bridge would refuse the send past three or four receipts. The phone sends the newest envelopes that fit (never more than 64), newest first because a reader rates each signer by its latest receipt. `Trust.MAX_LINK_CHARS` restates the cap.
3. **The test gate ran with `-x :android:nativeFreshness`.** `:android:testDebugUnitTest` depends on the freshness check, which fails on the coordinator's in-flight Rust change (jniLibs older than `mobile/src/lib.rs`). The check guards the APK's native libraries, which the JVM tests never load; the build gate already excludes it by design.
4. **The txid is not put in the attestation** (the desk passes `None` too). A receipt carries it between the two parties, but a record is shown to strangers; a txid inside would hand every reader the subject's settling transactions. Kept out on purpose; the field exists on the wire if a later draft wants it.
5. **Refusals from `attest` are unreachable from the screen** (the dialog cannot produce a rating outside 1..5, a note over 140, or subject == self), and `ThreadSends` renders any other failure as "Could not send the rating", which is the honest sentence for a bridge fault.

## What is left

- **Live proof.** No two-phone or phone↔desk run yet: rate → receipt lands on the subject's `received` → "Show my record" → reader's `about` and badge. Everything above is unit-tested arithmetic plus compiled screens; the emulator walk is the next step (memory: test locally, not releases).
- **Verdict text is per-bubble state.** A `Check it` verdict is lost when the bubble scrolls out of the lazy column; the verified burn itself is kept in Trust and reappears in the header. Fine for now; a per-message verdict store would be the fix if it grates.
- **A proof pasted with words around it** is drawn as text with a `ducat:…` link (the phone's generic handling) rather than as a proof — deliberate, so the drawing rule equals the ingest rule. The desk draws it as a proof.
- **Group threads** are not ingested (same as the desk).
- **The badge outside the thread** — listings, hails, the till — is not drawn yet; the desk's `trust_of` command has no phone twin. Same words, next pass.
- **First burn's block age** ("since March") is not shown; the badge says the block number, as the desk does.

## Gate output

```
python3 applications/check_strings.py
strings — 49 files × 19 languages agree

python3 applications/desk/scripts/strings_from_android.py
729 key(s) across 20 locale(s)
python3 applications/desk/scripts/strings_from_android.py --check
729 key(s) across 20 locale(s) — up to date

./gradlew :android:assembleDebug -x :android:nativeFreshness      EXIT=0
./gradlew :android:testDebugUnitTest -x :android:nativeFreshness  EXIT=0
  LedgerTest 7, ReferentTest 7, ReleaseArithmeticTest 5, SecondOpinionRuleTest 9,
  TabBillTest 5, TrustReceiptTest 8, TrustRuleTest 6 — 47 tests, 0 failures
./gradlew :desktop:compileKotlin -x :desktop:deskNativeFreshness   EXIT=0
./gradlew --stop
```

(The first run of `:android:testDebugUnitTest` without the exclusion failed at `:android:nativeFreshness`: "native libraries are older than the Rust they were built from. lib.rs changed after jniLibs/armeabi-v7a was written." — the coordinator's Rust change, not this pass's.)
