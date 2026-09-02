package org.ducatproject.desk

import org.ducatproject.ducat.Publications

/**
 * A publisher's period id must not be able to name a path.
 *
 * §16.20 asks only that a period id be non-empty and at most 64 bytes,
 * and the protocol is right not to care what is in it — but the reader
 * files a downloaded issue at `publications/<publisherHex>/<period>`.
 * `../../veilid` satisfies every rule the wire has and names the node's
 * keystore, and what runs at that path is `deleteRecursively` followed by
 * a fetch writing into the space it cleared. It reaches the phone from
 * any contact it has not muted, in a kind-13, and goes off when the
 * reader taps an issue they were told they had.
 *
 * Three layers, each of which must refuse on its own: the id is never
 * filed, never handed back if an older build filed one, and never joined
 * into a path.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:periodidtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("PERIODID ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    // The ones that must be refused.
    val bad = listOf(
        "../../veilid",           // the node's keystore
        "../../att",              // attachments
        "..",                     // the publisher's whole folder
        ".",
        "a/b",                    // a nested path, not a name
        "..\\..\\windows",        // the other separator
        "",                       // §16.20 refuses this too
        "x".repeat(65),           // over the ceiling
    )
    for (id in bad) {
        check("refused: ${id.ifEmpty { "(empty)" }.take(24)}", !Publications.isSafePeriodId(id))
    }

    // …and the ones that must not be, or an honest publisher's naming
    // scheme breaks. A denylist, not an allowlist, is the point.
    val good = listOf(
        "2026-09",                // what this app writes
        "2026-W40",
        "Autumn 2026",            // a space is a name, not a route
        "issue.42",               // a dot inside is not a dot segment
        "весна-2026",             // not everyone names periods in ASCII
        "第三期",
        "x".repeat(64),           // exactly the ceiling
    )
    for (id in good) {
        check("allowed: ${id.take(24)}", Publications.isSafePeriodId(id))
    }

    if (failures > 0) {
        println("PERIODIDTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("PERIODIDTEST OK — ${bad.size} refused, ${good.size} allowed")
}
