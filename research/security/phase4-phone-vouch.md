# Phase 4 on the phone — §9.2 vouching

Date: 2026-09-14. Built on the working tree after the desk's vouching landed (`app/src/trust.rs`, the vouch section; `app/src/backup.rs`; `applications/desk/src/lib/Chat.svelte`). Spec: ducat-protocol.md §9.2, "Vouching" (object type 29, fields 317–319). Reference: the desk, ported rule for rule. Follows `phase4-phone-trust.md`, which is where the receipts pattern this copies is explained. Not committed.

## What was built

### Trust.kt (`applications/android/src/main/java/org/ducatproject/ducat/Trust.kt`) — the §9.2 vouch section

Same object, same style as the receipts: `securePrefs("ducat_trust")`, whole-list writes under `lock`, re-read inside the lock.

- **Three shelves**, `vouches_given` / `vouches_received` / `vouches_about`, rows of `VouchRecord(signerHex, subjectHex, ts, envelopeHex)`. JSON names `signer, subject, ts, envelope` — the desk's `VouchRecord` serde names — so a bundle restores either way.
- **Links**: `ducat:vouch/<hex>` and `ducat:vouches/<hex>.<hex>…` join `Trust.Link` as `Vouch(hex)` / `Vouches(dotted)`. `linkIn` recognises them under the same whole-body rule as the other three (prefix at the start, payload to the end, no whitespace, hex or dotted hex); `ducat:vouches/` is matched as itself, never as `ducat:vouch/` with `es/…` for hex. `linkWords` gives the two kinds their one-line words, so `ChatList.previewOf` and `Notify.kt` — which already go through it — preview them as words, not hex.
- `vouch(context, personaHex, subjectHex)` → refuses subject == signer **before** looking for a key (`selfVouch`, case- and whitespace-insensitive); signs through `vouchSign(VouchIn(secret, subject, now))`; reads the envelope back through `vouchOpen` so the shelf holds what the envelope says; keeps **one per subject** on `given` (`givenWith`); returns `ducat:vouch/<hex>`. `AttestError`s from the bridge (nowhen, oneself, bad secret) surface as "The vouch could not be signed."
- `vouchedFor(context, subjectHex)` — any given row about that subject.
- `receiveVouch(context, fromHex, hex)`: opens; **signer must equal the sender**, **subject must be one of this phone's personas** (`PersonaStore.allHexes`), else dropped with the reason in the log; **one per (signer, subject)** (`withVouch`); stored on `received`. Null when dropped.
- `myVouchesLink(context, personaHex)`: the `received` vouches about that persona, newest first, packed into **one** message (`vouchesLinkOf` → `packLink`, the same packing `recordLinkOf` now uses: as many as fit `MAX_LINK_CHARS = 2000`, never more than 64); null when none.
- `readVouches(context, fromHex, dotted)`: split on `.`, at most 64; each envelope that opens and is about the sender goes on `about`, one per (signer, subject); the rest dropped without comment; returns `knownBy(sender)`.
- `knownBy(context, personaHex)`: display names of **this phone's contacts** (`ContactStore(context).all()`, `displayName()`) that signed a vouch about that persona, **excluding this phone's own personas**, sorted, distinct. Computed locally from the `about` shelf and never sent. Pure core: `knownByOf(about, personaHex, mine, nameOf)`.
- `ingestTrustLinks` routes `Vouch` → `receiveVouch`, `Vouches` → `readVouches`; it is reached from `ContactStore.appendAndAdvance` for every inbound kind-0 row, as before.
- **Backup**: the three shelves join `BACKUP` as `vouches_given_raw` / `vouches_received_raw` / `vouches_about_raw` — the desk's names. `ContactStore` export writes `Trust.backupEntries` and import walks `Trust.BACKUP_KEYS` through `Trust.restoreEntry`, so they ride the bundle with no change to `ContactStore.kt` (checked: lines ~1193 and ~1300).

### The chat screen (`ui/Chat.kt`)

- **Header line**: `TrustView` grew `knownBy`, `vouched`, `myVouchesLink`, read on the same IO beat as the thread (`LaunchedEffect(version)`), so reading a shown set of vouches — which bumps the store — refreshes it. `trustBadge` appends `knownWords(knownBy)`: "Pat knows them" (1), "Pat and Sam know them" (2), "3 of your contacts know them" (>2); nothing when none, like the rest of the badge.
- **Tray**: a third row under burn/record/rate — **I know them** (`Icons.Filled.Handshake`; reads **You vouched for them** and is disabled once `vouched`; also disabled with no keys or while sending) and **Show who knows me** (`Icons.Filled.Group`; disabled when `myVouchesLink` is null). Both go through the screen's one send door (`send(what) { … }` → `ThreadSends`), signing under `PersonaStore.ownerHexOf(c)` — the persona the thread speaks as, so the reader's signer == sender rule holds on a phone with several personas.
- **Bubbles** (`TrustBubble`, which now takes `knownBy`): `ducat:vouch/` → "You said you know them" / "They say they know you; kept with your vouches"; `ducat:vouches/` → "Who knows you, shown to them" / the known-by words, or "They showed who knows them; none of them are your contacts". Never the hex; long-press still copies the body verbatim.

### Strings

