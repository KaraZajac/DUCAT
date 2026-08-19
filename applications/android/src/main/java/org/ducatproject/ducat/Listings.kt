package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * What this device is renting out, and where it is advertised (§16.18).
 *
 * A listing has two halves and they live in different places on purpose.
 *
 * **The public half** goes on a geocell board, where anyone within a few
 * kilometres can read it: what the thing is, roughly where, what it costs,
 * what each side stakes, and a claim-once card to start a conversation.
 *
 * **The private half never leaves this device until its owner sends it** —
 * the address, the plate, where the keys are, the door code. It is stored
 * here beside the listing so that "share the details" is one tap once a
 * booking is real, and so that no code path can put it on a board by
 * accident: the board only ever sees [publicNotice].
 *
 * Boards are coarse (precision 5, ~5 km) and notices expire, so a listing is
 * refreshed while it is live and simply stops being refreshed when it is
 * not. Nothing needs a withdrawal message that a hostile writer could forge.
 */
object Listings {
    private const val TAG = "DucatListings"

    /** How long a posted notice claims to be good for. */
    const val TTL_SECONDS = 24L * 60 * 60

    /** Re-post this often while the listing is live, so a board stays true. */
    const val REFRESH_SECONDS = 6L * 60 * 60

    /** The board a listing sits on: coarse by rule (§16.18). */
    const val CELL_PRECISION = 5u

    /**
     * How long a search waits for the ring of boards before it settles for
     * what has arrived. Nothing is lost by stopping — every board publishes
     * as it lands — so this is the cap on how long a person can be asked to
     * watch a spinner, not a cap on how much can be found.
     */
    private const val WAVE_BUDGET_MS = 120_000L

    /** The same, for the second pass over boards that came back full. */
    private const val LADDER_BUDGET_MS = 90_000L

    /** Wait for all of them, but no longer than the budget for all of them. */
    private fun waitAll(jobs: List<java.util.concurrent.Future<*>>, budgetMs: Long) {
        val deadline = System.currentTimeMillis() + budgetMs
        jobs.forEach { job ->
            val left = deadline - System.currentTimeMillis()
            if (left <= 0) return@forEach
            runCatching { job.get(left, java.util.concurrent.TimeUnit.MILLISECONDS) }
        }
    }

    private fun prefs(context: Context) = securePrefs(context, "ducat_listings")

