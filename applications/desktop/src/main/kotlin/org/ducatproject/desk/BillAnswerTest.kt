package org.ducatproject.desk

import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.ui.billAnswers

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

    // A reaction before any bill answers nothing at all.
    val early = msg(kind = 5, seq = 9, ts = 50, outgoing = true, reSeq = 0)
    check(billAnswers(listOf(early, firstBill)).refused.isEmpty()) { "BILL_FAIL answered forwards" }

    // An emoji reaction (kind 4) is not a refusal.
    val emoji = msg(kind = 4, seq = 9, ts = 410, outgoing = true, reSeq = 0)
    check(billAnswers(listOf(firstBill, emoji)).refused.isEmpty()) { "BILL_FAIL kind 4" }

    println("BILL_OK a refusal answers the message it was sent about")
}
