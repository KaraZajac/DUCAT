# Phase 4.1, phone side — burning under a persona, keeping the proof, checking a stranger's

*2026-09-14. §9.5 on the Android client, following §8 step 4.1 of
[research/post-1.0/TRUST.md](../post-1.0/TRUST.md) and the desk's finished
`app/src/trust.rs`. Not committed.*

Gates green: `cargo test -p ducat-mobile --lib` (66 passed, 1 ignored),
`:android:assembleDebug`, `:android:testDebugUnitTest` (39 tests, 6 of them
new), `:desktop:compileKotlin`, `python3 applications/check_strings.py`
(48 files × 19 languages agree), `python3
applications/desk/scripts/strings_from_android.py` + `--check` (728 keys,
up to date — the desk pages use none of the new ones yet). Bindings
regenerated with `DUCAT_ABIS="x86_64-linux-android:x86_64"`; the expected
STALE arm64/armv7 warning is in the log. `applications/gradlew --stop` run.

---

## What was built

### The bridge — `mobile/src/txproof.rs`

The bindings had the proof half of §9.5 (`monero_burn_address`,
`monero_make_out_proof`, `monero_verify_out_proof`, `monero_tx_status`,
`monero_second_opinion_nodes`) and no way to *seal* a proof, so Kotlin could
not have produced a `BURN_PROOF` envelope without reimplementing §18.3 in a
screen's language. Two exports were added, wrapping `ducat_core::trust`:

- `burn_proof_sign(input: BurnProofIn) -> Vec<u8>` — builds the `BurnProof`
  (version 1, suite 1), derives the persona **from the secret** so the
  envelope and the name inside it cannot disagree, reads the object back
  through `BurnProof::from_value` — so every refusal a reader would make
  (nothing burned, no block, an in-proof where an out-proof belongs, a torn
  proof, a purpose over 32 characters) happens here rather than in front of
  the stranger it was handed to — and seals it with `sign_burn_proof`.
- `burn_proof_open(envelope) -> BurnProofView` — `open_burn_proof`, plus the
  message the out-proof must have been made over, handed out rather than
  rebuilt by the caller so the two cannot differ by a separator.

**One deviation from the brief, deliberate.** The brief spelled
`burn_proof_sign` as six positional arguments. It takes one record instead:
[[uniffi-struct-args-crash-arm64]] — many by-value `RustBuffer` arguments
segfault on arm64 and on nothing we can test here, and the rule that came out
of `seal_message` is "a uniffi export takes ONE record". `burn_proof_open`
takes the single envelope.

New unit test
`a_signed_burn_proof_opens_to_what_was_signed_and_refuses_what_the_wire_refuses`:
signs a proof made over a synthetic burn to the stagenet burn address, opens
it, checks every field round-trips, checks the message it hands back is the
one `check_out_proof_v2` accepts, then walks six refusals and a tampered
envelope.

### The store — `applications/android/src/main/java/org/ducatproject/ducat/Trust.kt` (new)

`object Trust`, `securePrefs(context, "ducat_trust")`, two JSON lists. It is
`app/src/trust.rs` in Kotlin, rule for rule:

- `burn(context, personaHex, amountPxmr, purpose)` — refuses under the floor
  (10 000 000 000 pXMR) and on an empty or over-long purpose *before* the
  money moves, checks the persona is one this phone can sign for, sends to
  `moneroBurnAddress(stagenet)` through `Wallet.send(..., note = "burn")`,
  and makes the `OutProofV2` from `SendResult.txKeyHex` **at once**. A proof
  that fails to build still writes the record (with an empty proof and a row
  that says so), because the money is gone either way.
- `burnLap(context)` — for records with no envelope, asks the node in use for
  the transaction's status; `InBlock` with a height over zero signs the
  envelope through `burnProofSign` and stores it with the height. Only the
  node in use is asked: this is our own burn, and a node lying about the
  height only produces an envelope the *reader's* two nodes then refuse.