    fun all(context: Context): List<JSONObject> {
        val raw = prefs(context).getString("listings", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return emptyList()
        return (0 until arr.length()).map { arr.getJSONObject(it) }
    }

    fun get(context: Context, id: String): JSONObject? =
        all(context).firstOrNull { it.optString("id") == id }

    private fun save(context: Context, items: List<JSONObject>) {
        prefs(context).edit().putString("listings", JSONArray(items).toString()).apply()
        ContactStore.bump()
    }

    fun put(context: Context, o: JSONObject) {
        val id = o.optString("id")
        save(context, all(context).filter { it.optString("id") != id } + o)
    }

    fun remove(context: Context, id: String) {
        save(context, all(context).filter { it.optString("id") != id })
    }

    /**
     * A new listing, unposted. `private` holds the half that never goes on a
     * board; everything else is what a stranger will see.
     */
    fun draft(
        context: Context,
        kind: Int,
        title: String,
        area: String,
        pricePxmr: Long,
        latE7: Long,
        lonE7: Long,
        specs: JSONObject,
        privateDetails: String,
    ): JSONObject {
        val cell = runCatching {
            uniffi.ducat_mobile.geohashEncode(latE7, lonE7, CELL_PRECISION)
        }.getOrNull()
        val deal = if (kind == KIND_VEHICLE) Stakes.Deal.Vehicle else Stakes.Deal.Stay
        return JSONObject().apply {
            put("id", java.util.UUID.randomUUID().toString())
            put("kind", kind)
            put("title", title)
            put("area", area)
            put("cell", cell ?: "")
            put("pricePxmr", pricePxmr)
            // Suggested from the same table the escrow will use, so what the
            // board advertises and what the booking asks for cannot disagree.
            put("depositPxmr", Stakes.stakeFor(deal, pricePxmr))
            put("specs", specs)
            put("private", privateDetails)
            put("created", System.currentTimeMillis() / 1000)
        }
    }

    /**
     * The public half, as the wire object (§16.18).
     *
     * The only path from a listing to a board. `private` is not read here and
     * cannot be: this function does not take it.
     */
    fun publicNotice(o: JSONObject, card: String): uniffi.ducat_mobile.RentalInfo {
        val specs = o.optJSONObject("specs") ?: JSONObject()
        val kind = o.optInt("kind")
        fun txt(k: String): String? = specs.optString(k, "").takeIf { it.isNotBlank() }
        fun num(k: String): ULong? = specs.optLong(k, 0L).takeIf { it > 0L }?.toULong()
        val features = specs.optJSONArray("features")?.let { a ->
            (0 until a.length()).map { a.getString(it) }
        } ?: emptyList()
        val vehicle = kind == KIND_VEHICLE
        return uniffi.ducat_mobile.RentalInfo(
            card = card,
            kind = kind.toULong(),
            title = o.optString("title"),
            area = o.optString("area"),
            cell = o.optString("cell").takeIf { it.isNotBlank() },
            pricePxmr = o.optLong("pricePxmr").toULong(),
            depositPxmr = o.optLong("depositPxmr").toULong(),
            expiry = (System.currentTimeMillis() / 1000 + TTL_SECONDS).toULong(),
            // A place has no gearbox and a car has no bedrooms — core refuses
            // the mismatch, so the split is enforced here rather than hoped for.
            make = if (vehicle) txt("make") else null,
            model = if (vehicle) txt("model") else null,
            year = if (vehicle) num("year") else null,
            gearbox = if (vehicle) num("gearbox") else null,
            fuel = if (vehicle) num("fuel") else null,
            seats = if (vehicle) num("seats") else null,
            color = if (vehicle) txt("color") else null,
            trim = if (vehicle) txt("trim") else null,
            rooms = if (!vehicle) num("rooms") else null,
            sleeps = if (!vehicle) num("sleeps") else null,
            sizeM2 = if (!vehicle) num("size_m2") else null,
            subtype = num("subtype"),
            features = features,
        )
    }

    /**
     * Put a listing on its board, minting a fresh claim-once card for it.
     *
     * The card is what turns a reader into a conversation, and it is
     * per-posting rather than per-listing: a card is claimed once, so a
     * listing that has been enquired about needs a new one before the next
     * stranger can reach its owner.
     */
    fun post(context: Context, id: String): Boolean {
        val o = get(context, id) ?: return false
        val cell = o.optString("cell")
        if (cell.isBlank()) throw IllegalStateException("this listing has no area yet")
        val card = Mailbox.issueCard(
            context, MyProfile(context).name(), TTL_SECONDS.toULong(), purpose = "rental",
        )
        val bytes = uniffi.ducat_mobile.rentalEncode(publicNotice(o, card.uri))
        val now = System.currentTimeMillis() / 1000

        // §15.12's overflow ladder, which listings need far more than hails
        // do: a hail is one person for ten minutes, but a five-kilometre
        // cell in a city holds every car and room in a neighbourhood at
        // once. Eight slots to a board, sixteen boards to a cell — shard 0
        // is the bare name so existing boards stay valid, and 1.. are
        // "<name>-<n>".
        //
        // Always from the bottom: writers filling the lowest free slot is
        // what lets a reader stop climbing, and what keeps the ladder short
        // enough to read.
        var placed: Pair<String, UInt>? = null
        val existing = o.optString("board").takeIf { it.isNotBlank() }
        val existingSlot = o.optInt("subkey", -1).takeIf { it >= 0 }?.toUInt()
        if (existing != null && existingSlot != null) {
            // Refreshing in place: keep the tenancy rather than taking a
            // second slot and leaving the first to expire as a ghost.
            if (runCatching { uniffi.ducat_mobile.standPost(existing, existingSlot, bytes) }
                    .isSuccess
            ) {
                placed = existing to existingSlot
            }
        }
        if (placed == null) {
            ladder@ for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
                val name = uniffi.ducat_mobile.standShardName(boardName(cell), shard)
                val taken = runCatching { uniffi.ducat_mobile.standRead(name) }
                    .getOrDefault(emptyList())
                    .mapNotNull { n ->
                        runCatching { uniffi.ducat_mobile.rentalDecode(n.data) }.getOrNull()
                            ?.takeIf { it.expiry.toLong() > now }?.let { n.subkey }
                    }.toSet()
                for (free in 0u..7u) {
                    if (free in taken) continue
                    // standPost verifies its own landing, so a slot two
                    // writers raced for is a throw here rather than a notice
                    // that quietly vanished under someone else's.
                    if (runCatching { uniffi.ducat_mobile.standPost(name, free, bytes) }.isSuccess) {
                        placed = name to free
                        break@ladder
                    }
                }
            }
        }
        val (board, slot) = placed ?: run {
            DucatLog.w(TAG, "every shard of ${boardName(cell)} is full")
            return false
        }
        // Remember the tenancy, or the notice is on the network and this
        // device has no idea where: a refresh would post a second copy and
        // "take it down" would have nothing to clear.
        o.put("card", card.uri)
        // Every card this listing has ever put on a board, not just the one
        // currently on it. A live notice is re-posted every few hours and each
        // posting mints a fresh card (a card is claimed once), so somebody who
        // read the board before the last refresh is holding a card this device
        // would otherwise have forgotten — and their enquiry would arrive with
        // no idea what it was about. Caught in exactly that state on 2026-08-19.
        val minted = o.optJSONArray("cards") ?: org.json.JSONArray()
        minted.put(card.uri)
        // A day's TTL over a six-hour refresh is four cards; the slack is for
        // re-posts after a failure, and anything older cannot still be on a
        // board to be read.
        while (minted.length() > 8) minted.remove(0)
        o.put("cards", minted)
        o.put("board", board)
        o.put("subkey", slot.toInt())
        o.put("postedAt", now)
        put(context, o)
        DucatLog.i(TAG, "listing ${o.optString("title")} posted to $board/$slot")
        return true
    }

