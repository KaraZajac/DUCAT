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

    // The case the first version of this test did not think of, and the one
    // that mattered most: a record holding a key share for an escrow *somebody
    // else* funds. Every funding test in isStale asks whether **this** device
    // has seen money, and these parties never will.
    //
    // An arbiter neither funds nor is shown the ride banner that scans, so its
    // three funding marks stay empty for ever. Before the fix its share was
    // deleted thirty minutes after the ceremony finished, turning every 2-of-3
    // into a 2-of-2 — silently, and exactly when nobody was looking.
    put("arbiter", """{"id":"arbiter","stage":"done","created":${now - 2 * hour},"keys":"a1b2c3"}""")
    // Same shape, different reason: a one-sided ride's driver stakes nothing
    // by design, and a reservation host may set a zero deposit.
    put("nostake", """{"id":"nostake","stage":"done","created":${now - 30 * day},"keys":"d4e5f6"}""")
    // And a finished one, old enough for the seven-day rule. A release is
    // recorded when the co-signature is made, not when the transaction
    // relays — so "over" is not proof the money moved.
    put("cosigned", """{"id":"cosigned","stage":"release_cosigned","created":${now - 30 * day},"keys":"99aa"}""")

    val removed = Ceremony.sweep(ctx)
    val p = ctx.getSharedPreferences("ducat_ceremonies", 0)
    fun gone(id: String) = p.getString("c_$id", null) == null

    check(removed == 2) { "SWEEPTEST_FAIL removed $removed, expected 2" }
    check(gone("dead")) { "SWEEPTEST_FAIL an unanswered escrow survived" }
    check(gone("old")) { "SWEEPTEST_FAIL a long-finished escrow survived" }

    for (id in listOf("funded", "hostpaid", "wepaid", "live", "justdone",
                      "arbiter", "nostake", "cosigned")) {
        check(!gone(id)) { "SWEEPTEST_FAIL the sweep took '$id', which it must never take" }
    }
    // Stated as the invariant rather than as three examples, so the next
    // record shape nobody thought of is covered by the same line.
    check(!Ceremony.isStale(org.json.JSONObject("""{"id":"x","created":1,"keys":"ff"}"""))) {
        "SWEEPTEST_FAIL a record holding a key share was called stale"
    }
    check(Ceremony.isStale(org.json.JSONObject("""{"id":"x","created":1}"""))) {
        "SWEEPTEST_FAIL an unanswered proposal with no share should still go"
    }

    // Idempotent: a second pass finds nothing and says so.
    check(Ceremony.sweep(ctx) == 0) { "SWEEPTEST_FAIL the sweep is not idempotent" }

    println("SWEEPTEST_OK removed=2 kept=8 (incl. arbiter, nostake, cosigned) shares=never idempotent")
}