16 new `trust_*` keys in `strings_trust.xml` across all 20 locale folders, wording mirrored from the desk's `desk_*` vouch keys (translated per locale from the desk's own translations where one exists) and the phone's existing `trust_*` phrasing for the rest: `trust_and`, `trust_known_by_one`, `trust_known_by_names`, `trust_known_by_count`, `trust_vouch`, `trust_vouched`, `trust_show_my_vouches`, `trust_vouch_sent`, `trust_vouch_received`, `trust_vouches_sent`, `trust_vouches_none_known`, `trust_vouches_preview`, `trust_what_vouch`, `trust_what_vouches`, `trust_err_vouch_self`, `trust_err_vouch_unsigned`. No desk key reused. Every key is referenced by code.

### Tests

`applications/android/src/test/java/org/ducatproject/ducat/TrustVouchTest.kt` (6 tests, JVM, no Context, no bridge): the two prefixes and the whole-body rule, and that `vouches/` is not read as `vouch/`; one per (signer, subject) with the newest replacing; one per subject on `given` whichever persona signed; `knownByOf` counting only the reader's contacts, never its own personas (case-insensitively), one name per signer, deduped names, nothing for a persona nothing was read about; the packing (this persona's only, newest first, fits one message, capped at 64, reads back through `linkIn`/`splitRecord`); `selfVouch` refusing the same hex however cased or padded.

## Deviations from the brief, and why

1. **`trust_and` carries its own spaces and there is a singular key.** The desk joins two names with `t("desk_and")`, whose value is `and` — so the desk's header reads "PatandSam know them", and with one name "Pat know them". Not copied: the phone's `trust_and` is the quoted `" and "` (aapt keeps whitespace inside quotes; `と`/`和` for ja/zh have none), and `trust_known_by_one` says "Pat knows them". Three-name-and-up wording is the desk's. The desk's join is a one-line fix in `Chat.svelte` or `strings_desk.xml` (`" and "`), left alone because the desk is out of scope for this pass.
2. **No hint strings.** `desk_vouch_hint` and `desk_show_my_vouches_hint` are tooltips; the phone's `TrayItem` has a label and no tooltip (the burn/record items have none either), so they have no phone twin.
3. **The signing persona is the thread's owner, not the worn one** — the same deviation as the receipts pass, for the same reason: `Mailbox.send` sends under `ownerHexOf(contact)`, and a vouch signed by any other persona would arrive with signer ≠ sender and be dropped by the rule this pass implements.
4. **`knownBy` runs on the screen's IO beat, not in the bubble.** `ContactStore(context).all()` decrypts every contact, so the names are computed once per store bump in the thread's `LaunchedEffect` and handed to the bubble, rather than looked up per row.
5. **`myVouchesLink` packs to one message**, as `myRecordLink` does — a vouch envelope is ~300 hex characters, so six or so fit under the 2000-character text cap, newest first. The desk joins everything and would be refused past that.
6. **The gates ran with `-x :android:nativeFreshness` / `-x :desktop:deskNativeFreshness`**, as instructed; the JVM tests never load the native library.

## What is left

- **Live proof.** No two-phone or phone↔desk walk yet: I know them → vouch lands on the subject's `received` → Show who knows me → reader's `about`, header and bubble. The emulator was in use by another agent during this pass; the walk is next (memory: test locally, not releases).
- **The badge outside the thread** — listings, hails, the till — still has no phone twin of the desk's `trust_of`; "2 of your contacts know them" belongs on those cards too.
- **The known-by words in a `vouches/` bubble are the thread's current answer**, not the answer at the time the message arrived: adding Sam as a contact later makes an old bubble say "Pat and Sam know them". That is the desk's behaviour too (`current.known_by`), and it is the right one — a vouch is re-weighed by every reader from what it holds now.
- **Group threads** are not ingested (same as the desk).
- **A pre-existing compiler warning** at `Chat.kt:5593` ("Condition is always 'false'", in the rent/stakes code) is unrelated to this pass.

## Gate output

```
python3 applications/check_strings.py
strings — 49 files × 19 languages agree

python3 applications/desk/scripts/strings_from_android.py
740 key(s) across 20 locale(s)
python3 applications/desk/scripts/strings_from_android.py --check
740 key(s) across 20 locale(s) — up to date

./gradlew :android:assembleDebug -x :android:nativeFreshness      BUILD SUCCESSFUL in 54s   EXIT=0
./gradlew :android:testDebugUnitTest -x :android:nativeFreshness  BUILD SUCCESSFUL in 3s    EXIT=0
  LedgerTest 7, ReferentTest 7, ReleaseArithmeticTest 5, SecondOpinionRuleTest 9,
  TabBillTest 5, TrustReceiptTest 8, TrustRuleTest 6, TrustVouchTest 6 — 53 tests, 0 failures
./gradlew :desktop:compileKotlin -x :desktop:deskNativeFreshness   BUILD SUCCESSFUL in 15s   EXIT=0
./gradlew --stop
```

(The first run of the unit tests failed one vouch test: `knownByOf` compared lowercased signers against `mine` as given, so an upper-cased own persona slipped through. Fixed by normalising `mine` inside the pure function rather than trusting the caller; the rerun is the line above.)
