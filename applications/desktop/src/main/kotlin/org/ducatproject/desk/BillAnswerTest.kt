package org.ducatproject.desk

import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.ui.billAnswers
import org.ducatproject.ducat.ui.reactionsOn

/**
 * A sequence number is unique in a mailbox, not in a conversation.
 * `./gradlew :desktop:billanswer`.
 *
 * A kind-5 names what it answers by sequence number alone, and the chat took
 * that as an exact address. It is exact only while a thread has one mailbox,
 * and threads do not: every card cut for a hail, a sale or a listing restarts
 * the numbering, so one conversation holds several messages numbered 0.
 *
 * Found live on 2026-08-24. A rider declined a ride offer that had arrived at
 * seq 0; ten minutes later the same person bought a coffee and a croissant
 * from the same trader, and the till's bill arrived on a fresh card — also at
 * seq 0. The chat matched the old refusal to the new bill and stamped it
 * "Declined", which took the Pay button with it. The customer was holding an
 * unpayable bill they had never refused while the till waited for a payment
 * that no screen could now send.
 *
 * With only a seq on the wire the honest reading is positional: a reaction
 * answers the message with that seq which most recently preceded it.
 */
private fun msg(
    kind: Int,
    seq: Long,
    ts: Long,
    outgoing: Boolean,
    reSeq: Long? = null,
    reOwn: Boolean = false,
    pxmr: Long = 0,
) = StoredMessage(
    outgoing = outgoing, seq = seq, body = "", timestamp = ts,
    kind = kind, amountPxmr = pxmr, reSeq = reSeq, reOwn = reOwn,
)