    /** Take it down: clear the slot, forget the tenancy. */
    fun unpost(context: Context, id: String) {
        val o = get(context, id) ?: return
        val board = o.optString("board")
        val slot = o.optInt("subkey", -1)
        if (board.isNotBlank() && slot >= 0) {
            runCatching { uniffi.ducat_mobile.standPost(board, slot.toUInt(), ByteArray(0)) }
                .onFailure { DucatLog.w(TAG, "clearing slot: ${it.message}") }
        }
        o.remove("board"); o.remove("subkey"); o.remove("postedAt"); o.remove("card")
        put(context, o)
    }

    /**
     * Tie each answered rental card back to the listing it was cut for.
     *
     * A card is minted per posting and claimed once (see [post]), so the
     * stranger who answered one is asking about exactly one thing — and the
     * owner's side is the only side that can work that out, because the
     * seeker's claim says nothing about which notice they read. Recorded once
     * per contact and never revised.
     *
     * Called after claims are collected rather than from inside the mailbox,
     * which has no business knowing that renting exists.
     */
    fun linkClaims(context: Context) {
        // Only the ones not already tied to something: this runs on every
        // poll, and reading back every listing to re-decide a question that
        // was settled days ago is work with no answer at the end of it.
        val answered = ContactStore(context).issuedCards()
            .filter { it.purpose == "rental" && it.answeredBy != null }
            .filter { Enquiries.about(context, it.answeredBy!!) == null }
        if (answered.isEmpty()) return
        val byCard = HashMap<String, JSONObject>()
        all(context).forEach { o ->
            o.optString("card").takeIf { it.isNotBlank() }?.let { byCard[it] = o }
            o.optJSONArray("cards")?.let { arr ->
                (0 until arr.length()).forEach { i ->
                    runCatching { byCard[arr.getString(i)] = o }
                }
            }
        }
        answered.forEach { issued ->
            val o = byCard[issued.uri] ?: return@forEach
            // The card on the board has just been used up — they are claimed
            // once — so put a fresh one out at once. Left alone, the notice
            // stays readable but unreachable until the next refresh hours
            // later, and the next person to tap Ask about it is told the card
            // has already been used and to ask the owner for a new one, which
            // is both untrue and impossible: asking is the thing they cannot
            // do. One listing, one enquiry, per six hours was never the deal.
            if (o.optString("card") == issued.uri && o.optString("board").isNotBlank()) {
                runCatching { post(context, o.optString("id")) }
                    .onSuccess {
                        DucatLog.i(TAG, "${o.optString("title")}: fresh card after an enquiry")
                    }
                    .onFailure { DucatLog.w(TAG, "re-post after enquiry: ${it.message}") }
            }
            Enquiries.remember(
                context, issued.answeredBy!!,
                Enquiries.About(
                    title = o.optString("title"),
                    pricePxmr = o.optLong("pricePxmr"),
                    depositPxmr = o.optLong("depositPxmr"),
                    kind = o.optInt("kind"),
                ),
            )
        }
    }

