package org.ducatproject.desk

import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.formatXmr
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * Renting discovery, end to end over the live network (§16.18).
 *
 * One desk posts a car and a room to the boards around a place; another desk
 * — a different identity, a different directory, no contact between them —
 * searches that place and finds them. That is the whole claim of the
 * feature: two strangers converge on a DHT record whose address is the
 * neighbourhood itself.
 *
 * It also checks the half that matters more than the finding: that the
 * private half of a listing is nowhere in the bytes a stranger reads.
 *
 *   DUCAT_LIST_ROLE=owner  DUCAT_DESK_STATE=/tmp/own ./gradlew :desktop:listtest
 *   DUCAT_LIST_ROLE=seeker DUCAT_DESK_STATE=/tmp/see ./gradlew :desktop:listtest
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("LIST_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val role = System.getenv("DUCAT_LIST_ROLE")?.takeIf { it.isNotEmpty() }
        ?: error("LIST_FAIL set DUCAT_LIST_ROLE to owner or seeker")
    // Somewhere fixed, so both roles agree without talking: a spot in Berlin.
    val lat = System.getenv("DUCAT_LIST_LAT")?.toLongOrNull() ?: 525200000L
    val lon = System.getenv("DUCAT_LIST_LON")?.toLongOrNull() ?: 134050000L

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    NameStore(context).get() ?: NameStore(context).put(role.replaceFirstChar { it.uppercase() })

    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "LIST_FAIL node never became ready" }
    println("LIST_UP $role at ${uniffi.ducat_mobile.geohashEncode(lat, lon, 5u)}")

    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("LIST ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    if (role == "owner") {
        // The private half is a string a stranger must never see. Making it
        // distinctive is what lets the assertion below mean something.
        val secret = "12 Rosenthaler Str, flat 4, keys in the cafe next door"
        val car = Listings.draft(
            context, Listings.KIND_VEHICLE,
            title = "2019 Corolla, automatic",
            area = "north side",
            pricePxmr = 40_000_000_000L,
            latE7 = lat, lonE7 = lon,
            specs = JSONObject().apply {
                put("make", "Toyota"); put("model", "Corolla"); put("year", 2019L)
                put("color", "silver"); put("trim", "Hybrid LE"); put("seats", 5L)
                put("gearbox", 2L); put("fuel", 4L); put("subtype", 1L)
                put("features", JSONArray(listOf("child seat")))
            },
            privateDetails = secret,
        )
        val room = Listings.draft(
            context, Listings.KIND_PLACE,
            title = "Sunny room near the park",
            area = "Kreuzberg",
            pricePxmr = 25_000_000_000L,
            latE7 = lat, lonE7 = lon,
            specs = JSONObject().apply {
                put("rooms", 1L); put("sleeps", 2L); put("size_m2", 28L); put("subtype", 2L)
                put("features", JSONArray(listOf("wifi")))
            },
            privateDetails = secret,
        )
        for (o in listOf(car, room)) {
            Listings.put(context, o)
            val ok = runCatching { Listings.post(context, o.optString("id")) }
                .onFailure { println("LIST_FAIL post: ${it.message}") }
                .getOrDefault(false)
            check("posted ${o.optString("title")}", ok)
        }

        // The claim §16.18 makes about itself: the bytes on the board carry
        // nothing that would get a stranger to the door.
        val onBoard = Listings.all(context).mapNotNull { o ->
            runCatching {
                uniffi.ducat_mobile.rentalEncode(Listings.publicNotice(o, o.optString("card")))
            }.getOrNull()
        }.joinToString("\n") { it.decodeToString() }
        check("the address is not in what goes on the board", !onBoard.contains("Rosenthaler"))
        check("nor the key handover", !onBoard.contains("keys in the cafe"))
        check("but the searchable shape is", onBoard.contains("Corolla"), "make and model")

        println("LIST_POSTED — leave this running while the seeker looks")
        // Boards are DHT records; stay up so they stay reachable.
        while (true) {
            Thread.sleep(30_000)
            Listings.needRefresh(context).forEach {
                runCatching { Listings.post(context, it.optString("id")) }
            }
        }
    }

    // ---- seeker ----
    // How long each board takes to answer, because a search that reads nine
    // of them is only as fast as the slowest, and an empty board is the slow
    // case: the network has to conclude the record is not there.
    // Opt-in, because measuring nine boards serially is the very cost the
    // parallel search exists to avoid — it should not be paid on every run.
    if (System.getenv("DUCAT_LIST_TIMING") == "1") {
        val home = uniffi.ducat_mobile.geohashEncode(lat, lon, 5u)
        val ring = listOf(home) + uniffi.ducat_mobile.geohashNeighbors(home)
        for (cell in ring) {
            val t0 = System.currentTimeMillis()
            val n = runCatching { uniffi.ducat_mobile.standRead("rent:$cell") }
                .getOrDefault(emptyList()).size
            println("LIST_BOARD rent:$cell $n notice(s) in ${System.currentTimeMillis() - t0} ms")
        }
    }

    var cars: List<uniffi.ducat_mobile.RentalInfo> = emptyList()
    var rooms: List<uniffi.ducat_mobile.RentalInfo> = emptyList()
    // One search per kind, each reporting as boards answer — the same path
    // the screen uses, so what is timed here is what a person waits for.
    val t0 = System.currentTimeMillis()
    runCatching {
        Listings.search(lat, lon, Listings.KIND_VEHICLE) { sofar ->
            if (sofar.size != cars.size) {
                println("LIST_PARTIAL cars=${sofar.size} after ${System.currentTimeMillis() - t0} ms")
            }
            cars = sofar
        }
    }
    val t1 = System.currentTimeMillis()
    runCatching {
        Listings.search(lat, lon, Listings.KIND_PLACE) { sofar ->
            if (sofar.size != rooms.size) {
                println("LIST_PARTIAL rooms=${sofar.size} after ${System.currentTimeMillis() - t1} ms")
            }
            rooms = sofar
        }
    }

    check("found a car on the boards", cars.isNotEmpty(), cars.firstOrNull()?.title ?: "")
    check("found a room", rooms.isNotEmpty(), rooms.firstOrNull()?.title ?: "")
    cars.firstOrNull()?.let { c ->
        check("the car's searchable shape arrived",
            c.make == "Toyota" && c.model == "Corolla" && c.trim == "Hybrid LE",
            "${c.year} ${c.make} ${c.model} ${c.trim}")
        check("its price and stake arrived",
            c.pricePxmr > 0uL && c.depositPxmr > 0uL,
            "${formatXmr(c.pricePxmr.toLong())} / stake ${formatXmr(c.depositPxmr.toLong())}")
        check("a stranger gets a card to ask with", c.card.startsWith("ducat:"))
        // The whole privacy argument, checked from the far side.
        val everything = listOfNotNull(
            c.title, c.area, c.make, c.model, c.color, c.trim,
        ).joinToString(" ")
        check("and no address anywhere in it", !everything.contains("Rosenthaler"))
    }
    rooms.firstOrNull()?.let { p ->
        check("the room's shape arrived",
            p.rooms == 1uL && p.sleeps == 2uL && p.sizeM2 == 28uL,
            "${p.rooms} bed, sleeps ${p.sleeps}, ${p.sizeM2} m²")
        check("a place carries no vehicle fields", p.make == null && p.gearbox == null)
    }

    println(if (failures == 0) "LISTTEST OK" else "LISTTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
