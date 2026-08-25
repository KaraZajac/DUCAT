package org.ducatproject.desk

import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.ui.freshRideOffer
import org.ducatproject.ducat.ui.offerStillOpen
import org.ducatproject.ducat.ui.rideOfferMark

/**
 * A second hail quotes the second driver, not the first one.
 * `./gradlew :desktop:staleoffer`.
 *
 * The rider posts a hail, a driver claims it, and the rider's client waits for
 * the fare to arrive as a kind-6. That wait used to end on whichever kind-6
 * was newest in the thread — which, three seconds in, is last ride's, because
 * nothing from this hail can have arrived yet. The rider was then shown a
 * fare from a completed ride and asked to accept it: found live on 2026-08-24,
 * where a second hail to the same driver quoted USD 6.01 / 0.013611 XMR while
 * the offer actually on the wire was 0.013619.
 *
 * Riding twice with the same driver is the ordinary case, not the exotic one,
 * so the wait has to distinguish *new* from *newest*.
 *
 * Identity here is (seq, timestamp) rather than seq alone, and that is the
 * part worth keeping: a hail cuts a fresh one-time card, which restarts the
 * mailbox, so both offers in a thread are legitimately seq 0. A comparison
 * against a moment on this phone's clock would be wrong for the mirror-image
 * reason — the timestamp in a message is the *sender's*.
 */
private fun offer(seq: Long, ts: Long, pxmr: Long, outgoing: Boolean = false) =
    StoredMessage(
        outgoing = outgoing, seq = seq, body = "on my way", timestamp = ts,
        kind = 6, amountPxmr = pxmr,
    )

fun main() {
    val first = offer(seq = 0, ts = 1_000, pxmr = 13_611_646_250)

    // The wait begins with last ride's offer already in the thread.
    val before = rideOfferMark(listOf(first))
    check(before == setOf(0L to 1_000L)) { "STALE_FAIL mark" }

    // Three seconds in, nothing has arrived. The old offer must not end it.
    check(freshRideOffer(listOf(first), before) == null) {
        "STALE_FAIL an offer from a finished ride ended the wait"
    }

    // The real offer lands — same seq, because the hail cut a fresh card.
    val second = offer(seq = 0, ts = 4_000, pxmr = 13_619_294_554)
    val got = freshRideOffer(listOf(first, second), before)
    check(got?.amountPxmr == 13_619_294_554L) {
        "STALE_FAIL waited for one fare and was handed another: ${got?.amountPxmr}"
    }

    // Order in the thread is not the discriminator either.
    check(freshRideOffer(listOf(second, first), before)?.amountPxmr == 13_619_294_554L) {
        "STALE_FAIL thread order decided it"
    }

    // Two drivers answer one hail: the newest of the *new* ones wins, and the
    // stale one still loses however new it looks beside it.
    val third = offer(seq = 1, ts = 4_500, pxmr = 20_000_000_000)
    check(freshRideOffer(listOf(first, second, third), before)?.amountPxmr == 20_000_000_000L) {
        "STALE_FAIL newest of the fresh"
    }

    // Our own outgoing kind-6 is not an offer to us.
    val mine = offer(seq = 9, ts = 9_000, pxmr = 99, outgoing = true)
    check(freshRideOffer(listOf(first, mine), before) == null) {
        "STALE_FAIL accepted this device's own message as an offer"
    }
    check(rideOfferMark(listOf(mine)).isEmpty()) { "STALE_FAIL marked an outgoing message" }

    // A first-ever ride: empty mark, and the one offer that comes is taken.
    check(freshRideOffer(listOf(second), emptySet())?.amountPxmr == 13_619_294_554L) {
        "STALE_FAIL first ride"
    }

    // Other kinds in the thread are not fares.
    val chat = StoredMessage(
        outgoing = false, seq = 3, body = "see you", timestamp = 5_000, kind = 0,
    )
    check(freshRideOffer(listOf(first, chat), before) == null) { "STALE_FAIL kind" }

    // ---- and the fare survives the screen that showed it ----
    //
    // Killed between the offer landing and the yes, the rider used to come
    // back to nothing: the thread kept the kind-6, but a chat bubble accepts
    // nothing, and the driver who had claimed the hail waited on somebody who
    // could no longer see the fare.
    val ttl = 15L * 60
    val now = 5_000L
    val live = offer(seq = 0, ts = now - 60, pxmr = 41_913_000_000)
    check(offerStillOpen(listOf(live), 0, now, ttl)?.amountPxmr == 41_913_000_000L) {
        "OPEN_FAIL a live offer was not offered back"
    }

    // Already accepted — nothing outstanding.
    val accept = StoredMessage(
        outgoing = true, seq = 0, body = "yes", timestamp = now - 30,
        kind = 7, amountPxmr = 41_913_000_000, reSeq = 0,
    )
    check(offerStillOpen(listOf(live, accept), 0, now, ttl) == null) { "OPEN_FAIL accepted" }

    // Already declined.
    val decline = StoredMessage(
        outgoing = true, seq = 1, body = "no", timestamp = now - 30, kind = 5, reSeq = 0,
    )
    check(offerStillOpen(listOf(live, decline), 0, now, ttl) == null) { "OPEN_FAIL declined" }

    // An answer from *before* the offer answers a different one: a fresh
    // one-time card restarts the mailbox, so last ride's accept carries the
    // same reSeq and must not silence this fare.
    val oldAccept = StoredMessage(
        outgoing = true, seq = 0, body = "yes", timestamp = now - 3_000,
        kind = 7, amountPxmr = 13_611_646_250, reSeq = 0,
    )
    check(offerStillOpen(listOf(live, oldAccept), 0, now, ttl)?.amountPxmr == 41_913_000_000L) {
        "OPEN_FAIL an older ride's yes answered this ride's offer"
    }

    // Past the time a hail stands for, it is not a fare any more.
    val dead = offer(seq = 0, ts = now - ttl - 1, pxmr = 41_913_000_000)
    check(offerStillOpen(listOf(dead), 0, now, ttl) == null) { "OPEN_FAIL expired" }

    // A sender's clock running ahead must not hide a live offer.
    val ahead = offer(seq = 0, ts = now + 90, pxmr = 41_913_000_000)
    check(offerStillOpen(listOf(ahead), 0, now, ttl) != null) { "OPEN_FAIL clock skew" }

    println("STALE_OK a second hail is quoted its own fare, and a fare outlives its screen")
}