    /** Live listings whose notice is old enough to be worth re-posting. */
    fun needRefresh(context: Context): List<JSONObject> {
        val now = System.currentTimeMillis() / 1000
        return all(context).filter {
            it.optString("board").isNotBlank() &&
                now - it.optLong("postedAt") >= REFRESH_SECONDS
        }
    }

    /**
     * Read every listing on the boards around a place, answering as it goes.
     *
     * Measured on the live network, and the numbers decided the shape: a
     * board somebody has posted to answers in 1.1 s, and an empty one takes
     * 21.0 — flat, to the millisecond, across eight different empty boards,
     * because that is the network giving up rather than searching.
     *
     * So the home cell is read first and alone, and its results handed over
     * immediately: nearly free when it finds something, bounded when it does
     * not, and it is where a person is most likely to be renting. The ring is
     * read afterwards, and `onFound` is called with everything found so far
     * each time more of it lands, so what is right here is on screen while
     * the rest arrives.
     */
    fun search(
        latE7: Long,
        lonE7: Long,
        kind: Int?,
        onFound: (List<uniffi.ducat_mobile.RentalInfo>) -> Unit,
        /**
         * How many of the boards have answered, and how many there are.
         *
         * Concluding that a board is empty costs the better part of a minute
         * (see below), so a search of a quiet area is a minute and a half of
         * screen with nothing on it. A spinner that cannot say whether it is
         * halfway or stuck is indistinguishable from a hang, and people close
         * apps that look hung — so the count is reported as it goes.
         */
        onProgress: (Int, Int) -> Unit = { _, _ -> },
        /**
         * How many boards actually answered — which is not how many were
         * asked. Zero means the network could not be reached, and "nothing
         * listed around here" would be a confident lie.
         */
    ): Int {
        val started = System.currentTimeMillis()
        val replied = java.util.concurrent.atomic.AtomicInteger()
        val home = runCatching {
            uniffi.ducat_mobile.geohashEncode(latE7, lonE7, CELL_PRECISION)
        }.getOrNull() ?: return 0
        val seen = HashSet<String>()
        val found = mutableListOf<uniffi.ducat_mobile.RentalInfo>()

        fun absorb(fresh: List<uniffi.ducat_mobile.RentalInfo>) {
            synchronized(found) {
                // One card, one listing: a notice copied onto two boards is
                // still one thing being rented.
                fresh.filter { seen.add(it.card) }.forEach { found += it }
                onFound(found.toList())
            }
        }

        val ring = runCatching { uniffi.ducat_mobile.geohashNeighbors(home) }
            .getOrDefault(emptyList())
        val pool = java.util.concurrent.Executors.newFixedThreadPool(9)
        try {
            // Where you are, alone and first. Then the ring.
            //
            // This order has now been both ways round, because the cost of a
            // board read changed underneath it. Measured against the live
            // network: a board somebody has posted to answers in about a
            // second, and an empty one takes twenty-one — flat, every time,
            // because that is Veilid giving up rather than searching. And
            // nine reads issued at once still take about as long as nine
            // issued in turn, so whatever serialises them is below us and
            // more threads do not move it.
            //
            // Under those two facts the first read is nearly free when it
            // finds something and bounded when it does not, while queueing it
            // behind eight empty neighbours cost twenty to thirty seconds of
            // staring at a spinner with the answer already sitting on the
            // board underfoot. Your own neighbourhood is also where you are
            // most likely to be renting something. So: home, then the rest.
            val boards = listOf(home) + ring
            val answered = java.util.concurrent.atomic.AtomicInteger()
            // Which boards came back with every slot taken, noted while
            // reading them. This used to be a second read of all nine
            // afterwards (`looksFull`), asking the network a question the
            // first pass had already answered.
            val full = java.util.Collections.synchronizedSet(HashSet<String>())
            fun read(cell: String) {
                val got = runCatching {
                    readCell(cell, kind, false) { slots -> if (slots >= 8) full += cell }
                }.getOrNull()
                if (got != null) {
                    absorb(got)
                    replied.incrementAndGet()
                }
                onProgress(answered.incrementAndGet(), boards.size)
            }
            onProgress(0, boards.size)
            read(home)
            val jobs = ring.map { cell -> pool.submit { read(cell) } }
            // One deadline for the wave, not one per board.
            //
            // Waiting `get(150s)` on each future in turn reads like a 150
            // second cap and is not: a board that never answers spends the
            // whole 150 before the next is even looked at, so nine slow
            // boards could hold the screen for twenty-two minutes. The
            // deadline is wall-clock, and every board is already publishing
            // its findings through `absorb` as it lands, so cutting the wait
            // short loses the waiting, not the results.
            waitAll(jobs, WAVE_BUDGET_MS)

            // Wave three: climb the ladder, but only where shard 0 came back
            // full — a board with a free slot is the end of its own ladder,
            // because every writer fills the lowest free slot first.
            val crowded = boards.filter { it in full }
            if (crowded.isNotEmpty()) {
                DucatLog.i(TAG, "climbing the ladder on ${crowded.size} full board(s)")
                waitAll(
                    crowded.map { cell ->
                        pool.submit {
                            runCatching { readCell(cell, kind, true) }.getOrNull()
                                ?.let { absorb(it) }
                        }
                    },
                    LADDER_BUDGET_MS,
                )
            }
        } finally {
            pool.shutdownNow()
            DucatLog.i(
                TAG,
                "search near $home: ${found.size} listing(s) from " +
                    "${replied.get()} board(s) in " +
                    "${(System.currentTimeMillis() - started) / 1000}s",
            )
        }
        return replied.get()
    }

