# Phase 1, steps 1.1 and 1.2: the kiosk sighting and the phone's second opinion

*2026-09-14. What was changed for TRUST.md §8's steps 1.1 and 1.2 — the
adversarial review's M1, M3/N2 (phone side), M10 and N22.*

Both fixes come from the same sentence in §15.11: goods MUST NOT be released
on sight alone without a bond. The kiosk was doing exactly that, and the
phone's second opinion — the check that is supposed to stand between one
node's claim and a merchant handing over the goods — settled on silence and
counted a mempool sighting as corroboration.

---

## 1.2 — the phone's second opinion, rewritten on the desk's rule (M3/N2, N22)

`app/src/opinion.rs` is the reference; the phone now says the same thing.

### Files

| File | What changed |
|---|---|
| `applications/android/src/main/java/org/ducatproject/ducat/SecondOpinion.kt` | rewritten |
| `applications/android/src/test/java/org/ducatproject/ducat/SecondOpinionRuleTest.kt` | new — 9 tests on a walked clock |
| `applications/desktop/src/main/kotlin/org/ducatproject/desk/SecondOpinionTest.kt` | rewritten for the new rule (`:desktop:secondopinion`) |
| `.../ducat/Tabs.kt` | the receipt waits for the confirmations the amount needs |
| `.../ducat/Donations.kt` | the amount is passed to `settles` |
| `.../ducat/ui/Pos.kt`, `ui/BarTab.kt` | "N of M blocks", and *settle anyway* when the opinion stalls |
| `.../ducat/ui/Items.kt` | the operator's small-sale floor |
| `res/values*/strings_{pos,items,notify}.xml` | new and changed wording, 20 languages |

### The rule

`onTx` is now built on the bridge's `moneroTxStatus` (which hashes the
transaction the node returns, so a node answering with *some* transaction has
not answered the question) and `moneroSecondOpinionNodes` (never the node in
use, never the operator's own node, https first, one other node when an own
node is configured and two when not).

- **`InBlock` elsewhere settles**, and only that is cached.
- **`InPool`, `Unknown` and `Unreachable` defer.** The tab stays billed, the
  next poll asks again, `asked_` throttles the ask to once a minute.
