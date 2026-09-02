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
    // A notice is stamped against the chain tip (§16.18.1), and a reader
    // confirms the stamp against the same chain — so both roles need a
    // Monero node, and a fresh directory has none remembered. This used to
    // pass only in a directory `:desktop:wallet` had already visited, and
    // failed every post in a clean one with "no recent Monero block".
    if (org.ducatproject.ducat.NodeStore(context).lastGood() == null) {
        runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            )
        }.onSuccess { org.ducatproject.ducat.NodeStore(context).rememberLastGood(it.url) }
            .onFailure { println("LIST_WARN no Monero node reachable: ${it.message}") }
    }
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
        // The three nouns added in 0.89, on the same board. They carry no
        // typed extras — a category and a few tags is everything — so what
        // this proves is that the board takes them and a stranger reads them
        // back, which is the half a unit test cannot reach.
        val bike = Listings.draft(
            context, Listings.KIND_SALE,
            title = "Bicycle, barely ridden",
            area = "north side",
            pricePxmr = 90_000_000_000L,
            latE7 = lat, lonE7 = lon,
            specs = JSONObject().apply {
                put("subtype", 4L) // sport
                put("features", JSONArray(listOf("54cm frame", "new tyres")))
            },
            privateDetails = secret,
        )
        val kayak = Listings.draft(
            context, Listings.KIND_GEAR,
            title = "Sea kayak, paddle included",
            area = "by the lake",
            pricePxmr = 15_000_000_000L,
            latE7 = lat, lonE7 = lon,
            specs = JSONObject().apply {
                put("subtype", 3L) // outdoor
                put("features", JSONArray(listOf("two seats")))
            },
            privateDetails = secret,
        )
        val sparks = Listings.draft(
            context, Listings.KIND_SKILL,
            title = "Electrician, 20 years",
            area = "Kreuzberg",
            pricePxmr = 30_000_000_000L,
            latE7 = lat, lonE7 = lon,
            specs = JSONObject().apply {
                put("subtype", 1L) // electrical
                put("features", JSONArray(listOf("rewiring", "certificates")))
            },
            privateDetails = secret,
        )

        // §15.12's overflow ladder, which a rental cell reaches far sooner
        // than a taxi stand does: a five-kilometre cell in a city holds
        // every car and room in a neighbourhood, and a board is eight slots.
        // DUCAT_LIST_CROWD asks for that many listings so the ladder is
        // exercised rather than assumed.
        val crowd = System.getenv("DUCAT_LIST_CROWD")?.toIntOrNull() ?: 0
        val extras = (1..crowd).map { i ->
            Listings.draft(
                context, Listings.KIND_VEHICLE,
                title = "Fleet car $i",
                area = "north side",
                pricePxmr = 30_000_000_000L + i * 1_000_000_000L,
                latE7 = lat, lonE7 = lon,
                specs = JSONObject().apply {
                    put("make", "Fleet"); put("model", "Number $i"); put("year", 2020L)
                    put("gearbox", 2L); put("fuel", 1L); put("subtype", 1L)
                    put("features", JSONArray())
                },
                privateDetails = secret,
            )
        }

        for (o in listOf(car, room, bike, kayak, sparks) + extras) {
            Listings.put(context, o)
            val ok = runCatching { Listings.post(context, o.optString("id")) }
                .onFailure { println("LIST_FAIL post: ${it.message}") }
                .getOrDefault(false)
            if (!ok) check("posted ${o.optString("title")}", false)
        }
        val live = Listings.all(context).filter { it.optString("board").isNotBlank() }
        // Five nouns now, not two: a car, a room, a bicycle, a kayak and an
        // electrician, all on one board.
        check("every listing found a slot", live.size == 5 + crowd,
            "${live.size} of ${2 + crowd} placed")
        val shards = live.map { it.optString("board") }.toSet()
        check("the ladder was used when the first board filled",
            crowd < 7 || shards.size > 1,
            shards.sorted().joinToString(", "))

        // The claim §16.18 makes about itself: the bytes on the board carry
        // nothing that would get a stranger to the door.
        // Sealed for a slot, because a notice is signed for the one it goes
        // into — any slot will do here, since what is being checked is what
        // the bytes contain rather than where they land.
        val persona = org.ducatproject.ducat.PersonaStore(context).secret()
        val onBoard = Listings.all(context).mapNotNull { o ->
            runCatching {
                uniffi.ducat_mobile.rentalEncode(
                    Listings.publicNotice(o, o.optString("card")),
                    persona, o.optString("id"), "geo:u33dc", 0u,
                    // A pinned block: this test asks what the *bytes* contain,
                    // and reaching for a chain tip would make it need a node.
                    3_210_000uL, "5a".repeat(32),
                )
            }.getOrNull()
        }.joinToString("\n") { it.decodeToString() }
        check("the address is not in what goes on the board", !onBoard.contains("Rosenthaler"))
        check("nor the key handover", !onBoard.contains("keys in the cafe"))
        check("but the searchable shape is", onBoard.contains("Corolla"), "make and model")

        // A live notice is re-posted every few hours, and each posting mints a
        // fresh card because a card is claimed once. The card it replaces must
        // not be forgotten: somebody who read the board an hour ago is still
        // holding it, and their enquiry has to arrive knowing what it is about.
        Listings.all(context).firstOrNull { it.optString("title").contains("Corolla") }
            ?.let { before ->
                val replaced = before.optString("card")
                runCatching { Listings.post(context, before.optString("id")) }
                val after = Listings.get(context, before.optString("id"))
                val arr = after?.optJSONArray("cards")
                val remembered = (0 until (arr?.length() ?: 0)).map { arr!!.getString(it) }
                check(
                    "a re-post keeps the card it replaced", remembered.contains(replaced),
                    "${remembered.size} card(s) remembered",
                )
                check(
                    "and the notice now carries the new one",
                    after?.optString("card") == remembered.lastOrNull() &&
                        after?.optString("card") != replaced,
                )
            }

        println("LIST_POSTED — leave this running while the seeker looks")
        // Boards are DHT records; stay up so they stay reachable.
        //
        // And watch for somebody answering one of the cards. A rental card is
        // cut per posting and claimed once, so the owner can say which listing
        // a stranger is asking about without being told — which is the half of
        // §16.18 that no amount of reading the board proves.
        val known = mutableSetOf<String>()
        var refreshedAt = System.currentTimeMillis()
        while (true) {
            Thread.sleep(10_000)
            runCatching { org.ducatproject.ducat.Mailbox.collectClaims(context) }
            runCatching { Listings.linkClaims(context) }
            org.ducatproject.ducat.ContactStore(context).all().forEach { c ->
                if (!known.add(c.personaHex)) return@forEach
                val about = org.ducatproject.ducat.Enquiries.about(context, c.personaHex)
                println(
                    if (about == null) {
                        "LIST FAIL a claim arrived with no listing attached — ${c.displayName()}"
                    } else {
                        "LIST ok   the owner knows which listing was claimed — " +
                            "${about.title} at ${formatXmr(about.pricePxmr)}"
                    },
                )
            }
            if (System.currentTimeMillis() - refreshedAt >= 30_000) {
                refreshedAt = System.currentTimeMillis()
                Listings.needRefresh(context).forEach {
                    runCatching { Listings.post(context, it.optString("id")) }
                }
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
            val n = runCatching { uniffi.ducat_mobile.standRead(org.ducatproject.ducat.standNow("rent:$cell")) }
                .getOrDefault(emptyList()).size
            println("LIST_BOARD rent:$cell $n notice(s) in ${System.currentTimeMillis() - t0} ms")
        }
    }

    // One search for the whole board, which is what the screen does now: five
    // nouns share a board, so asking per noun would pay the read five times
    // and an empty board is a flat twenty-one seconds each.
    var found: List<uniffi.ducat_mobile.RentalInfo> = emptyList()
    val t0 = System.currentTimeMillis()
    runCatching {
        Listings.search(context, lat, lon, null, onFound = { sofar ->
            if (sofar.size != found.size) {
                println("LIST_PARTIAL found=${sofar.size} after ${System.currentTimeMillis() - t0} ms")
            }
            found = sofar
        })
    }
    fun of(kind: Int) = found.filter { it.kind.toInt() == kind }
    // The owner's own first, for the same reason as the titles below.
    fun ownFirst(kind: Int, title: String) =
        of(kind).sortedBy { if (it.title == title) 0 else 1 }
    val cars = ownFirst(Listings.KIND_VEHICLE, "2019 Corolla, automatic")
    val rooms = ownFirst(Listings.KIND_PLACE, "Sunny room near the park")

    // The three nouns added in 0.89, off a real board rather than a unit
    // test: posted by one client, read back by another that never saw the
    // draft.
    // The owner's own, by title, when the board holds more than one of a
    // noun: a board keeps notices for a day, and the spot is a fixed one, so
    // a run meets whatever the last run — or an emulator — left there. The
    // first sale on the board was somebody else's bicycle with a different
    // category, and this read that as a lost tag.
    listOf(
        Triple(Listings.KIND_SALE, "for sale", 4) to "Bicycle, barely ridden",
        Triple(Listings.KIND_GEAR, "gear", 3) to "Sea kayak, paddle included",
        Triple(Listings.KIND_SKILL, "a skill", 1) to "Electrician, 20 years",
    ).forEach { (spec, title) ->
        val (kind, what, subtype) = spec
        val hit = of(kind).firstOrNull { it.title == title } ?: of(kind).firstOrNull()
        check("found $what on the board", hit != null, of(kind).size.toString())
        hit?.let {
            check(
                "  and $what kept its category and tags",
                it.subtype?.toInt() == subtype && it.features.isNotEmpty(),
                "subtype=${it.subtype} features=${it.features}",
            )
            check(
                "  and $what carries no typed extras",
                it.rooms == null && it.make == null && it.gearbox == null,
            )
            check("  and $what never carried the address", "Rosenthaler" !in it.toString())
        }
    }

    val want = System.getenv("DUCAT_LIST_CROWD")?.toIntOrNull() ?: 0
    check("found a car on the boards", cars.isNotEmpty(), cars.firstOrNull()?.title ?: "")
    if (want > 0) {
        // The point of the ladder: nothing is invisible because a board was
        // full when it was posted.
        check("every car on the ladder was found", cars.size >= want + 1,
            "${cars.size} of ${want + 1}")
        val fleet = cars.mapNotNull { it.model }.filter { it.startsWith("Number ") }.toSet()
        check("no fleet car is missing", fleet.size >= want, "${fleet.size} of $want distinct")
    }
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
    // The last step of the seeker's arc: turn a notice into a conversation.
    // The owner's side of this (which listing was that?) is asserted in the
    // owner's own loop above, because only that process can answer it.
    if (cars.isNotEmpty()) {
        // Every car, not the first one. A board holds notices for a day, so a
        // run against a long-lived board meets postings from hours ago whose
        // one card somebody has already used — which says nothing about
        // whether claiming works, only that this notice is spent.
        val claimed = cars.asSequence().mapNotNull { c ->
            runCatching {
                val card = uniffi.ducat_mobile.readContactCard(c.card)
                c.card to org.ducatproject.ducat.Mailbox.claimCard(context, card, null)
            }.getOrNull()
        }.firstOrNull()
        if (claimed != null) {
            val (uri, contact) = claimed
            check("the card turns into a conversation", contact.personaHex.isNotEmpty(),
                contact.displayName())
            // The same card again, from the same seeker — a second tap on the
            // listing. The reply in its slot is this seeker's own, so the
            // answer is the thread already opened, named, and not a stranger
            // who "asked just before you".
            val again = runCatching {
                org.ducatproject.ducat.Mailbox.claimCard(
                    context, uniffi.ducat_mobile.readContactCard(uri), null,
                )
            }.exceptionOrNull()
            check(
                "claiming it twice names the thread it made",
                (again as? org.ducatproject.ducat.Mailbox.CardAlreadyMine)
                    ?.contact?.personaHex == contact.personaHex,
                again?.toString() ?: "claimed twice",
            )
        } else {
            println("LIST skip  every card on the board was already claimed")
        }
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
