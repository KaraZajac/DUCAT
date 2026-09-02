package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Listings
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The pipe-width experiment (2026-09-01): time one 9-board ring read of a
 * quiet area against the live network, under whatever DUCAT_DHT_OPS says.
 *
 *   DUCAT_DHT_OPS=16 ./gradlew :desktop:boardbench   # veilid's default
 *   DUCAT_DHT_OPS=72 ./gradlew :desktop:boardbench   # the whole wave at once
 *
 * The fix is the middle of the South Atlantic: nine boards nobody has ever
 * posted to, so every read pays the full empty-board timeout and the wave's
 * width is the only variable. Prints BENCH_RING_MS. Run each width a couple
 * of times; a fresh store each run keeps the paint cache out of the timing.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-bench").toFile()
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "BENCH_FAIL node never became ready" }
    System.err.println("node ready, ops width = ${System.getenv("DUCAT_DHT_OPS") ?: "72 (default)"}")

    // -34.5, -14.2: open ocean between Brazil and Angola.
    val started = System.currentTimeMillis()
    val replied = Listings.search(
        context,
        (-34.5 * 1e7).toLong(), (-14.2 * 1e7).toLong(),
        null,
        onFound = { },
        onProgress = { done, total -> System.err.println("  $done of $total") },
    )
    val ms = System.currentTimeMillis() - started
    println("BENCH_RING_MS $ms replied=$replied")
}
