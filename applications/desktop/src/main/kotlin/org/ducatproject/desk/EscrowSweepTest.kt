package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony

/**
 * What the escrow sweep removes, and — the part that matters — what it does
 * not. `./gradlew :desktop:escrowsweep`.
 *
 * The ceremony store never had a sweep: every round-0 that arrived wrote a
 * record and nothing removed one. The risk in adding a sweep is not that it
 * fails to delete; it is that it deletes an escrow somebody's money is sitting
 * in. So the cases below are mostly the ones that must survive.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-sweep").toFile()
    val ctx = DeskContext(dir)
    val now = System.currentTimeMillis()
    val hour = 60L * 60 * 1000
    val day = 24 * hour

    fun put(id: String, json: String) =
        ctx.getSharedPreferences("ducat_ceremonies", 0).edit().putString("c_$id", json).apply()

    // Goes: nobody ever answered it, and nothing was ever funded.
    put("dead", """{"id":"dead","stage":"committed","created":${now - 2 * hour}}""")
    // Goes: over, and old enough that nobody is still looking at it.
    put("old", """{"id":"old","stage":"released","created":${now - 30 * day}}""")

    // Stays: funded. The way out of one of these is a release or an arbiter,
    // never a sweep — even though nobody has answered in a month.
    put("funded", """{"id":"funded","stage":"committed","created":${now - 30 * day},"fundedPxmr":500000}""")
    // Stays: funded by the other side, recorded as a txid rather than a scan.
    put("hostpaid", """{"id":"hostpaid","stage":"committed","created":${now - 30 * day},"hostFundTxid":"ab12"}""")
    // Stays: this device paid into it.
    put("wepaid", """{"id":"wepaid","stage":"committed","created":${now - 30 * day},"fundTxid":"cd34"}""")
    // Stays: live and recent.
    put("live", """{"id":"live","stage":"shared","created":${now - 60_000}}""")
    // Stays: settled, but only just — still worth looking at.
    put("justdone", """{"id":"justdone","stage":"released","created":${now - hour}}""")

    val removed = Ceremony.sweep(ctx)
    val p = ctx.getSharedPreferences("ducat_ceremonies", 0)
    fun gone(id: String) = p.getString("c_$id", null) == null

    check(removed == 2) { "SWEEPTEST_FAIL removed $removed, expected 2" }
    check(gone("dead")) { "SWEEPTEST_FAIL an unanswered escrow survived" }
    check(gone("old")) { "SWEEPTEST_FAIL a long-finished escrow survived" }

    for (id in listOf("funded", "hostpaid", "wepaid", "live", "justdone")) {
        check(!gone(id)) { "SWEEPTEST_FAIL the sweep took '$id', which it must never take" }
    }

    // Idempotent: a second pass finds nothing and says so.
    check(Ceremony.sweep(ctx) == 0) { "SWEEPTEST_FAIL the sweep is not idempotent" }

    println("SWEEPTEST_OK removed=2 kept=funded,hostpaid,wepaid,live,justdone idempotent")
}