- **Ten minutes of deferral** tells the operator once (the `said_` path) and
  the tab, the sale and the order offer **Settle anyway**
  (`SecondOpinion.settleAnyway`, recorded as `forced_`, logged as the
  operator's word rather than as corroboration).
- **The small-sale floor** (operator setting, default 0 = never) is the only
  place the old behaviour survives: at or under it, one block settles and
  silence settles.
- **`confirmationsNeeded(amount)`** = 1 under the floor, 3 up to one XMR, 10
  above, and a receipt now waits for that many blocks; the screens say
  "settling — N of M blocks" meanwhile.

Two notification pairs now exist where there was one: `notify_unconfirmed_*`
("another node has no record of it") for a node that answered, and the new
`notify_uncorroborated_*` ("no second node could be reached") for silence.
They are different facts and the old single wording was wrong for half of
them.

### Decisions

- **The rule is a pure Kotlin object.** `SecondOpinion.Rule` (`Memo`, `Move`,
  `move`, `due`, `stalled`, `confirmationsNeeded`, `confirmationsOf`) touches
  no Android class, so `SecondOpinionRuleTest` drives it on a clock it walks
  itself — opinion.rs's tests, asked in Kotlin. `SecondOpinion` is the wiring:
  prefs in, notification out.
- **`settles(context, txid, amountPxmr = 0)`.** The default keeps
  `Ceremony.kt`'s four call sites compiling untouched; amount 0 is *unknown*,
  which is treated as above any floor — the ceremony's escrow releases pass no
  amount and are the largest sums in the app.
- **`holdsEscrow` still settles on silence, deliberately.** It has no "in a
  block" answer to wait for — the escrow is found by scanning its own address
  — so deferring on an unreachable node is a permanent stall on a ceremony
  with a countdown, not a two-minute wait for a block. The reason is written
  at the function. This is the phone-only path; `opinion.rs` has no
  counterpart.
- **Settling on the floor is not cached.** That was our own node's word, not a
  fact; the next sale asks again.
- **`Elapsed.due` everywhere**, never a bare subtraction — a stamp ahead of
  now must not wedge the only road to *settled*.

---

## 1.1 — the kiosk hands over on a sighting (M1)

### Files

| File | What changed |
|---|---|
| `.../ducat/ui/Kiosk.kt` | `Seen` renders as settling; paid panel and Ready need `Confirmed` (or the cap); staff row shows the blocks and *settle anyway* |
| `.../ducat/Orders.kt` | the sighting cap, `handsOverOnSight`, `settlingBlocks`, `settlingTx` |
| `.../ducat/ui/Items.kt` | the cap is typed on the kiosk's staff Items tab |
| `app/src/orders.rs` | `kiosk_sight_cap_pxmr`, `hands_over_on_sight`, `order_settling` |
| `applications/desk/src-tauri/src/lib.rs` | `counter_risk` / `set_counter_risk`; `OrderRow` carries `blocks`, `blocks_needed` |
| `applications/desk/src/lib/api.ts`, `lib/Kiosk.svelte` | the settling render and the two settings |
| `res/values*/strings_kiosk.xml` | new and changed wording, 20 languages |

### What the screens do now

- **Phone.** `BilledPanel` and `PayPanelMonero` send a `Seen` order to a new
  `SettlingPanel` — spinner, the amount, "N of M blocks", the note, and no
  thank-you. `PaidPanel` is reached only from `Confirmed`, or from `Seen` when
  the order is at or under the operator's cap. The staff list shows the block
  count under a settling order and offers **Ready** only when the goods may
  actually go (`StaffRow.handOver`).
- **Desk.** `stateWord` renders `Seen` as "settling — N of M blocks" instead
  of "paid — seen, not yet on the chain"; the sale card grows a spinner row
  with the same line; the Ready button moved from `state === "Confirmed"` to
  `released(o)`, which is `Confirmed` or a `Seen` under the cap.
- **The setting**, both clients: *Hand over on first sight*, an amount in XMR,
  default empty (= 0 = off), with the risk beside it in one sentence — "a
  payment seen but not yet in a block can still be replaced; up to this amount
  the kiosk trusts the sighting."

### Decisions

- **The cap is per order, not per counter session**: a counter that trusts a
  sighting for a coffee does not thereby trust one for the espresso machine.
- **Storage.** Phone: with the tax rate in the plain `ducat_business` prefs —
  business configuration the operator sets once, not per-order state, and not
  a secret. Desk: a key under the orders store, as briefed. The small-sale
  floor sits with the opinion's own notes on both clients, mirroring
  `opinion.rs`.
- **`Confirmed` now means deep enough**, on both clients: `reconcile_orders` /
  `Orders.reconcile` promote a sighted order only when the transaction has the
  confirmations its size needs *and* a second node has it in a block. Without
  that the "N of M blocks" line would be describing a threshold nothing
  enforced.
- **The kiosk's staff Items tab took the setting** (`ItemsScreen(kiosk = true)`)
  rather than a new screen: it is the only staff surface a kiosk has, and the
  tax rate already lives there. The till and the bar do not show the sighting
  cap — it is a kiosk rule — but they do show the small-sale floor, which is
  theirs too.

---

## M10 — a pool-sighted order promoted without the second opinion

`Orders.kt`'s `Seen → Confirmed` branch promoted on the txid landing in our
own wallet scan — our own node's word, twice — while the never-sighted branch
below it asked a second node. `app/src/orders.rs` had the same shape. Both now
take the second opinion (and the confirmations) on that path. A forged block is
exactly as convincing to a sighted order as to an unsighted one.

---

## Gates

| Gate | Result |
|---|---|
| `cargo test -p ducat-app` | 67 passed, 0 failed (one new test: `a_sighting_releases_nothing_until_the_operator_prices_it`) |
| `:android:assembleDebug` | BUILD SUCCESSFUL |
| `:android:testDebugUnitTest --tests '*SecondOpinionRuleTest*'` | 9 tests, 0 failures |
| `:desktop:secondopinion` | `2NDTEST_OK block=settles silence=defers pool=defers alarm=once forced=ok floor=small-only escrow=per-amount` |
| `python3 applications/check_strings.py` | 47 files × 19 languages agree |
| `strings_from_android.py` + `--check` | 694 keys across 20 locales, up to date |
| `pnpm check` | 143 files, 0 errors, 0 warnings |
| `cargo build --manifest-path src-tauri/Cargo.toml` | Finished |
| `:desktop:compileKotlin` | **green on a clean HEAD with these changes** (verified in a scratch worktree); red in the working tree on 39 errors in `desktop/.../ReleaseReadTest.kt`, which calls `Ceremony.releaseToMe(list, set, long)` against the three-argument `(context, JSONObject, ByteArray)` form another step-1.4 change introduced. None of the errors are in the files touched here. |

## Left

- **The desk's tab receipts still go out at one block.** The phone's
  `Tabs.kt` now waits for `confirmationsNeeded`; `app/src/tabs.rs`'s
  `reconcile_tabs` was out of the stated scope for this step, so a desk
  running a bar tab still receipts on the first block (after the second
  opinion, which it already took). One line beside the existing
  `self.settles(...)` closes it, and it should be closed — two clients
  settling differently is the shape this whole check exists to prevent.
- **Settle anyway is offered where the transaction is known** — a sighted tab,
  sale or order. A tab whose payment was never sighted defers on a txid the
  screens never learn, so the operator sees no button there; the existing
  *paid outside* remains their way through. Recording the deferred txid on the
  tab would close this.
- **The desk has no *settle anyway* button yet.** `App::settle_anyway` exists
  and is unwired; only the phone's screens offer it.
- The kiosk walk on two phones (a sighting shows settling, a block shows paid,
  the cap works) has not been run — no emulators were started for this work.