    /**
     * Everything live on one shard, already filtered and decoded.
     *
     * Returns null when the read itself failed, which is not the same as an
     * empty board: a network that could not answer must not be mistaken for
     * the end of a ladder.
     */
    private fun readShard(
        name: String,
        kind: Int?,
        /** How many slots the board held, before expiry and kind filtered
         *  them — which is what says whether the ladder needs climbing. */
        onSlots: (Int) -> Unit = {},
    ): List<uniffi.ducat_mobile.RentalInfo>? {
        val now = System.currentTimeMillis() / 1000
        val raw = runCatching { uniffi.ducat_mobile.standRead(name) }.getOrNull() ?: return null
        onSlots(raw.size)
        return raw.mapNotNull { runCatching { uniffi.ducat_mobile.rentalDecode(it.data) }.getOrNull() }
            // A reader MUST drop an expired listing unrendered (§16.18).
            .filter { it.expiry.toLong() > now }
            .filter { kind == null || it.kind.toInt() == kind }
    }

    /**
     * One cell's whole ladder, bottom up.
     *
     * The stopping rule is the hail's and for the hail's reason: a shard
     * that comes back empty is not proof the ladder ended, because expiry
     * empties low shards first and leaves gaps above them. Two empty shards
     * in a row is the signal, which costs a quiet cell one extra read and
     * costs a busy one nothing.
     *
     * `deep` is what keeps a search affordable. Reading sixteen shards of
     * nine cells is 144 board reads, and an empty board costs tens of
     * seconds — so the first pass reads only shard 0 everywhere, and the
     * ladder is climbed afterwards, only where shard 0 came back full.
     */
    private fun readCell(
        cell: String,
        kind: Int?,
        deep: Boolean,
        onSlots: (Int) -> Unit = {},
    ): List<uniffi.ducat_mobile.RentalInfo>? {
        val base = boardName(cell)
        // Null, not empty. readShard is careful to tell "nobody has posted
        // here" from "we could not ask", and this used to flatten the two a
        // line later — so a search run before the node had finished attaching
        // read nothing, failed at every board, and told the person with total
        // confidence that there was nothing listed near them.
        val first = readShard(base, kind, onSlots) ?: return null
        if (!deep) return first
        val out = first.toMutableList()
        var quiet = 0
        for (shard in 1u until uniffi.ducat_mobile.maxStandShards()) {
            val name = runCatching { uniffi.ducat_mobile.standShardName(base, shard) }
                .getOrNull() ?: break
            val live = readShard(name, kind) ?: break
            if (live.isEmpty()) {
                if (++quiet >= 2) break
            } else {
                quiet = 0
                out += live
            }
        }
        return out
    }

    /** Rental boards are their own namespace: a hail must never collide. */
    private fun boardName(cell: String) = "rent:$cell"

    const val KIND_PLACE = 1
    const val KIND_VEHICLE = 2
}
