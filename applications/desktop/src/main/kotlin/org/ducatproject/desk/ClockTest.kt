package org.ducatproject.desk

import org.ducatproject.ducat.Elapsed

/**
 * Three questions about a clock, and the three different right answers.
 *
 * [Elapsed.due] exists because "now - stamp >= window" never fires once a
 * stamp sits ahead of now, so a refresh that missed its window missed it
 * for ever. Its answer — a stamp this device cannot vouch for reads as
 * *due* — is right for a refresh and wrong twice over:
 *
 *  - For an irreversible action it is backwards. TabStore.sweepAbandoned
 *    said so first; ContactStore.pruneCards and Wallet2.refreshSpent were
 *    doing it anyway, one deleting every live card at once and the other
 *    releasing the notes of a payment that may still be in flight.
 *  - For a stamp the *counterparty* wrote it is not this device's clock at
 *    all. Treating it as due deletes something the reader may still be
 *    looking at; treating it raw hands the bound to the sender, who can
 *    hold a ring "fresh" or a message un-expirable for ever by stamping
 *    ahead. [Elapsed.notAhead] caps it at now instead.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:clocktest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("CLOCK ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val now = 1_700_000_000_000L
    val hour = 3_600_000L

    // --- due: the refresh rule, unchanged -------------------------------
    check("a fresh stamp is not due", !Elapsed.due(now, now - 60_000, hour))
    check("an old stamp is due", Elapsed.due(now, now - 2 * hour, hour))
    check("a future stamp reads as due", Elapsed.due(now, now + 10 * hour, hour))
    check(
        "and a second of skew does not",
        !Elapsed.due(now, now + 1_000, hour),
        "the slack is ${Elapsed.FUTURE_SLACK_MS}ms",
    )

    // --- somebody else's stamp: due is the freshness test ---------------
    //
    // The first attempt here capped a future stamp at now, which reads well
    // and is wrong: age becomes zero, i.e. maximally *fresh*, so the ring
    // still rang, the accept never aged out and the message was never
    // expired. This test caught that before it shipped. Elapsed's own
    // future rule is the right tool — "not due" is what fresh means, and a
    // stamp this device cannot vouch for is not fresh.
    val theirs = now + 365L * 24 * hour
    val nowS = now / 1000
    val theirsS = theirs / 1000

    check(
        "a ring stamped a year ahead is not fresh",
        !(!Elapsed.dueSecs(nowS, theirsS, 90 + 120)),
    )
    check(
        "an honest ring still rings",
        !Elapsed.dueSecs(nowS, nowS - 10, 90 + 120),
    )
    check(
        "a ride accept stamped ahead is expired",
        Elapsed.dueSecs(nowS, theirsS, 12 * 3600),
    )
    check(
        "an accept from an hour ago is not",
        !Elapsed.dueSecs(nowS, nowS - 3600, 12 * 3600),
    )
    check(
        "a message stamped ahead is past its window",
        Elapsed.dueSecs(nowS, theirsS, 3600),
    )
    check(
        "a message from a minute ago is kept",
        !Elapsed.dueSecs(nowS, nowS - 60, 3600),
    )
    // The property that matters: stamping ahead can never buy more time
    // than stamping honestly.
    check(
        "stamping ahead never beats stamping now",
        (0..20).all { i -> Elapsed.dueSecs(nowS, nowS + i * 3600L, 3600) || i == 0 },
    )

    // --- and the shape a delete must use --------------------------------
    // Not an Elapsed call at all: a plain subtraction, which is what
    // pruneCards, refreshSpent and sweepAbandoned now do.
    val made = now + 10 * hour
    check("a future stamp is NOT old by plain subtraction", (now - made) < hour)
    check("which is what keeps a live card alive", !((now - made) >= hour))

    if (failures > 0) {
        println("CLOCKTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("CLOCKTEST OK")
}
