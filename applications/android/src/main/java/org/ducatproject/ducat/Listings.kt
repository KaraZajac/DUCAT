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
        val board = boardName(cell)
        // Eight slots to a board; take the first that is free or ours.
        val now = System.currentTimeMillis() / 1000
        val taken = runCatching { uniffi.ducat_mobile.standRead(board) }.getOrDefault(emptyList())
            .mapNotNull { n ->
                runCatching { uniffi.ducat_mobile.rentalDecode(n.data) }.getOrNull()
                    ?.takeIf { it.expiry.toLong() > now }?.let { n.subkey }
            }.toSet()
        val mine = o.optInt("subkey", -1).takeIf { it >= 0 }?.toUInt()
        val slot = mine ?: (0u..7u).firstOrNull { it !in taken }
        if (slot == null) {
            DucatLog.w(TAG, "board $board is full")
            return false
        }
        uniffi.ducat_mobile.standPost(board, slot, bytes)
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

        fun absorb(cell: String) {
            val fresh = readBoard(cell, kind)
            synchronized(found) {
                // One card, one listing: a notice copied onto two boards is
                // still one thing being rented.
                fresh.filter { seen.add(it.card) }.forEach { found += it }
                onFound(found.toList())
            }
        }

        // Where you are, first and alone: the answer people usually want.
        absorb(home)

        val ring = runCatching { uniffi.ducat_mobile.geohashNeighbors(home) }
            .getOrDefault(emptyList())
        if (ring.isEmpty()) return
        val pool = java.util.concurrent.Executors.newFixedThreadPool(ring.size.coerceAtMost(8))
        try {
            ring.map { cell -> pool.submit { runCatching { absorb(cell) } } }
                .forEach { runCatching { it.get(120, java.util.concurrent.TimeUnit.SECONDS) } }
        } finally {
            pool.shutdownNow()
        }
    }

    /** Everything live on one board, already filtered and decoded. */
    private fun readBoard(cell: String, kind: Int?): List<uniffi.ducat_mobile.RentalInfo> {
        val now = System.currentTimeMillis() / 1000
        return runCatching { uniffi.ducat_mobile.standRead(boardName(cell)) }
            .getOrDefault(emptyList())
            .mapNotNull { runCatching { uniffi.ducat_mobile.rentalDecode(it.data) }.getOrNull() }
            // A reader MUST drop an expired listing unrendered (§16.18).
            .filter { it.expiry.toLong() > now }
            .filter { kind == null || it.kind.toInt() == kind }
    }

    /** Rental boards are their own namespace: a hail must never collide. */
    private fun boardName(cell: String) = "rent:$cell"

    const val KIND_PLACE = 1
    const val KIND_VEHICLE = 2
}
