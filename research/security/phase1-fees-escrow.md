# Phase 1.3 and 1.4 — the fee ceiling, and escrow consent read from the transaction

*2026-09-14. Closes N1, N10, M2, M4, M5, M11 of
[the adversarial review](2026-09-07-adversarial-review.md), per §8 of
[research/post-1.0/TRUST.md](../post-1.0/TRUST.md). Not committed.*

Gates green: `cargo test -p ducat-mobile --lib` (63), `cargo test -p ducat-app`
(67), `:android:assembleDebug`, `:desktop:compileKotlin`,
`:android:testDebugUnitTest`, `python3 applications/check_strings.py`,
`python3 applications/desk/scripts/strings_from_android.py --check`.
Bindings regenerated (`DUCAT_ABIS="x86_64-linux-android:x86_64"`), STALE arm64
warning expected.

---

## 1.3 — the fee ceiling (N1, N10)

### (a) A real `max_per_weight`, on both build paths

`mobile/src/monero.rs` already carried the wallet half from an earlier pass:
`FEE_NORMAL_PER_BYTE = [20_000, 80_000, 320_000, 4_000_000]` (the tiers
stagenet and mainnet both quote), `FEE_CEILING_FACTOR = 50`, and
`fee_ceiling_per_byte(priority)` passed to `rpc.fee_rate(...)` in `send_inner`
and checked against the raw tiers in `monero_fee_estimate`. What was left was
the escrow, which is the larger pot.

- `mobile/src/monero.rs` — `fee_ceiling_per_byte` and `fmt_xmr` are now
  `pub(crate)`; no behaviour change.
- `mobile/src/ceremony.rs:frost_propose_split` — `rpc.fee_rate(FeePriority::Normal,
  u64::MAX)` became `rpc.fee_rate(FeePriority::Normal,
  crate::monero::fee_ceiling_per_byte(1))`. `u64::MAX` switches the crate's own
  sanity ceiling off; a lying node (or plain http on the way to it) named the
  fee and the residual claimant paid it.

**Why 50×.** Fifty times the *normal* tier is 4 000 000 pXMR/byte — exactly the
honest **fastest** tier, so no tier the network actually quotes is refused, and
anything above it is a node inventing a number. Far below "the whole note".

### (b) Refuse a built fee over the quote, on both clients

The rule (`fee_acceptable`, already in `monero.rs`, unchanged): a built fee is
refused before signing when it exceeds the quote by more than a quarter (plus
one quantisation step), **or** exceeds a twentieth of the amount — the second
only once the fee is over `FEE_SHARE_FLOOR_PXMR` (0.01 XMR), because the
fastest tier honestly costs more than 5 % of a coffee. What changed is that the
clients now supply the quote:

- `app/src/wallet.rs:send_xmr` — `monero_send` → `monero_send_checked(…,
  plan.fee_pxmr)`.
- `applications/android/…/Wallet2.kt:send` — `moneroSend` → `moneroSendChecked(…,
  plan.feePxmr)`.

`plan.fee_pxmr` is the same estimate `quote()` / the confirm dialog renders, so
the number on the screen is the number the build is held to. Zero means the
estimate failed (both screens say so) and only the absolute ceilings hold.
Every refusal is raised under the `"fee rate:"` prefix, which both clients'
`never_left` / `builtNothing` lists already read as "nothing was built, the
notes are free again" — a refused send costs a retry, not notes pinned for half
an hour.

- `applications/android/…/ui/Pay.kt` — the confirm dialog now says the quote is
  a ceiling (`pay_fee_capped`), so "nothing was sent" is not a surprise.

### (c)(d) The escrow's own co-signers

`mobile/src/ceremony.rs` — new pure `release_acceptable(inputs_total, funded,
fee, fee_per_byte, local_estimate)`, called from `frost_cosign` **before the
signing machine is touched**. Three refusals:

1. `funded == 0` — this device has not scanned the escrow. *Unknown refuses*;
   unknown and empty are different answers.
2. `inputs_total < funded` — a partial sweep (this is also 1.4(b)).
3. `fee_per_byte > fee_ceiling_per_byte(1)`, or
   `fee > min(FEE_CEILING, max(FEE_RESERVE, 2 × local_estimate))`.

