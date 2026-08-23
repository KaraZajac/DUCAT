package org.ducatproject.desk

import java.io.File
import uniffi.ducat_mobile.nodeDhtWatch
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeWaitChange
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead
import uniffi.ducat_mobile.standRecordKey

/**
 * Does a watched board actually ring?
 *
 * The driver's map arms `node_dht_watch` on every cell it watches and then
 * sleeps on `node_wait_change`, so that a fare posted a hundred metres away
 * reaches the screen in seconds instead of at the end of a sweep. The sweep
 * is the guarantee; the watch is what makes hailing feel instant. Measured on
 * the emulator, a lap of eighteen boards takes 44 to 64 seconds — which is
 * either the floor, if the watch never rings, or a backstop nobody normally
 * waits for. Nothing had ever told us which.
 *
 * Two roles against one board on the live network:
 *
 *   DUCAT_WATCH_ROLE=watcher DUCAT_DESK_STATE=/tmp/w1 ./gradlew :desktop:watchtest
 *   DUCAT_WATCH_ROLE=poster  DUCAT_DESK_STATE=/tmp/w2 ./gradlew :desktop:watchtest
 *
 * Both derive the same board from DUCAT_WATCH_CELL, so they need no contact
 * with each other — the geocell *is* the rendezvous, which is the whole idea
 * behind §15.12. The watcher prints WATCH_RANG with how long it took, or
 * WATCH_SILENT if the deadline passed with nothing; then it reads the board
 * either way, so a silent watch on a board that did change is reported as
 * exactly that rather than as a failure to post.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("WATCH_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val role = System.getenv("DUCAT_WATCH_ROLE")?.takeIf { it.isNotEmpty() }
        ?: error("WATCH_FAIL set DUCAT_WATCH_ROLE to watcher or poster")
    // A cell no test has used, so the board starts empty and the only change
    // on it is the one this run makes.
    val cell = org.ducatproject.ducat.standNow(
        System.getenv("DUCAT_WATCH_CELL")?.takeIf { it.isNotEmpty() } ?: "geo:u0zh7wx",
    )
    val waitSecs = System.getenv("DUCAT_WATCH_SECS")?.toLongOrNull() ?: 240L

    Unlock.orExit(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "WATCH_FAIL node never became ready" }
    println("WATCH_UP $role on $cell")

    if (role == "poster") {
        // Give the watcher time to arm before there is anything to hear.
        val delay = System.getenv("DUCAT_WATCH_DELAY")?.toLongOrNull() ?: 45L
        println("WATCH_WAITING ${delay}s so the watcher can arm first")
        Thread.sleep(delay * 1000)
        val note = "watchtest ${System.currentTimeMillis()}".toByteArray()
        val at = System.currentTimeMillis()
        runCatching { standPost(cell, 0u, note) }
            .onSuccess {
                // Absolute, so the watcher's ring can be subtracted from it:
                // what matters is post-to-notification, and each side only
                // knows its own clock.
                println(
                    "WATCH_POSTED at ${System.currentTimeMillis()} " +
                        "(the set itself took ${System.currentTimeMillis() - at} ms)",
                )
            }
            .onFailure { println("WATCH_FAIL post: ${it.message}"); return }
        println("WATCHTEST OK (poster)")
        return
    }

    // Arm first, then sleep on the flag exactly as the driver's sweep does.
    val key = runCatching { standRecordKey(cell) }.getOrNull()
        ?: run { println("WATCH_FAIL no record key for $cell"); return }
    // Open first when asked to: watch_dht_values needs the record open in this
    // process, and the phone's sweep does not open it — which is the thing
    // under test. DUCAT_WATCH_OPEN=1 is the control.
    // standWatch opens the board (creating it if nobody has pinned that
    // corner yet) and leaves it open, which is what watching requires;
    // DUCAT_WATCH_RAW=1 arms it the old way, to keep the failure reproducible.
    val armed = if (System.getenv("DUCAT_WATCH_RAW") == "1") {
        runCatching { nodeDhtWatch(key) }
    } else {
        runCatching { uniffi.ducat_mobile.standWatch(cell) }
    }
    println(
        "WATCH_ARMED ${armed.getOrNull()} ${key.take(24)}… " +
            (armed.exceptionOrNull()?.message ?: ""),
    )

    val started = System.currentTimeMillis()
    var rang = false
    while (System.currentTimeMillis() - started < waitSecs * 1000) {
        if (nodeWaitChange(10_000u)) { rang = true; break }
    }
    val took = (System.currentTimeMillis() - started) / 1000
    if (rang) {
        println("WATCH_RANG at ${System.currentTimeMillis()} (${took}s after arming)")
        // And *which* record. A ring that cannot say which of eighteen boards
        // moved leaves a driver reading all eighteen, which is the lap the
        // watch exists to avoid.
        val moved = runCatching { uniffi.ducat_mobile.nodeChangedKeys() }
            .getOrDefault(emptyList())
        println(
            if (moved.contains(key)) {
                "WATCH_NAMED the ring named this board (${moved.size} key(s) in it)"
            } else {
                "WATCH_UNNAMED the ring did not name this board — ${moved.size} key(s): " +
                    moved.joinToString(",") { it.take(16) }
            },
        )
    } else {
        println("WATCH_SILENT after ${took}s")
    }

    // Either way, is the post actually on the board? A watch that never rang
    // on a board that never changed says nothing about watches.
    val found = runCatching { standRead(cell) }.getOrDefault(emptyList())
    println("WATCH_BOARD ${found.size} notice(s) on $cell")
    println(
        when {
            rang && found.isNotEmpty() -> "WATCHTEST OK — the watch rang, and the board had it"
            !rang && found.isNotEmpty() ->
                "WATCHTEST SILENT — the board changed and no notification came; " +
                    "the sweep is the only thing finding fares"
            else -> "WATCHTEST INCONCLUSIVE — nothing was on the board to notice"
        },
    )
}
