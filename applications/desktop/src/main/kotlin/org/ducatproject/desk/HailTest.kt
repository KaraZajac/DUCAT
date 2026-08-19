package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Hailing
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.formatXmr
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * A hail, from the kerb to the driver's map — over the live network.
 *
 * The hail was the last of the three flows with no harness at all, because
 * both halves of it lived inside screens: the post inside a button, the sweep
 * inside a `LaunchedEffect`. They are plain functions now ([Hailing]), which
 * is what makes this possible — and what makes it worth having, since the
 * overflow ladder, the deserted-corner copy and the shard climbing are real
 * logic with real edge cases that had only ever been exercised by hand.
 *
 * Two roles, and no contact between them: the geocell *is* the rendezvous.
 *
 *   DUCAT_HAIL_ROLE=rider  DUCAT_DESK_STATE=/tmp/h1 ./gradlew :desktop:hailtest
 *   DUCAT_HAIL_ROLE=driver DUCAT_DESK_STATE=/tmp/h2 ./gradlew :desktop:hailtest
 *
 * The driver sweeps the rider's cell and the ring around it, exactly as the
 * map does, and reports how long a fare took to appear. That number is the
 * whole question for somebody sitting in a car.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("HAIL_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val role = System.getenv("DUCAT_HAIL_ROLE")?.takeIf { it.isNotEmpty() }
        ?: error("HAIL_FAIL set DUCAT_HAIL_ROLE to rider or driver")
    // Somewhere fixed, so both roles agree without talking. Precision 6 is
    // the fine cell a hail is posted to (§15.12).
    val lat = System.getenv("DUCAT_HAIL_LAT")?.toLongOrNull() ?: 525200000L
    val lon = System.getenv("DUCAT_HAIL_LON")?.toLongOrNull() ?: 134050000L
    val waitSecs = System.getenv("DUCAT_HAIL_SECS")?.toLongOrNull() ?: 300L

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    NameStore(context).get() ?: NameStore(context).put(role.replaceFirstChar { it.uppercase() })

    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 120_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "HAIL_FAIL node never became ready" }

    val cell = uniffi.ducat_mobile.geohashEncode(lat, lon, 6u)
    println("HAIL_UP $role at $cell")

    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("HAIL ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    if (role == "rider") {
        // A destination a driver can read, and a fare they can decide about.
        val dest = uniffi.ducat_mobile.geohashEncode(lat + 30_000L, lon + 30_000L, 6u)
        val fare = System.getenv("DUCAT_HAIL_FARE")?.toULongOrNull() ?: 800_000_000uL
        val at = System.currentTimeMillis()
        val standing = runCatching { Hailing.post(context, cell, dest, "Alexanderplatz", fare) }
            .onFailure { println("HAIL_FAIL post: ${it.message}") }
            .getOrNull() ?: return
        println(
            "HAIL_POSTED at ${System.currentTimeMillis()} on ${standing.board}/" +
                "${standing.subkey} (${System.currentTimeMillis() - at} ms, " +
                "fare ${formatXmr(fare.toLong())})",
        )
        check("the notice went onto a board", standing.board.startsWith("geo:"), standing.board)
        check("and the card that answers it is claim-once", standing.cardUri.startsWith("ducat:"))

        // §15.12's density rule: a deserted corner earns a copy on the
        // containing 5-cell, where a driver kilometres away is looking.
        val wide = Hailing.wideCopy(context, standing)
        println(
            if (standing.aloneHere) {
                "HAIL_WIDE ${wide?.first ?: "none"} (the corner was deserted)"
            } else {
                "HAIL_WIDE none needed — somebody else is already standing here"
            },
        )

        // Stay up: the board is a DHT record, and a rider who walks away
        // takes their notice's reachability with them.
        println("HAIL_STANDING — leave this running while the driver looks")
        while (System.currentTimeMillis() - at < waitSecs * 1000) Thread.sleep(5_000)
        println(if (failures == 0) "HAILTEST OK (rider)" else "HAILTEST FAILED ($failures)")
        if (failures > 0) kotlin.system.exitProcess(1)
        return
    }

    // ---- driver ----
    // The nine fine cells a driver watching "Nearby" covers, plus the
    // containing 5-cell and its ring — the same net the map draws.
    val ring = listOf(cell) + uniffi.ducat_mobile.geohashNeighbors(cell)
    val wide = uniffi.ducat_mobile.geohashEncode(lat, lon, 5u)
    val cells = (ring + listOf(wide) + uniffi.ducat_mobile.geohashNeighbors(wide))
        .distinct().map { "geo:$it" }
    println("HAIL_WATCHING ${cells.size} board(s)")
    val armed = cells.count { runCatching { uniffi.ducat_mobile.standWatch(it) }.getOrDefault(false) }
    check("the boards can actually be watched", armed > 0, "$armed of ${cells.size}")

    val started = System.currentTimeMillis()
    var seen: Hailing.Seen? = null
    var laps = 0
    while (seen == null && System.currentTimeMillis() - started < waitSecs * 1000) {
        val now = System.currentTimeMillis() / 1000
        laps++
        val lapAt = System.currentTimeMillis()
        for (c in cells) {
            seen = Hailing.sweepCell(c, now)?.firstOrNull() ?: continue
            break
        }
        println(
            "HAIL_LAP $laps over ${cells.size} board(s) in " +
                "${(System.currentTimeMillis() - lapAt) / 1000}s" +
                (seen?.let { " — found one" } ?: ""),
        )
    }
    val found = seen
    check(
        "a driver finds a standing hail",
        found != null,
        found?.let { "${it.dest}, ${formatXmr(it.farePxmr ?: 0L)} XMR" } ?: "nothing in ${waitSecs}s",
    )
    if (found != null) {
        println("HAIL_FOUND at ${System.currentTimeMillis()} after ${(System.currentTimeMillis() - started) / 1000}s")
        check("it carries somewhere to go", found.dest.isNotBlank(), found.dest)
        check("and a card to answer it with", found.card.startsWith("ducat:"))
        check("and it has not expired", found.expiry > System.currentTimeMillis() / 1000)
    }
    println(if (failures == 0) "HAILTEST OK (driver)" else "HAILTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