**Constants.** `FEE_RESERVE = 200_000_000` pXMR (0.0002 XMR) is the existing
reserve every DUCAT release is sized against, so it is the floor. `2 ×` a fresh
local estimate raises it for an escrow holding several notes, where the honest
fee is larger than the reserve. `FEE_CEILING = FEE_RESERVE * 20` (0.004 XMR)
caps what any estimate can raise it to: a sixteen-note release at the normal
tier weighs ~11 kB and costs well under a thousandth of an XMR, and a local
estimate is still a number from *a node*, however much that node is ours.

`frost_cosign` gained two **scalar** parameters (`funded_pxmr`,
`local_estimate_pxmr`) rather than a record — scalars are not the by-value
`RustBuffer` args of the arm64 crash, and the call stays at seven arguments,
inside the register set.

Kotlin approve path (`Ceremony.approveRideRelease`) supplies both: `fundedPxmr`
from the record, refreshed with `checkRideFunding` when the record has never
been scanned (an arbiter never scans, so this is its ordinary path — `scanFrom`
is written at DKG finish, so the scan is bounded); the estimate from
`Wallet.feeFor(context, release.inputs)`, which prices the real shape because
the input count now comes out of the payload.

**Tests** (`mobile/src/ceremony.rs`, `mod consent_tests`): an honest sweep
signs; a half-swept escrow is refused; one piconero short is refused; an
unscanned escrow refuses rather than skips; a fee a piconero over the reserve
is refused; a rate 51× normal is refused while the honest fastest tier passes;
a local estimate doubles the bar and no more; the extremes refuse instead of
overflowing. Plus the pre-existing `a_fee_is_held_to_the_quote_and_to_the_amount`.

---

## 1.4 — consent from the transaction, not the balance (M2, M4, M5, M11)

### (a) The parsed view carries the inputs and the fee

`mobile/src/ceremony.rs`:

- `read_destinations` → `read_tx`, returning `ParsedTx { destinations,
  inputs_total_pxmr, inputs }`. The inputs were already walked to reach the
  payments; now their commitments are summed (checked add). `SignableTransaction::read`
  runs the crate's `validate` first, so inputs ≥ payments + fee is already
  guaranteed — what it does *not* guarantee is that they are **all** the escrow
  holds.
- `frost_destinations` → **`frost_view`**, returning `TxView { destinations,
  inputs_total_pxmr, inputs, fee_pxmr, fee_per_byte }`. `FrostCosign` also
  gained `inputs_total_pxmr` / `inputs`.

### (b) The co-signer sizes its residual from the transaction

The residual output carries **no amount on the wire** — it takes
`inputs − fixed − fee` — and the old `releaseToMe` substituted the escrow's
scanned balance for `inputs`. Those agree only when the release sweeps the
whole escrow, and nothing made it: a proposer spending one of two notes paid
every fixed slice in full and halved the residual, which is the co-signer's own
stake, while every figure on the screen still added up.

`applications/android/…/Ceremony.kt`:

- `residualOf(inputsTotal, fixed, fee)` — pure, `null` when the terms do not
  close (a disagreement with the crate is a refusal, never a zero).
- `sizeRelease(dests, mine, payerAddr, theirs, inputsTotal, fee)` — pure: the
  shape refusals, the residual arithmetic, and the attribution of each output.
- `readRelease(context, o, payload)` — the thin impure shell: `frostView`, then
  `sizeRelease`, returning `Release { outs, inputsTotalPxmr, inputs, feePxmr,
  toMePxmr, payerBackPxmr, digest }`.
- `releaseToMe` is now `readRelease(...).toMePxmr`; the old
  `(dests, mine, funded)` overload is gone.
- The bridge refusal (inputs ≥ funded) lives in `frost_cosign`, where it costs
  nothing and is enforced for every client.

**Refused shapes** (no DUCAT release builds them, and a screen fed a
description it cannot stand behind is worse than no screen): more than two
outputs; an output with no address (change named by a view pair); the same
address paid twice; arithmetic that does not close.

### (c) The consent screen lists the outputs

`applications/android/…/ui/Chat.kt`, `ui/Ceremony.kt`, and the desk's
`Arbiter.kt`:

- At park time (`onFrostRound` round 0) the parsed outputs are written to the
  record as `pendingOuts` (address, sized amount, residual flag, side) with
  `pendingDigest`. The screen reads that — no bridge call per recomposition,
  and the figures cannot come from the note beside the payload.
