package org.ducatproject.desk

import org.ducatproject.ducat.WalletEntry
import org.ducatproject.ducat.WalletStore
import java.util.concurrent.CountDownLatch
import kotlin.concurrent.thread

/**
 * Money that arrived must not be erased by a backfill.
 * `./gradlew :desktop:walletrace`.
 *
 * The wallet's output list is rewritten whole by every writer, and there are
 * three: the poller's scan records what it found, the spent check writes back
 * what the chain confirms, and the ledger's backfill fills in transaction ids
 * and block times. Each did its own read … write with nothing in between, so
 * two overlapping meant the second wrote a list it had read *before* the
 * first's change — and a freshly scanned output, already announced in the log
 * as received, simply vanished. The coin is still on the chain and a rescan
 * finds it, but the wallet has stopped counting it and "Ready to spend"
 * understates by whatever arrived.
 *
 * Orders documents this exact hazard at its own lock. The wallet is where it
 * costs the most and had none.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-walletrace").toFile()
    val store = WalletStore(DeskContext(dir))

    fun entry(n: Int, spent: Boolean = false) = WalletEntry(
        amountPxmr = 1_000L + n,
        height = 100L + n,
        spent = spent,
        keyImage = "ki%04d".format(n),
        blob = ByteArray(8) { n.toByte() },
        txHashHex = "",
        timestamp = 0L,
        minor = 0,
    )

    // Seed the store the way a scan would.
    store.mutateEntries { (0 until 20).map { entry(it) } }
    check(store.entries().size == 20) { "RACE_FAIL seeding failed: ${store.entries().size}" }

    // Now run the two writers against each other, hard: one adding freshly
    // "scanned" outputs, the other backfilling timestamps over everything.
    val ARRIVALS = 200
    val start = CountDownLatch(1)
    val scanner = thread(start = false) {
        start.await()
        for (i in 20 until 20 + ARRIVALS) {
            store.mutateEntries { cur -> cur + entry(i) }
        }
    }
    val backfill = thread(start = false) {
        start.await()
        repeat(ARRIVALS) {
            store.mutateEntries { cur -> cur.map { it.copy(timestamp = 1_700_000_000L) } }
        }
    }
    scanner.start(); backfill.start(); start.countDown()
    scanner.join(); backfill.join()

    // Every arrival must still be there. Before the lock, the backfill's write
    // routinely landed on top of a list read before an arrival and dropped it.
    val kis = store.entries().map { it.keyImage }.toSet()
    val missing = (0 until 20 + ARRIVALS).map { "ki%04d".format(it) }.filterNot { it in kis }
    check(missing.isEmpty()) {
        "RACE_FAIL ${missing.size} output(s) erased by a concurrent write — " +
            "e.g. ${missing.take(4)}"
    }
    check(store.entries().size == 20 + ARRIVALS) {
        "RACE_FAIL wrong count: ${store.entries().size}"
    }
    // And the backfill's work is not lost either — the last writer wins on a
    // field, which is fine; what must never happen is a whole entry vanishing.
    check(store.entries().all { it.keyImage.isNotEmpty() }) { "RACE_FAIL corrupt entry" }

    // A mutation that declines writes nothing, which is what stops a backfill
    // that recovered nothing from rewriting identical data every ten seconds.
    val before = store.entries().size
    store.mutateEntries { null }
    check(store.entries().size == before) { "RACE_FAIL a declined mutation still wrote" }

    println("WALLETRACE_OK arrivals=$ARRIVALS lost=0 kept=${store.entries().size} decline=noop")
}