- `verifyBurn(context, personaHex, envelopeHex)` — §9.5's three questions in
  order: the opened proof must name the persona presenting it; the amount
  must clear the floor; our own node must bear the out-proof out **for
  exactly the amount the proof proves** (`verified.amountPxmr != claimed` is
  a refusal, never a rounding) with a height over zero; and at least one
  second-opinion node (`moneroSecondOpinionNodes`, never the node in use,
  never the operator's own) must answer `InBlock`. Pool, unknown and
  unreachable are all "not yet". The verdict is kept and never expires.
- `myBurn` / `burnOf` take the **largest** burn, never a sum — §9.5's
  JoinMarket rule, so many small identities do not add to one large one.
- Pure and testable: `refusal(amount, purpose)` returns an enum (the wording
  is chosen at the one call that holds a `Context`), and `burnMessage`
  builds `"DUCAT-BURN-v1" ‖ 0x00 ‖ persona (32 bytes) ‖ 0x00 ‖ purpose`
  exactly as `core/src/trust.rs::burn_message` does.
- Both whole-list writes are under one lock, and re-read inside it: the sweep
  and the screen both write the list back, and a stale snapshot would drop a
  proof somebody paid for. An unparseable store answers empty with a warning
  rather than taking the wallet screen down with it.

### The screen — `ui/Burn.kt` (new), reached from `ui/Accounts.kt`

A full-screen `Dialog` + `Scaffold`, the pattern `TxDetail.kt` already uses
inside a tab, with `BackHandler`. In reading order: what a burn is
(irreversible, nothing comes back), what it buys (a proof bound to this name,
shown to one reader, never published), the persona it is under, the amount
(default 0.01) and purpose (default "identity") fields with the floor stated
under the amount, the irreversibility line, the Burn button — `PinGate` with
`why = burn_pin_why` between the button and the send, as `ui/Pay.kt` has it —
then the burn address with its derivation explained and a copy action, then
this persona's burns with "Waiting for its block" / "In block N" and
**Copy the proof** on the finished ones. Rounded rectangles (`Card`,
`Button`, `OutlinedButton`), no chips.

The wallet screen (`ui/Accounts.kt`) grew one card under the balances —
title, one sentence, "Burned 0.01 XMR" when this persona has a finished burn,
and the button that opens the screen.

### The lap — `Poller.kt`

One line in the sweep, beside the mempool work and before the card prune:
`runCatching { Trust.burnLap(context) }`.

### Strings

`res/values/strings_burn.xml` and all nineteen locale folders, 39 keys each
(spread the way `net_reconnect` is). `check_strings.py` passes: placeholders
match, apostrophes escaped, nothing written in a script the locale does not
use.

### Tests

`applications/android/src/test/java/org/ducatproject/ducat/TrustRuleTest.kt`
— six JVM tests over the pure half: the message's exact bytes (asserted
literally, not against a second copy of the builder, because a phone that
laid it out differently would only find out from a stranger who could not
verify a burn that had already cost money), that a different name or purpose
is a different message, that a trailing space in the label does not change
it, UTF-8 purposes, the floor as a refusal at exactly `FLOOR_PXMR - 1` and a
burn at the floor, a purpose counted in code points (sixteen emoji fit), and
the hex round trip.

## Decisions

1. **`Trust.kt` and `ui/Burn.kt` cross to the old Kotlin desk**
   (`applications/desktop/build.gradle.kts`). Not optional: `ui/Accounts.kt`
   is already compiled there, and a wallet screen that names `BurnScreen`
   does not compile without it. Neither file touches `Poller` or
   `DucatApplication`; both go through the shim's `securePrefs`, `Wallet`,
   `PersonaStore` and generated `R`.
2. **The floor is restated in Kotlin** (`Trust.FLOOR_PXMR`) rather than read
   from the bridge, because §9.5 makes it a client policy, not a wire rule.
   The unit test pins it to `core`'s 10 000 000 000.
3. **The envelope is signed on the lap, not at send time**, exactly as the
   desk does it — a burn without a block is not a burn yet, and `from_value`
   refuses `height == 0` anyway.
4. **The screen shows only the worn persona's burns.** Listing another
   compartment's here would tie two personas together on the one screen whose
   whole purpose is that they are not tied.

## Left for later

- **`Trust.verifyBurn` has no caller yet.** It is the store half of step 4.2
  (the badge on listings, hails, tills and threads; the gate; "ask for their
  record"), which is the next step and owns the UI that would call it. It is
  tested only through the bridge's own round trip; the first end-to-end
  exercise of it will be a burned seller and an unburned one on two devices.
- **Nothing rides the backup or the attestation record yet.** §9.5 says the
  proof lives in the persona's attestation record and rides the backup with
  it; `attestation_records` is still the empty slot it has always been, so a
  restored phone loses its envelopes. Step 3.3's "restored from a backup,
  still verifies" is therefore *not* proven on the phone.
- **No live burn was made from the phone.** Emulators were out of scope for
  this pass, so the path is proven by construction (the desk made the first
  real burn — txid `0b1a7ac3…9301b`, block 2207293) and by the bridge tests,
  not by a stagenet send from an Android build.
- **A burn whose `OutProofV2` failed to build can never be proved**, by
  construction: the transaction key is not kept. The record says so and the
  screen shows it; the money is spent either way.
- **Concurrent work in the same tree.** `mobile/src/attest.rs` and the
  `pub mod attest;` line in `mobile/src/lib.rs` are another pass's (§9.2's
  attestations); the regenerated `uniffi/ducat_mobile.kt` in this pass does
  **not** carry its exports, so whoever lands second must regenerate the
  bindings again.