- New `ParsedOutputs` composable: one line per output with the address in full,
  what it receives, whose this device believes it is, and the residual marked
  (it is the one that pays the fee). Shown on the banner and on the
  full-screen `EscrowStep`.
- `riderBack` / `toDriver` / the step's amount and note now come from the parse,
  with the old claim-based figures kept only as the fallback for a proposal
  parked by an older build.
- `approveRideRelease` refuses when the proposer's claim (`pendingRiderBack`)
  and the parsed payer-side total differ by more than `FEE_RESERVE_PXMR` — the
  fee is the only honest difference, because whichever side takes the residual
  pays it.
- The desk arbiter console prints the inputs total, the note count, the fee,
  every output with its attribution, the parsed payer-side figure and the
  proposal's fingerprint; it reprints when a counter-offer supersedes, and an
  approval is refused unless a readable listing was printed for that exact
  proposal.

**Attribution, and its honest limit.** Four outcomes: `MINE` (an address this
wallet controls), `PAYER` (the refund address named in the round-0 frame that
every party echoed and stored — the one term nobody has to take on trust),
`THEIRS` (the address that party published to this device), `UNKNOWN`.

`UNKNOWN` is **shown in the warning colour and not refused**, deliberately. A
subaddress is published per counterparty (§15.10), so the address a seller
published to its buyer is not the one it published to the arbiter, and neither
need be the one it proposes to pay itself at. Refusing every unplaceable output
would refuse honest releases. Two changes narrow the gap: a payee proposing now
pays itself at `addressFor(<counterparty>)` rather than its primary
(`proposeRideSplit`, `releaseBond`) — which on a ride is exactly the address the
handshake published to the other side, so it reads as `THEIRS` — and it is
better §15.10 hygiene besides. See "left" below for the rest.

### (d) The joiner checks the frame's terms

`Ceremony.onDkgRound`, in the invitation branch (M5). Every economic term is the
inviter's word for it and the ceremony id binds only the roster and the nonce:

- **`funderIdx == me` → refuse.** `start` always names *itself* the funder, so
  there is no such thing as being invited to fund. (This replaces the previous
  mitigation of minting our own refund address, which is now unreachable; the
  frame's `refundAddr` is adopted verbatim, which is what lets every party
  check an output against it later.)
- **Ride fare must equal the accepted offer** — the newest incoming kind-7
  accept in this thread, which carries the offer's amount. Mismatch or absence
  *drops* the invite rather than refusing it for ever: `nudge` retransmits round 0,
  by which time an accept still in flight has landed. The arbiter is exempt (it
  has no offer or accept with either principal).
- **Host stake ≤ `Stakes.stakeFor(Ride, fare)`**, else refuse. Both clients
  compute the schedule from the fare, so a larger ask is something nobody
  offered; joining is automatic and silent, so the ceiling has to be here.

### (e) The consent tap carries what the screen showed

`approveRideRelease(context, idHex, shownToMePxmr, shownDigest)`:

- `shownDigest` (SHA-256 of the payload the screen was drawn from) must match
  the payload now on the record, or `ReleaseSuperseded` — M11's TOCTOU, which a
  counter-offer landing on the poller thread produces routinely. Passed by the
  chat banner, the bond section in `ui/Profile.kt`, and the desk arbiter.
- `shownToMePxmr` is the parse-derived `pendingToMe`; a payload paying this
  device less than that by more than `FEE_RESERVE_PXMR` throws
  `ReleaseMisstated`. (The reserve tolerance exists for one case: a proposal
  parked by a build that sized the residual from the balance carries a figure
  one network fee larger. Refusing that would strand a settlement nobody
  disputes, and the reserve is the bound on the fee itself.)

**Errors reach people in their own language.** New typed
`Ceremony.ReleaseRefused(bodyRes, args…)`, `ReleaseSuperseded`, and the
previously unrendered `ReleaseMisstated` are all rendered by
`ui/ClaimErrors.kt:moneyFailure`.

### Tests

- Rust, `mod consent_tests` — above.
- Rust, `mod destination_tests` — unchanged and still green through the
  `read_tx` refactor.
- Kotlin unit test, `applications/android/src/test/…/ReleaseArithmeticTest.kt`
  — `residualOf`: the ordinary split, a sweep, **a partial sweep sized smaller
  than a full one**, terms that do not close (including the overflow an
  unchecked sum would have wrapped through), and an exactly-spent escrow.
