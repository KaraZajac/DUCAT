package org.ducatproject.desk

import org.ducatproject.ducat.DucatLog

/**
 * The log file's ceiling, which was documented but not enforced.
 * `./gradlew :desktop:logcap`.
 *
 * FILE_CAP_BYTES says half a mebibyte — "enough for a day of testing, small
 * enough to share" — and the trim that honoured it ran in init() and nowhere
 * else. That is fine for an app that gets restarted, and wrong for this one:
 * DUCAT holds a foreground service open for as long as the phone is on. The
 * transport really does flap between AttachedStrong and AttachedFull, which is
 * a line every half-minute before anything else is written at all, so a phone
 * left running quietly grows a file nobody ever cuts back.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-log").toFile()
    val ctx = DeskContext(dir)
    DucatLog.clear()
    DucatLog.init(ctx)

    val f = java.io.File(dir, "files/ducat.log")
    val cap = 512 * 1024

    // Well past the cap: about two megabytes of ordinary lines, which on a
    // phone is a few days of transport flapping and nothing else.
    val filler = "x".repeat(200)
    repeat(10_000) { DucatLog.i("LogCap", "$it $filler") }

    check(f.exists()) { "LOGCAP_FAIL nothing was written" }
    check(f.length() <= cap) {
        "LOGCAP_FAIL grew to ${f.length() / 1024} KiB against a ${cap / 1024} KiB cap"
    }

    // Bounded is not enough. It has to keep the *newest* lines, because the
    // whole reason this file exists is the last thing that happened before a
    // crash — a trim that kept the head would throw away exactly that.
    val text = f.readText()
    check(text.contains("9999 ")) { "LOGCAP_FAIL the newest line was trimmed away" }
    check(!text.contains("|LogCap|0 ")) { "LOGCAP_FAIL the oldest line survived a full cap" }

    // And the in-memory ring the Logs screen reads still ends where the file
    // does, so what a user sends and what a user sees are the same events.
    val snap = DucatLog.snapshot()
    check(snap.isNotEmpty()) { "LOGCAP_FAIL the ring is empty" }
    check(snap.last().message.startsWith("9999 ")) {
        "LOGCAP_FAIL the ring's last entry is '${snap.last().message.take(20)}'"
    }

    println("LOGCAP_OK size=${f.length() / 1024}KiB cap=${cap / 1024}KiB newest=kept ring=${snap.size}")
}
