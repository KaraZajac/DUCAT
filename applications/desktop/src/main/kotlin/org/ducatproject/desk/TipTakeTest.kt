package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.RunningTab

/**
 * The takings are what came in, not what was asked for.
 * `./gradlew :desktop:tiptake`.
 *
 * Reconciliation always knew the tip — it subtracts the bill from the output,
 * puts "Tip — thank you" on the receipt and names the figure in the
 * notification — and then dropped it on the floor. The tab recorded only
 * `settledTotal`, the amount billed, so the sales screen summed bills: a
 * counter where tipping is a real part of the margin read its own till short
 * by every tip it took. Found live on 2026-08-24 — USD 1.00 tipped on a
 * coffee, and "Today" said USD 8.03.
 *
 * `settledTotal` still means what was billed, because that is what the
 * customer holds on paper and what matching compares against; what arrived is
 * kept beside it rather than over it.
 */
fun main() {
    fun tab(settled: Long, paid: Long, state: String = "paid") = RunningTab(
        id = "t1", origin = "pos", personaHex = "aa".repeat(32),
        openedAt = 0, lines = listOf(BillItem("Flat white", settled)),
        taxPxmr = null, state = state, settledTotal = settled, paidPxmr = paid,
    )

    // A tipped sale counts for what was paid, and names the tip.
    val tipped = tab(settled = 12_419_000_000, paid = 14_113_000_000)
    check(tipped.takePxmr == 14_113_000_000L) { "TIP_FAIL takings: ${tipped.takePxmr}" }
    check(tipped.tipPxmr == 1_694_000_000L) { "TIP_FAIL tip: ${tipped.tipPxmr}" }
    // And what was billed is untouched — matching and the paper both use it.
    check(tipped.totalPxmr == 12_419_000_000L) { "TIP_FAIL the bill moved" }

    // No tip: takings are the bill, and nothing is claimed.
    val flat = tab(settled = 12_419_000_000, paid = 12_419_000_000)
    check(flat.takePxmr == 12_419_000_000L) { "TIP_FAIL flat takings" }
    check(flat.tipPxmr == 0L) { "TIP_FAIL invented a tip" }

    // A tab written before the field existed reads back as zero, and must
    // still count for its bill rather than for nothing.
    val old = tab(settled = 12_419_000_000, paid = 0)
    check(old.takePxmr == 12_419_000_000L) { "TIP_FAIL an old sale vanished from the takings" }
    check(old.tipPxmr == 0L) { "TIP_FAIL old tab claimed a tip" }

    // Underpayment cannot show as a negative tip.
    check(tab(settled = 100, paid = 60).tipPxmr == 0L) { "TIP_FAIL negative tip" }

    // An open tab is still the sum of its running lines, not a settled total.
    val open = tab(settled = 12_419_000_000, paid = 0, state = "open")
        .copy(settledTotal = 0)
    check(open.totalPxmr == 12_419_000_000L) { "TIP_FAIL open tab total" }

    // The field has to survive the disk, which is where a new one usually
    // does not: the sales screen reads tabs back through JSON every time.
    val round = RunningTab.from(tipped.toJson())
    check(round.paidPxmr == 14_113_000_000L) { "TIP_FAIL paid total lost on the round trip" }
    check(round.settledTotal == 12_419_000_000L) { "TIP_FAIL billed total lost" }
    check(round.tipPxmr == 1_694_000_000L) { "TIP_FAIL tip lost on the round trip" }

    // And a tab from before the field is still readable.
    val legacy = tipped.toJson().apply { remove("paid_total") }
    check(RunningTab.from(legacy).paidPxmr == 0L) { "TIP_FAIL legacy tab" }
    check(RunningTab.from(legacy).takePxmr == 12_419_000_000L) { "TIP_FAIL legacy takings" }

    println("TIP_OK the till counts what it was actually paid")
}