- Desk exercise, `applications/desktop/…/ReleaseReadTest.kt` rewritten against
  `sizeRelease`: fixed slice exact, the stolen split, a near-miss address, the
  residual sized from the inputs, **the partial sweep**, an overstated residual,
  a nameless output refused, a three-way payout refused, arithmetic that does
  not close refused, both of this device's addresses recognised, and the
  `PAYER` / `THEIRS` / `UNKNOWN` attributions.

---

## Strings

Twelve new keys in `strings_bond.xml` (`bond_outputs_title`,
`bond_output_{mine,payer,theirs,unchecked,residual}`,
`bond_refuse_{shape,nameless,arithmetic,claim}`, `bond_superseded`,
`bond_misstated`) and one in `strings_pay.xml` (`pay_fee_capped`), each with a
translation in all nineteen locale folders. `check_strings.py` and the desk's
`--check` are both clean.

---

## Files touched

| File | What |
|---|---|
| `mobile/src/monero.rs` | `fee_ceiling_per_byte`, `fmt_xmr` → `pub(crate)` |
| `mobile/src/ceremony.rs` | `read_tx`/`ParsedTx`, `TxView` + `frost_view`, `view_of`, ceiling in `frost_propose_split`, `FEE_CEILING`, `release_acceptable`, `frost_cosign` params + checks, `consent_tests` |
| `app/src/wallet.rs` | `monero_send_checked` with the quote |
| `…/ducat/Wallet2.kt` | `moneroSendChecked` with the quote |
| `…/ducat/Ceremony.kt` | join-time term checks; `Side`/`Payout`/`Release`; `digestOf`, `residualOf`, `sizeRelease`, `readRelease`, `outsJson`/`parkedOuts`; `arbitersPayeeAddress`; parked outputs + digest in `onFrostRound`; `approveRideRelease` signature and checks; per-contact payout address in `proposeRideSplit` and `releaseBond` |
| `…/ducat/ui/Chat.kt` | figures from the parse; `ParsedOutputs`; the tap carries figure + digest |
| `…/ducat/ui/Ceremony.kt` | `EscrowStep(outputs = …)` |
| `…/ducat/ui/ClaimErrors.kt` | render the three consent refusals |
| `…/ducat/ui/Pay.kt` | the confirm step says the quote is a ceiling |
| `…/ducat/ui/Profile.kt` | the bond release tap carries the digest |
| `…/desk/Arbiter.kt` | the ruling console prints the whole parsed transaction; approval bound to the listing printed |
| `…/desk/ReleaseReadTest.kt` | rewritten against `sizeRelease` |
| `…/android/src/test/…/ReleaseArithmeticTest.kt` | new |
| `res/values*/strings_bond.xml`, `strings_pay.xml` | 13 keys × 20 locales |
| `uniffi/ducat_mobile/ducat_mobile.kt` | regenerated |

`Stakes.kt` was read and needed no change: `stakeFor(Ride, fare)` is what the
join check holds the frame to.

---

## Left

- **An unplaceable payout address is warned about, not refused.** Closing it
  properly needs either the payee's payout address carried in the round-0 frame
  beside the funder's refund address (a wire change, so a spec change), or the
  contact record remembering which subaddress *we* published to each contact
  (`Mailbox.kt` / `ContactStore.kt`, both outside this pass). Until then, a
  release to an address neither principal can place is shown in full, in the
  warning colour, and signed only by a person who chose to.
- **A larger-than-schedule host stake is refused outright**, not offered with a
  confirmation. Two people could legitimately agree to one; it needs a screen
  showing the stake beside the fare before the commitment, and joining is
  currently automatic on the poller. No honest invite the app produces is
  affected — `startRide` always sends exactly `Stakes.stakeFor(Ride, fare)`.
- **At most two outputs** matches everything the clients build today. A MAD
  escrow returning two deposits beside a payment (mentioned in
  `frost_propose_split`'s own doc) would be a third output and would be
  refused; the bound must be raised deliberately, with the consent screen
  checked against the new shape, if such a release is ever built.
- **The desk's own send confirm** (Svelte) does not yet say the quote is a
  ceiling; the desk *enforces* it (`app/src/wallet.rs`). Only the phone's
  `ui/Pay.kt` was in scope.
- **Not run**: a stagenet send and a bonded ride on the emulators — the two
  field proofs §8 names for 1.3 and 1.4.
