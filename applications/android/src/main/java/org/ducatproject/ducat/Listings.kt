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
     * board that *has* listings answers in about 18 seconds, and a board
     * that is empty takes 10 to 80, because the network has to satisfy
     * itself the record is not there. Nine of those in a row is five minutes
     * of spinner for a result that was mostly ready in the first twenty
     * seconds.
     *
     * So the home cell is read first and its results handed over
     * immediately, and the ring around it is read in parallel afterwards —
     * the wait becomes the slowest single board rather than the sum of nine,
     * and the common case (what is right here) is on screen while the rest
     * arrives. `onFound` is called with everything found so far, each time
     * more is.
     */
    fun search(
        latE7: Long,
        lonE7: Long,
        kind: Int?,
        onFound: (List<uniffi.ducat_mobile.RentalInfo>) -> Unit,
    ) {
        val home = runCatching {
            uniffi.ducat_mobile.geohashEncode(latE7, lonE7, CELL_PRECISION)
        }.getOrNull() ?: return
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
            // Wave one: where you are *and* the ring around it, shard 0, all
            // at once — each board drawn as it answers.
            //
            // The home cell used to be read alone and first, on the reasoning
            // that it is the answer people usually want. True where somebody
            // is listing something; false everywhere else, and "everywhere
            // else" is what a quiet neighbourhood is. An empty board takes 51
            // to 85 seconds to come back empty — concluding a record is *not*
            // there costs more than finding one — so a blocking first read
            // bought nothing and delayed the other eight by a minute and a
            // half before they had even started. A populated home cell still
            // arrives first: it answers in 12 to 18 seconds and `absorb`
            // publishes on arrival, not in order.
            (listOf(home) + ring)
                .map { cell -> pool.submit { runCatching { absorb(readCell(cell, kind, false)) } } }
                .forEach { runCatching { it.get(150, java.util.concurrent.TimeUnit.SECONDS) } }

            // Wave three: climb the ladder, but only where shard 0 came back
            // full — a board with a free slot is the end of its own ladder,
            // because every writer fills the lowest free slot first.
            val crowded = (listOf(home) + ring).filter { looksFull(boardName(it)) }
            if (crowded.isNotEmpty()) {
                DucatLog.i(TAG, "climbing the ladder on ${crowded.size} full board(s)")
                crowded.map { cell -> pool.submit { runCatching { absorb(readCell(cell, kind, true)) } } }
                    .forEach { runCatching { it.get(240, java.util.concurrent.TimeUnit.SECONDS) } }
            }
        } finally {
            pool.shutdownNow()
        }
    }

    /**
     * Everything live on one shard, already filtered and decoded.
     *
     * Returns null when the read itself failed, which is not the same as an
     * empty board: a network that could not answer must not be mistaken for
     * the end of a ladder.
     */
    private fun readShard(name: String, kind: Int?): List<uniffi.ducat_mobile.RentalInfo>? {
        val now = System.currentTimeMillis() / 1000
        val raw = runCatching { uniffi.ducat_mobile.standRead(name) }.getOrNull() ?: return null
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
    ): List<uniffi.ducat_mobile.RentalInfo> {
        val base = boardName(cell)
        val first = readShard(base, kind) ?: return emptyList()
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

    /** A board is full when every one of its eight slots holds something live. */
    private fun looksFull(name: String): Boolean =
        (runCatching { uniffi.ducat_mobile.standRead(name) }.getOrNull()?.size ?: 0) >= 8

    /** Rental boards are their own namespace: a hail must never collide. */
    private fun boardName(cell: String) = "rent:$cell"

    const val KIND_PLACE = 1
    const val KIND_VEHICLE = 2
}
