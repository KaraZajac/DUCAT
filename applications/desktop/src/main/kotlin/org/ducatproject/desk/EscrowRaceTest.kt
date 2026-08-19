package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import org.json.JSONObject

/**
 * Two writers, one ceremony record.
 *
 * Six functions in Ceremony read a record, do something slow — build a
 * transaction, run a FROST round, send a message — and write it back
 * afterwards, while the poller lands protocol rounds into the same record on
 * its own thread. A plain write puts back a snapshot taken before all of
 * that, so whichever finished last silently undid the other.
 *
 * What that costs: a DKG round overwritten is a ceremony that stalls with
 * nothing to restart it, and a funding mark overwritten is the only thing
 * standing between a second tap and a second payment into an escrow that
 * needs a co-signature to give anything back.
 *
 * `./gradlew :desktop:escrowracetest`
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("RACE ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    // A ceremony built and unfunded, as both threads first read it.
    val start = JSONObject().put("id", "abc").put("stage", "done")
        .put("fundTxid", "").put("round", 1).toString()

    // The poller landed a round while the screen was building a transaction.
    val moved = JSONObject(start).put("stage", "signing").put("round", 3).toString()

    // The screen, finishing its send, records the transaction on the record
    // it read before any of that happened.
    val screen = JSONObject(start).put("fundTxid", "deadbeef")

    val merged = Ceremony.mergeOnto(start, moved, screen)
    check(
        "the round that landed underneath survives",
        merged.optString("stage") == "signing" && merged.optInt("round") == 3,
        merged.toString(),
    )
    check(
        "and so does the funding mark",
        merged.optString("fundTxid") == "deadbeef",
        merged.toString(),
    )

    // The other order: the screen writes first, the poller second. The
    // poller's record never knew about the funding.
    val poller = JSONObject(start).put("stage", "signing")
    val afterScreen = JSONObject(start).put("fundTxid", "deadbeef").toString()
    val merged2 = Ceremony.mergeOnto(start, afterScreen, poller)
    check(
        "a payment already recorded is not rolled back by a round",
        merged2.optString("fundTxid") == "deadbeef" &&
            merged2.optString("stage") == "signing",
        merged2.toString(),
    )

    // Nobody else wrote: the record goes through untouched, and it must be
    // the same object so the caller's own edits are what land.
    check(
        "an uncontended write is left alone",
        Ceremony.mergeOnto(start, start, screen) === screen,
    )
    check("and so is one with no snapshot to compare", Ceremony.mergeOnto(null, moved, screen) === screen)

    // A field both of them changed: the caller doing the writing wins, which
    // is the only answer available and is what a plain write did too.
    val bothA = JSONObject(start).put("stage", "cancelled").toString()
    val bothB = JSONObject(start).put("stage", "signing")
    check(
        "when both changed one field, the writer's value lands",
        Ceremony.mergeOnto(start, bothA, bothB).optString("stage") == "signing",
    )

    println(if (failures == 0) "ESCROWRACE OK" else "ESCROWRACE FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