fun main() {
    // ---- the live failure ----
    // An inbound ride offer at seq 0, refused; then an inbound bill, also at
    // seq 0 because the sale cut its own card.
    val rideOffer = msg(kind = 6, seq = 0, ts = 100, outgoing = false)
    val refuseRide = msg(kind = 5, seq = 7, ts = 110, outgoing = true, reSeq = 0)
    val bill = msg(kind = 1, seq = 0, ts = 200, outgoing = false, pxmr = 18_124_000_000)

    val a = billAnswers(listOf(rideOffer, refuseRide, bill))
    check(a.refused.isEmpty()) {
        "BILL_FAIL a refused ride offer marked a later bill declined"
    }
    check(a.withdrawn.isEmpty()) { "BILL_FAIL nothing was withdrawn" }

    // ---- and a real refusal still lands ----
    val refuseBill = msg(kind = 5, seq = 8, ts = 210, outgoing = true, reSeq = 0)
    val b = billAnswers(listOf(rideOffer, refuseRide, bill, refuseBill))
    check(b.refused == setOf(0L to 200L)) { "BILL_FAIL a genuine refusal was lost: ${b.refused}" }

    // The refusal answers the bill, which is the nearest seq-0 before it —
    // not the ride offer a hundred ticks earlier.
    check(0L to 100L !in b.refused) { "BILL_FAIL resolved past the nearer message" }

    // ---- the sender's own retraction ----
    // Our bill, our log, so reOwn is set and the sides match.
    val myBill = msg(kind = 1, seq = 3, ts = 300, outgoing = true, pxmr = 500)
    val retract = msg(kind = 5, seq = 4, ts = 310, outgoing = true, reSeq = 3, reOwn = true)
    val c = billAnswers(listOf(myBill, retract))
    check(c.withdrawn == setOf(3L to 300L)) { "BILL_FAIL retraction: ${c.withdrawn}" }
    check(c.refused.isEmpty()) { "BILL_FAIL a retraction read as a refusal" }

    // A retraction must not reach across to the other side's log.
    val theirBill = msg(kind = 1, seq = 3, ts = 300, outgoing = false, pxmr = 500)
    check(billAnswers(listOf(theirBill, retract)).withdrawn.isEmpty()) {
        "BILL_FAIL retraction crossed logs"
    }

    // ---- two bills at the same seq, one refused ----
    // The second sale's bill is refused; the first one, already paid and long
    // past, must keep its own state.
    val firstBill = msg(kind = 1, seq = 0, ts = 400, outgoing = false, pxmr = 100)
    val secondBill = msg(kind = 1, seq = 0, ts = 500, outgoing = false, pxmr = 900)
    val no = msg(kind = 5, seq = 9, ts = 510, outgoing = true, reSeq = 0)
    val d = billAnswers(listOf(firstBill, secondBill, no))
    check(d.refused == setOf(0L to 500L)) { "BILL_FAIL wrong bill of two: ${d.refused}" }

    // An answer "before" the only candidate resolves to it when the gap is
    // clock-skew-sized — you can only refuse what arrived, and the two
    // stamps come from two different phones (the 08-27 live finding).
    val early = msg(kind = 5, seq = 9, ts = 50, outgoing = true, reSeq = 0)
    check(billAnswers(listOf(early, firstBill)).refused == setOf(0L to 400L)) {
        "BILL_FAIL a skew-sized gap did not resolve"
    }

    // An emoji reaction (kind 4) is not a refusal.
    val emoji = msg(kind = 4, seq = 9, ts = 410, outgoing = true, reSeq = 0)
    check(billAnswers(listOf(firstBill, emoji)).refused.isEmpty()) { "BILL_FAIL kind 4" }

    // ---- and an emoji lands on the message it was left on ----
    // Same address, same collision: a thumbs-up on one card's message must not
    // decorate a different message that a later card numbered the same.
    val theirMsg = msg(kind = 0, seq = 0, ts = 100, outgoing = false)
    val thumbs = StoredMessage(
        outgoing = true, seq = 5, body = "\uD83D\uDC4D", timestamp = 110,
        kind = 4, reSeq = 0, reOwn = false,
    )
    val laterBill = msg(kind = 1, seq = 0, ts = 300, outgoing = false, pxmr = 900)
    val on = reactionsOn(listOf(theirMsg, thumbs, laterBill))
    check(on[0L to 100L]?.first == "\uD83D\uDC4D") { "REACT_FAIL lost its own message" }
    check(on[0L to 300L] == null) { "REACT_FAIL agreed to a bill nobody had reacted to" }

    // Their reaction to our message goes in the other slot.
    val myMsg = msg(kind = 0, seq = 2, ts = 400, outgoing = true)
    val theirs = StoredMessage(
        outgoing = false, seq = 6, body = "\u2764\uFE0F", timestamp = 410,
        kind = 4, reSeq = 2, reOwn = false,
    )
    check(reactionsOn(listOf(myMsg, theirs))[2L to 400L]?.second == "\u2764\uFE0F") {
        "REACT_FAIL side"
    }

    // Changing your mind: the later one wins.
    val first4 = StoredMessage(
        outgoing = true, seq = 5, body = "a", timestamp = 110, kind = 4, reSeq = 0, reOwn = false,
    )
    val second4 = StoredMessage(
        outgoing = true, seq = 7, body = "b", timestamp = 150, kind = 4, reSeq = 0, reOwn = false,
    )
    check(reactionsOn(listOf(theirMsg, first4, second4))[0L to 100L]?.first == "b") {
        "REACT_FAIL latest per side"
    }

    // ---- two clocks, honestly skewed (found live 2026-08-27) ----
    // The bill's stamp comes from the asker's clock, the refusal's from the
    // payer's, and phones disagree by minutes. A bill minted by a fast
    // clock and declined straight away sits "after" its own refusal; the
    // positional rule must still resolve it. Ten minutes of skew is inside
    // the grace; the seq-rebirth case above (ts 100 vs 200+) stays apart
    // because rebirth is minutes-to-hours later, not seconds.
    val fastBill = msg(kind = 1, seq = 9, ts = 1000, outgoing = true, pxmr = 700)
    val quickNo = msg(kind = 5, seq = 2, ts = 1000 - 600, outgoing = false, reSeq = 9)
    check(billAnswers(listOf(fastBill, quickNo)).refused == setOf(9L to 1000L)) {
        "BILL_FAIL a ten-minute clock skew lost the refusal"
    }
    // And when the only candidate sits far past the grace, it is still the
    // one: one of the two clocks was simply wrong when it stamped (bills
    // minted under a clock set four days ahead, on the emulators, 2026-09-01),
    // and a refusal that could reach nothing else would leave a refused bill
    // reading as owed. The rebirth case stays apart because rebirth has a
    // predecessor, and a predecessor always wins.
    val farBill = msg(kind = 1, seq = 9, ts = 1000 + 86_400, outgoing = true, pxmr = 700)
    check(billAnswers(listOf(farBill, quickNo)).refused == setOf(9L to 1000L + 86_400)) {
        "BILL_FAIL a bill stamped by a wrong clock lost its refusal"
    }
    // Two far-future candidates: the earliest, the one that existed first.
    val fartherBill = msg(kind = 1, seq = 9, ts = 1000 + 172_800, outgoing = true, pxmr = 900)
    check(billAnswers(listOf(fartherBill, farBill, quickNo)).refused == setOf(9L to 1000L + 86_400)) {
        "BILL_FAIL two wrong-clock bills: the later one took the refusal"
    }

    println("BILL_OK a refusal answers the message it was sent about")
}
