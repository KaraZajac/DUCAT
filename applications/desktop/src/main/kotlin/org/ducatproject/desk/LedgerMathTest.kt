package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.RunningTab

/** The summary arithmetic, pinned without a wallet: pending and
 *  out-of-period rows must not count, tax rides money in, fees ride
 *  money out, and the business lens buckets by door and state. */
fun main() {
    fun ev(
        ts: Long, dir: Ledger.Direction, amt: Long,
        fee: Long = 0, net: Long = 0, tax: Long? = null,
        donation: Boolean = false, pending: Boolean = false,
    ) = Ledger.Event(
        txid = "t$ts", height = 1, timestamp = ts, direction = dir,
        amountPxmr = amt, feePxmr = fee, netPxmr = net,
        balanceAfterPxmr = 0, counterparty = null, address = null,
        donation = donation, source = Ledger.Source.Unknown, note = null,
        ours = emptyList(), consumed = emptyList(), chain = null,
        pending = pending, locked = false, unlocksInBlocks = 0,
        taxPxmr = tax,
    )
    val events = listOf(
        ev(100, Ledger.Direction.Received, 1000, net = 1000, tax = 80),
        ev(200, Ledger.Direction.Sent, 400, fee = 7, net = -407),
        ev(300, Ledger.Direction.Sent, 200, fee = 3, net = -203, donation = true),
        ev(400, Ledger.Direction.Received, 500, net = 500, pending = true), // pending: out
        ev(50, Ledger.Direction.Received, 9999, net = 9999),                // before window
    )
    val s = Ledger.summarize(events, 100, 1000)
    check(s.inPxmr == 1000L) { "in ${s.inPxmr}" }
    check(s.outPxmr == 600L) { "out ${s.outPxmr}" }
    check(s.feesPxmr == 10L) { "fees ${s.feesPxmr}" }
    check(s.netPxmr == 390L) { "net ${s.netPxmr}" }
    check(s.inCount == 1 && s.outCount == 2) { "counts ${s.inCount}/${s.outCount}" }
    check(s.taxCollectedPxmr == 80L) { "tax ${s.taxCollectedPxmr}" }
    check(s.donationsPxmr == 200L) { "don ${s.donationsPxmr}" }

    fun tab(
        origin: String, state: String, settledAtMs: Long,
        total: Long, paid: Long = 0, tax: Long? = null, billSeq: Long = -1,
    ) = RunningTab(
        id = "x$settledAtMs$origin$state", origin = origin, personaHex = "p",
        openedAt = settledAtMs, state = state, settledTotal = total,
        settledAt = settledAtMs, taxPxmr = tax,
        lines = listOf(BillItem("thing", total)), paidPxmr = paid, billSeq = billSeq,
    )
    val tabs = listOf(
        tab("pos", "settled", 200_000, 300, tax = 24),
        tab("pos", "paid", 300_000, 100, paid = 120),         // tip 20
        tab("taxi", "settled", 400_000, 250),
        tab("bar", "open", 500_000, 999, billSeq = 4),        // billed, outstanding
        tab("bar", "open", 500_000, 111),                     // not billed: not counted
        tab("pos", "settled", 50_000, 777),                   // before window
        tab("donate", "cancelled", 300_000, 500),             // cancelled: out
    )
    val b = Ledger.summarizeBusiness(tabs, 100, 1000)
    check(b.salesCount == 3) { "sales ${b.salesCount}" }
    check(b.salesPxmr == 300L + 120L + 250L) { "take ${b.salesPxmr}" }
    check(b.byOrigin["pos"]!!.tipPxmr == 20L) { "tip" }
    check(b.taxCollectedPxmr == 24L) { "biz tax ${b.taxCollectedPxmr}" }
    check(b.outstandingCount == 1 && b.outstandingPxmr == 999L) { "outstanding" }
    println("LEDGER_MATH_OK")
}
