# Phase 4 on the phone — §9.5's seller's minimum, and the badge outside the thread

Date: 2026-09-14. Built on the working tree after the desk's two pieces landed (`applications/desk/src/lib/Market.svelte`, `Till.svelte`, `app/src/listings.rs`, the bridge's `RentalInfo.min_burn_pxmr`). Spec: ducat-protocol.md §16.18 (`RENTAL_NOTICE` field 320, `min_burn`) and §9.5 ("What a client shows and does": the badge in words wherever a decision is made; a seller MAY publish a minimum, a buyer's client MUST show it before the buyer commits). Reference: the desk, ported rule for rule. Follows `phase4-phone-trust.md` and `phase4-phone-vouch.md`, whose "what is left" both named these two. Not committed.

## What was built

### 1. The listing's minimum

**`Listings.kt`** (`applications/android/src/main/java/org/ducatproject/ducat/Listings.kt`)

- The draft record carries `minBurnPxmr` (the desk's `Listing.min_burn_pxmr`, serde name `minBurnPxmr`, default 0): `draft(…, minBurnPxmr: Long = 0L)` writes it, never negative; `minBurnOf(o)` reads it, zero for a record written before the field existed.
- `publicNotice(o, card)` puts it on the wire object (`RentalInfo.minBurnPxmr`). The bridge writes field 320 only when non-zero and refuses a written zero; the phone only carries the number.
- The found-listing cache (`rowToJson` / `rowFromJson`, what the browse screen paints from between sweeps) carries it as `min_burn` — the desk's `Found.min_burn` name — so a phone bundle of remembered boards reads the same on either client.
- `minBurnPxmrOf(text): Long?` — the poster's field to piconero. Empty is 0 (asks nothing); a figure goes through `Amounts.parse` / `Amounts.toPxmr` like every money field (any script's digits, either decimal mark); not a number, a negative, or a figure past the wire's integer is **null**, which the form treats as a refusal. A typed zero is nothing typed, because the wire refuses a written zero.
- `isMine(context, card)` — whether a found notice is one of ours: its card is one a listing here minted (`card` or the `cards` history). See deviation 2.

**`ui/Renting.kt`** — the poster's form: a "Minimum burn for buyers (XMR)" field after "How many", with the desk's hint under it, the money-field character filter, `isError` when `minBurnPxmrOf` refuses, and the Post button held until it does not. Empty is the default and asks nothing. The draft call passes `minBurnPxmr`.

**`ui/RentSearch.kt`** — the opened listing (`ListingSheet`):
- After the stake line, when the notice carries a minimum: "The seller asks that buyers have burned at least X".
- Under it, for a listing that is not ours, the worn persona's own largest finished burn (`Trust.myBurn(context, PersonaStore.worn())`) against the minimum through `Trust.againstMinimum`: "This persona has burned X, less than they ask. They may not deal with you." or "This persona has burned nothing yet. They may not deal with you." — in the error colour, before the Ask button, which stays enabled. **Warn, never refuse.**
- The three facts (ours or not, the poster's badge, my burn) are read together off the main thread into one nullable `ListingSeen` on every store bump, so the sheet says nothing until it knows rather than warning for a frame and taking it back.

**`Trust.kt`** — `enum Minimum { MET, SHORT, NOTHING }` and `againstMinimum(myBurnPxmr: Long?, minBurnPxmr: Long)`: nothing asked (≤ 0) is met; at or over the line is met; under it, SHORT when something was burned and NOTHING when nothing (null, zero and the impossible negatives alike). The desk's `Market.svelte` expression, as a pure function.

### 2. The badge outside the thread

**`ui/TrustBadge.kt`** (new; added to `applications/desktop/build.gradle.kts`'s `sharedLogic`) — Chat.kt's header builder factored out, not copied: `trustBadge(burn, record, knownBy, sayNone = false): String?` says "Burned 0.01 XMR, since block N" (`trust_burned_since`), "3 receipts, 1 from burned personas · 4.7 ★" (`trust_receipts_summary`, the star average only when `weighted > 0`), and `knownWords` — "Pat knows them" / "Pat and Sam know them" / "3 of your contacts know them". Null when nothing is known. `sayNone` is the listing's exception: on a poster, "No burn of theirs checked here" (`trust_no_burn_known`) stands in the burn's place, as the desk's Market page does. `Chat.kt`'s private `trustBadge(TrustView)` now delegates to it and its private `knownWords` is gone (the bubble uses the shared one).

**`Trust.kt`** — `Badge(burn, record, knownBy)` with `badgeOf(context, personaHex)` (the desk's `trust_of`, minus the links, which belong to the thread that sends them) and `badgesOf(context, personaHexes)` for a picker's rows: every shelf read once, and the contact book — which `knownBy` decrypts whole — opened only when a vouch is about one of the rows at all.

Where it is worn:
- **The found listing's poster** (`RentSearch.kt`, under the title, not on our own listing): `Trust.badgeOf(context, info.poster)` with `sayNone`. See deviation 1 — in practice this is the "No burn of theirs checked here" line until the card is claimed.
- **The ride offer** (`ui/Ceremony.kt`, `RideOfferScreen` — the rider's yes to a driver's fare; the contact is known there): the badge under "offers to drive", read on IO. Nothing when nothing is known.
- **The tab's customer picker** (`ui/BarTab.kt`, `OpenTab`'s "or a regular" rows): the badge under each name, from one `badgesOf` pass with the rows. The desk's Till row, wording for wording (plus the star average, which the phone's builder has always carried).

### Strings

Six new keys in all 20 locale folders, every one referenced by code, no `desk_` key reused. `strings_renting.xml`: `rent_min_burn_label`, `rent_min_burn_hint`, `rent_min_burn_x`, `rent_min_burn_short`, `rent_min_burn_none`. `strings_trust.xml`: `trust_no_burn_known`. The nineteen translations are the desk's own translations of the same six sentences (`applications/desk/strings/values-*/strings_desk.xml`), copied raw so the two clients say exactly the same thing in every language.

### Tests

`applications/android/src/test/java/org/ducatproject/ducat/TrustMinimumTest.kt` (6 tests, JVM, no Context, no bridge): nothing asked is met whatever was burned; at or over the line is met, the line itself included; under it SHORT vs NOTHING with null, zero and negative burns all NOTHING; empty and a typed zero ask nothing; "0.01", "0,01", " 2.5 ", Arabic-Indic digits with their own decimal mark, and a thirteenth decimal place truncated; "abc", "1.2.3", negatives and a figure past 2^63 piconero refused.

## Deviations from the brief, and why

1. **The listing's badge is computed over the poster key, and that key is per listing.** `RentalInfo.poster` is `SHA-256("DUCAT-LISTING-v0" ‖ persona_secret ‖ listing_id)`-derived (§16.18.1), never a persona, so nothing this phone verified — no burn, no receipt, no vouch — is ever filed under it, and the line on an opened listing is always "No burn of theirs checked here" until the card is claimed. The phone holds no mapping from a found listing to a persona before the claim (the seeker's `Enquiries.remember` is keyed by the contact *after* it, and a listing's card rotates every refresh), and none was invented, as instructed. The desk does exactly the same call (`api.trustOf(f.poster)`) with the same result. Where the persona *is* known — the enquiry thread the claim opens — the header already wears the full badge. If a later draft lets a listing carry a persona-bound proof, `badgeOf(context, info.poster)` is already the call.
2. **"Ours" is decided by the card, not the poster key.** The desk's `found_row` sets `mine = persona_hexes.contains(f.poster)`, which for the same reason as above is never true. The phone asks the one fact it holds: whether the notice's card is one a listing here minted (`Listings.isMine`). This is what keeps a seeker from being warned against their own minimum on their own kayak. Not a mapping to a persona — the phone minted the card.
3. **The warning sentence uses `Amounts.show(…).primary`**, as the thread's badge and Pay's gate already do, rather than the desk's `fmtXmr` (always XMR). On one screen the minimum and the buyer's own burn must read in the same unit, and the phone's existing rule is the preference (XMR whenever there is no rate). The poster's *field* is XMR whatever the price was typed in, because a burn is an XMR quantity and a rate would move the line the seller drew.
4. **No extra "that is not a number" sentence.** The desk silently zeroes a bad entry; the phone refuses it (field in error, Post held) with the hint as the supporting text in the error colour. A dedicated sentence would be one more key across twenty locales for a state the character filter already makes hard to reach; left out on purpose.
5. **The owner's own listing row does not say its minimum** (`MyListingCard`), because the desk's does not either; the form is where the owner set it and the opened listing is where a buyer reads it.
6. **The POS till has no picker to badge.** `Pos.kt`'s customer arrives by claiming a card and the bill goes out on the claim; there is no moment where the till chooses a persona. The desk's Till rows correspond to the phone's bar tab "or a regular" list, which is where the badge went.
7. **The driver's side of a hail has no persona to badge** before the claim (a `HAIL_NOTICE` carries only a card), and `RideConfirmed` / `DriverFound` are after the decision, so the ride's badge is on the rider's `RideOfferScreen` alone.
8. **Gates ran with `-x :android:nativeFreshness` / `-x :desktop:deskNativeFreshness`**, as instructed.

## What is left

- **Live proof.** No emulator or two-phone walk this pass (adb and the emulator were off limits): post with a minimum → the notice on the board carries field 320 → a second phone opens it and reads "The seller asks…" and, with no burn, "…burned nothing yet…"; then burns, and the sentence changes. The desk walk proved the wire; the phone's screens are compiled and unit-tested arithmetic.
- **The listing's badge stays the none-line** until a listing can be tied to a persona (deviation 1). That is a protocol question, not a phone one.
- **The star average on the tab's rows** is the phone builder's; the desk's Till row omits it. One of the two should give way; the phone's is the brief's wording.
- **First burn's block age** ("since March") is still not shown; the badge says the block number, as the desk does.

## Gate output

```
python3 applications/check_strings.py
strings — 49 files × 19 languages agree

python3 applications/desk/scripts/strings_from_android.py
746 key(s) across 20 locale(s)
python3 applications/desk/scripts/strings_from_android.py --check
746 key(s) across 20 locale(s) — up to date

./gradlew :android:assembleDebug -x :android:nativeFreshness      BUILD SUCCESSFUL in 55s
./gradlew :android:testDebugUnitTest -x :android:nativeFreshness  BUILD SUCCESSFUL in 4s
  LedgerTest 7, ReferentTest 7, ReleaseArithmeticTest 5, SecondOpinionRuleTest 9,
  TabBillTest 5, TrustMinimumTest 6, TrustReceiptTest 8, TrustRuleTest 6,
  TrustVouchTest 6 — 59 tests, 0 failures
./gradlew :desktop:compileKotlin -x :desktop:deskNativeFreshness   BUILD SUCCESSFUL in 16s
./gradlew --stop
```

(The first `assembleDebug` failed on my own edit: a data class inserted between `ListingSheet`'s `@OptIn(ExperimentalMaterial3Api)` and its `@Composable` took the annotation with it. Moved above the sheet's doc comment; the line above is the rerun. The one remaining warning, `Renting.kt:253` deprecated `Icons.Filled.MenuBook`, predates this pass.)
