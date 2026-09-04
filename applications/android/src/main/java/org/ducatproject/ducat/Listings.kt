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

    /**
     * How long a listing that wants a board and has none waits between tries.
     *
     * The screen's failure sentences promise the retry — "it will go up on
     * its own once the network is reachable", "goes up by itself when one
     * frees" — and [needRefresh] only ever knew about listings already on a
     * board, so neither promise was kept: a first post that met no node, or
     * a full board, left the listing saved and never tried again. A try is
     * seconds of Argon2 and a ladder walk that reads sixteen boards at
     * twenty-one seconds an empty one, so not every poll.
     */
    const val RETRY_SECONDS = 30L * 60

    /** The board a listing sits on: coarse by rule (§16.18). */
    const val CELL_PRECISION = 5u

    /**
     * How long a search waits for the ring of boards before it settles for
     * what has arrived. Nothing is lost by stopping — every board publishes
     * as it lands — so this is the cap on how long a person can be asked to
     * watch a spinner, not a cap on how much can be found.
     */
    private const val WAVE_BUDGET_MS = 120_000L

    /** Shards read at once when climbing a full board's ladder — see readCell. */
    private const val LADDER_RUNG = 3u

    /**
     * What each cell last showed, so the next search has something to draw
     * before the network answers. Process-lifetime, and short: see the paint
     * in [search] for why it is safe to be a little stale and why it is never
     * the last word.
     *
     * Keyed by kind as well as cell, because what a board "showed" is already
     * filtered — [readShard] drops every notice of another kind before the
     * caller sees it. Keyed by cell alone, leaving Marketplace for Renting
     * would have painted the bicycles you were just offered into a list of
     * rooms, for as long as it took the real read to land.
     */
    private val cellCache =
        java.util.concurrent.ConcurrentHashMap<
            String, Pair<Long, List<uniffi.ducat_mobile.RentalInfo>>,
            >()

    private fun cacheKey(cell: String, kind: Int?) = "$cell|${kind ?: -1}"

    // How old a remembered board may be and still paint. Three minutes when
    // the cache lived and died with the process; six hours now that it
    // survives a relaunch, because that is the re-post cadence — a notice
    // older than its board's own refresh clock is stale twice over. The
    // window governs only the first paint: every board is still read, every
    // answer still replaces what was painted, and a notice past its own
    // expiry is dropped at paint time whatever the cache thinks.
    private const val CACHE_TTL_MS = 6 * 60 * 60_000L

    // The remembered boards, on disk: what makes "open Marketplace, see the
    // neighbourhood" instant on the first open after a relaunch, not just
    // the second in a session. Plain prefs, not securePrefs — everything in
    // here was read off a public board.
    private const val CACHE_PREFS = "ducat_board_cache"
    private const val CACHE_KEEP = 48
    @Volatile private var cacheLoaded = false

    private fun rowToJson(r: uniffi.ducat_mobile.RentalInfo) = JSONObject().apply {
        put("poster", r.poster); put("card", r.card)
        put("kind", r.kind.toLong()); put("title", r.title); put("area", r.area)
        r.cell?.let { put("cell", it) }
        put("price", r.pricePxmr.toString()); put("deposit", r.depositPxmr.toString())
        put("expiry", r.expiry.toLong())
        r.make?.let { put("make", it) }; r.model?.let { put("model", it) }
        r.year?.let { put("year", it.toLong()) }
        r.gearbox?.let { put("gearbox", it.toLong()) }
        r.fuel?.let { put("fuel", it.toLong()) }
        r.seats?.let { put("seats", it.toLong()) }
        r.color?.let { put("color", it) }; r.trim?.let { put("trim", it) }
        r.rooms?.let { put("rooms", it.toLong()) }
        r.sleeps?.let { put("sleeps", it.toLong()) }
        r.sizeM2?.let { put("size_m2", it.toLong()) }
        r.subtype?.let { put("subtype", it.toLong()) }
        put("features", org.json.JSONArray(r.features))
        put("quantity", r.quantity.toLong())
        // The picture too: this cache is what the browse screen draws from
        // between sweeps, and a row that forgot its thumbnail would go grey
        // every time the list was rebuilt from disk.
        r.thumb?.let {
            put("thumb", android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP))
        }
        r.galleryShare?.let { put("gallery", it) }
        r.galleryDigest?.let { put("gallery_dig", it) }
    }

    private fun rowFromJson(o: JSONObject): uniffi.ducat_mobile.RentalInfo? = runCatching {
        uniffi.ducat_mobile.RentalInfo(
            poster = o.getString("poster"),
            card = o.getString("card"),
            kind = o.getLong("kind").toULong(),
            title = o.getString("title"),
            area = o.optString("area"),
            cell = if (o.has("cell")) o.getString("cell") else null,
            pricePxmr = o.getString("price").toULong(),
            depositPxmr = o.getString("deposit").toULong(),
            expiry = o.getLong("expiry").toULong(),
            make = if (o.has("make")) o.getString("make") else null,
            model = if (o.has("model")) o.getString("model") else null,
            year = if (o.has("year")) o.getLong("year").toULong() else null,
            gearbox = if (o.has("gearbox")) o.getLong("gearbox").toULong() else null,
            fuel = if (o.has("fuel")) o.getLong("fuel").toULong() else null,
            seats = if (o.has("seats")) o.getLong("seats").toULong() else null,
            color = if (o.has("color")) o.getString("color") else null,
            trim = if (o.has("trim")) o.getString("trim") else null,
            rooms = if (o.has("rooms")) o.getLong("rooms").toULong() else null,
            sleeps = if (o.has("sleeps")) o.getLong("sleeps").toULong() else null,
            sizeM2 = if (o.has("size_m2")) o.getLong("size_m2").toULong() else null,
            subtype = if (o.has("subtype")) o.getLong("subtype").toULong() else null,
            features = o.optJSONArray("features")?.let { a ->
                (0 until a.length()).map { a.getString(it) }
            } ?: emptyList(),
            quantity = o.optLong("quantity", 1).toULong(),
            thumb = thumbOf(o),
            galleryShare = o.optString("gallery").takeIf { it.isNotBlank() },
            galleryDigest = o.optString("gallery_dig").takeIf { it.isNotBlank() },
        )
    }.getOrNull()

    private fun loadCellCache(context: Context) {
        if (cacheLoaded) return
        synchronized(cellCache) {
            if (cacheLoaded) return
            cacheLoaded = true
            runCatching {
                val raw = context.getSharedPreferences(CACHE_PREFS, 0)
                    .getString("cells", null) ?: return
                val all = JSONObject(raw)
                for (key in all.keys()) {
                    val e = all.optJSONObject(key) ?: continue
                    val rows = e.optJSONArray("rows") ?: continue
                    val list = (0 until rows.length())
                        .mapNotNull { i -> rows.optJSONObject(i)?.let(::rowFromJson) }
                    cellCache.putIfAbsent(key, e.optLong("at") to list)
                }
            }.onFailure { DucatLog.w(TAG, "board cache load: ${it.message}") }
        }
    }

    private fun saveCellCache(context: Context) {
        runCatching {
            val keep = cellCache.entries
                .sortedByDescending { it.value.first }
                .take(CACHE_KEEP)
            val all = JSONObject()
            for ((key, v) in keep) {
                all.put(
                    key,
                    JSONObject()
                        .put("at", v.first)
                        .put("rows", org.json.JSONArray(v.second.map(::rowToJson))),
                )
            }
            context.getSharedPreferences(CACHE_PREFS, 0)
                .edit().putString("cells", all.toString()).apply()
        }.onFailure { DucatLog.w(TAG, "board cache save: ${it.message}") }
    }

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

    /**
     * Guards read-change-write of the listing set.
     *
     * `linkClaims` runs on the poller — matching answered cards to listings
     * and minting a replacement card for each — while a screen posts, edits or
     * deletes a listing. Both rewrite the whole array. The write that loses
     * takes a card with it, and a listing whose card was dropped is one nobody
     * can enquire about until it is posted again.
     *
     * (The two locks already in this file guard the *search*, which is a
     * different thing: many boards read in parallel into one result set.)
     */
    private val lock = Any()

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

    fun put(context: Context, o: JSONObject) = synchronized(lock) {
        val id = o.optString("id")
        // Replaced where it stands, appended only when new. `all` returns
        // stored order and the listings screen does not sort, so rebuilding
        // the table as "everything else, then this" moved the row to the
        // bottom every time it changed. Seen on the phone: two listings with
        // the same title swapping places under a quantity stepper, so three
        // taps on "one more" raised two different listings and neither of
        // them by three.
        val cur = all(context)
        save(
            context,
            if (cur.none { it.optString("id") == id }) {
                cur + o
            } else {
                cur.map { if (it.optString("id") == id) o else it }
            },
        )
    }

    /** What [post] and [unpost] own on a record: where it is, and the cards it cut. */
    private val TENANCY = listOf("owner", "board", "subkey", "postedAt", "card", "cards", "wanted", "triedAt")

    /**
     * Save a draft over whatever record already carries its id, keeping the
     * record's tenancy.
     *
     * The form re-posts the same draft after a failure — one listing per
     * form — and a rotation mid-post cancels the screen's coroutine while
     * the post it started runs on to the end. The second press then arrived
     * with a fresh draft over a record that had just taken a slot: [put]
     * dropped the board, the slot and the cards, the post walked the ladder
     * to a *second* slot, and the first notice sat on as a ghost holding a
     * card nothing would ever link.
     */
    fun putDraft(context: Context, draft: JSONObject) = synchronized(lock) {
        get(context, draft.optString("id"))?.let { cur ->
            for (k in TENANCY) if (cur.has(k) && !draft.has(k)) draft.put(k, cur.get(k))
        }
        put(context, draft)
    }

    /**
     * One post per listing at a time. The poll's refresh, the owner's Post
     * and a quantity tap can all ask for the same listing in the same
     * breath; only the final write was serialised, so two walks of the
     * ladder took two slots and the record remembered one of them.
     */
    private val postLocks = java.util.concurrent.ConcurrentHashMap<String, Any>()

    fun remove(context: Context, id: String) {
        // The pictures go with it, and the seeding stops. A listing taken
        // down whose share kept serving would be a picture on the network
        // for a thing nobody can buy, with nothing left pointing at it.
        stopGallery(context, id)
        photoDir(context, id).deleteRecursively()
        synchronized(lock) { save(context, all(context).filter { it.optString("id") != id }) }
    }

    // --- §16.18.3's gallery ------------------------------------------------
    //
    // Full-size pictures live here from the moment they are picked, and are
    // seeded from here — never from a staging directory that is about to
    // move, which leaves the seeder serving a path that no longer exists
    // (Releases.share learned that one the hard way).

    /** How many photographs one listing may carry.
     *
     *  Not a protocol bound — the swarm does not care — but a browser
     *  fetching a gallery is waiting on it, and twenty pictures is a
     *  minute of somebody's patience for a bicycle. */
    const val MAX_PHOTOS = 8

    /** Where one listing's full-size pictures live on this phone. */
    fun photoDir(context: Context, listingId: String): java.io.File =
        java.io.File(java.io.File(context.filesDir, "listing_photos"), safeId(listingId))

    /** An id that cannot walk out of its own directory. */
    private fun safeId(id: String): String =
        id.filter { it.isLetterOrDigit() || it == '-' || it == '_' }.take(64)
            .ifBlank { "unnamed" }

    fun photos(context: Context, listingId: String): List<java.io.File> =
        photoDir(context, listingId).listFiles()?.filter { it.isFile }?.sortedBy { it.name }
            ?: emptyList()

    /**
     * Put this listing's pictures on the swarm, and keep serving them.
     *
     * Returns share key and index digest, or null when there is nothing to
     * seed. Idempotent by design: seeding an unchanged directory returns the
     * same share, so a refresh does not mint a second one.
     */
    fun seedGallery(context: Context, listingId: String): Pair<String, String>? {
        val dir = photoDir(context, listingId)
        if (!dir.isDirectory || dir.listFiles().orEmpty().none { it.isFile }) return null
        return runCatching {
            val share = Swarm.seed(dir.absolutePath)
            // Narrated here rather than at the call site, because there are
            // two: posting mints it, and the lap re-parks it after a node
            // restart. A silent re-park is indistinguishable from one that
            // never happened, which is the whole failure being guarded.
            //
            // In full, not truncated: this pair is public — it rides the
            // board where anybody can read it — and a half-printed share
            // key is no use to whoever is reading the log to find out why
            // a gallery will not arrive.
            DucatLog.i(
                "Listings",
                "gallery of ${listingId.take(8)}… serving at ${share.shareKey} " +
                    "digest ${share.indexDigestHex}",
            )
            share.shareKey to share.indexDigestHex
        }.onFailure {
            DucatLog.w("Listings", "gallery of ${listingId.take(8)}…: ${it.message}")
        }.getOrNull()
    }

    /** Stop serving this listing's pictures. */
    fun stopGallery(context: Context, listingId: String) {
        val share = get(context, listingId)?.optString("gallery")?.takeIf { it.isNotBlank() }
            ?: return
        runCatching { Swarm.stopShare(share) }
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
        /** What the owner typed, and in what, when they priced in their own
         *  currency. Kept so [reprice] can hold that price steady. */
        priceTyped: String? = null,
        priceCurrency: String? = null,
        /** How many of it there are. One unless the owner said otherwise. */
        quantity: Long = 1,
        /** §16.18.3's inline picture, already shrunk to fit the board by
         *  [SafeImage.thumbnail]. Stored base64 beside the words, because a
         *  listing that loses its picture on a rotation is one nobody
         *  re-photographs. */
        thumb: ByteArray? = null,
    ): JSONObject {
        val cell = runCatching {
            uniffi.ducat_mobile.geohashEncode(latE7, lonE7, CELL_PRECISION)
        }.getOrNull()
        val deal = dealFor(kind)
        return JSONObject().apply {
            put("id", java.util.UUID.randomUUID().toString())
            put("kind", kind)
            // Straight from a text field, and headed for a public board where
            // every reader's core will refuse it if it carries an override.
            put("title", withoutDisplayHazards(title))
            put("area", withoutDisplayHazards(area))
            put("cell", cell ?: "")
            put("pricePxmr", pricePxmr)
            // Suggested from the same table the escrow will use, so what the
            // board advertises and what the booking asks for cannot disagree.
            put("depositPxmr", Stakes.stakeFor(deal, pricePxmr))
            put("specs", specs)
            put("private", privateDetails)
            // Stored even when it is one, so the owner's counter has something
            // to count down from without a migration the first time they sell.
            put("quantity", quantity.coerceIn(1L, MAX_QUANTITY))
            thumb?.let {
                put("thumb", android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP))
            }
            put("created", System.currentTimeMillis() / 1000)
            // The doorway decision: this listing — its cards, its notice
            // signatures, every enquiry it opens — belongs to the persona
            // worn when it was written, forever.
            put("owner", PersonaStore(context).worn())
            if (priceTyped != null && priceCurrency != null) {
                put("priceTyped", priceTyped)
                put("priceCurrency", priceCurrency)
            }
        }
    }

    /** Change how many are left. Clamped to what the wire will take. */
    fun setQuantity(context: Context, id: String, n: Long) = synchronized(lock) {
        val items = all(context).map { o ->
            if (o.optString("id") == id) {
                JSONObject(o.toString()).put("quantity", n.coerceIn(1L, MAX_QUANTITY))
            } else o
        }
        save(context, items)
        ContactStore.bump()
    }

    /**
     * How many of this listing there are, defaulting to one.
     *
     * One place, so a listing written before the field existed and a listing
     * whose owner never touched the number are the same thing, and neither
     * has to be migrated. Clamped to what the wire will take, because a stored
     * value that core refuses is a listing nobody could post.
     */
    fun quantityOf(o: JSONObject): Long =
        o.optLong("quantity", 1L).coerceIn(1L, MAX_QUANTITY)

    /** A shop with six kayaks, not a warehouse — core's own ceiling. */
    const val MAX_QUANTITY = 999L

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
        // Three-way, not two. `!vehicle` used to mean "a place", which was
        // true while a board held two nouns and would now put bedrooms on a
        // kayak — core refuses that, so a listing that reached the board
        // would be one nobody could read back.
        val vehicle = kind == KIND_VEHICLE
        val place = kind == KIND_PLACE
        return uniffi.ducat_mobile.RentalInfo(
            // Derived by the encoder from the listing's own signing key; a
            // value set here would be a claim rather than a fact.
            poster = "",
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
            rooms = if (place) num("rooms") else null,
            sleeps = if (place) num("sleeps") else null,
            sizeM2 = if (place) num("size_m2") else null,
            subtype = num("subtype"),
            features = features,
            // How many are left, not how many there ever were: the owner's
            // own screen counts down as they sell, and this is read at every
            // refresh so the board follows. A skill cannot carry one at all.
            quantity = (if (kind == KIND_SKILL) 1L else quantityOf(o)).toULong(),
            // The picture rides the notice (§16.18.3). Already within the
            // board's budget when it was stored — core refuses an oversized
            // one, and the poster is the only person who could fix it.
            thumb = thumbOf(o),
            // The gallery does not: the notice carries only where to find
            // it. Both halves or neither, so a half-written pair never
            // reaches core.
            galleryShare = o.optString("gallery").takeIf { it.isNotBlank() }
                ?.takeIf { o.optString("gallery_dig").isNotBlank() },
            galleryDigest = o.optString("gallery_dig").takeIf { it.isNotBlank() }
                ?.takeIf { o.optString("gallery").isNotBlank() },
        )
    }

    /** A stored listing's inline picture, or null. Refused rather than
     *  published if it will not fit: a notice one byte over the cap is
     *  refused by every reader, and this is the last place that can say so. */
    fun thumbOf(o: JSONObject): ByteArray? {
        val raw = o.optString("thumb").takeIf { it.isNotBlank() } ?: return null
        val bytes = runCatching {
            android.util.Base64.decode(raw, android.util.Base64.NO_WRAP)
        }.getOrNull() ?: return null
        if (bytes.isEmpty() || bytes.size > SafeImage.THUMB_BYTES) {
            DucatLog.w("Listings", "dropping a ${bytes.size}-byte thumbnail: over the board's cap")
            return null
        }
        return bytes
    }

    /**
     * Put a listing on its board, minting a fresh claim-once card for it.
     *
     * The card is what turns a reader into a conversation, and it is
     * per-posting rather than per-listing: a card is claimed once, so a
     * listing that has been enquired about needs a new one before the next
     * stranger can reach its owner.
     */
    /**
     * Bring a listing's piconero back in line with the price its owner set.
     *
     * A listing is a *standing* price. The wire carries piconero and nothing
     * else (§16.18), so a room posted at forty euros is forty euros' worth of
     * monero on the day it went up — and a fortnight later, when the rate has
     * moved, it is not forty euros any more. The owner did not change their
     * price; the board changed it for them, and neither end was told.
     *
     * The till has always had the answer for this: a catalogue keeps what was
     * typed and what it was typed in, and converts at the moment of the sale.
     * A listing can do the same thing at the moment of the refresh, which
     * comes round every six hours, so what a stranger reads is never more than
     * a few hours away from what the owner meant.
     *
     * Two cases are deliberately left alone. A listing priced in XMR to begin
     * with has nothing to hold steady — that is the price. And a phone whose
     * rate store has since been switched to some other currency has no rate
     * for the one this listing was written in, so the last good conversion
     * stands rather than being replaced by an invented one.
     */
    fun reprice(context: Context, o: JSONObject): JSONObject {
        val typed = o.optString("priceTyped", "").takeIf { it.isNotBlank() } ?: return o
        val cur = o.optString("priceCurrency", "").takeIf { it.isNotBlank() } ?: return o
        val store = RateStore(context)
        if (!store.enabled() || !store.currency().equals(cur, ignoreCase = true)) return o
        val rate = store.cached()?.first?.takeIf { it > 0 } ?: return o
        val v = Amounts.parse(typed) ?: return o
        val pxmr = Amounts.toPxmr(
            v.divide(java.math.BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN),
        )?.takeIf { it > 0 } ?: return o
        if (pxmr == o.optLong("pricePxmr")) return o
        val next = JSONObject(o.toString()).apply {
            put("pricePxmr", pxmr)
            put("depositPxmr", Stakes.stakeFor(dealFor(o.optInt("kind")), pxmr))
        }
        put(context, next)
        DucatLog.i(
            TAG,
            "${o.optString("title")}: $typed $cur is now ${formatXmr(pxmr)} XMR",
        )
        return next
    }

    fun post(context: Context, id: String): Boolean =
        synchronized(postLocks.getOrPut(id) { Any() }) { postLocked(context, id) }

    private fun postLocked(context: Context, id: String): Boolean {
        val now = System.currentTimeMillis() / 1000
        // Asked for a board, and asked now — written before anything can
        // fail, so a listing that meets no node or a full board is one the
        // poll keeps trying (needRefresh), which is what the screen promised.
        synchronized(lock) {
            get(context, id)?.let { it.put("wanted", true); it.put("triedAt", now); put(context, it) }
        }
        val o = reprice(context, get(context, id) ?: return false)
        // The pictures go on the network before the notice that names them.
        // The other order publishes an address for a share nobody is
        // serving yet, and a reader who arrives in that window is told the
        // listing has photographs and then cannot fetch them.
        if (o.optString("gallery").isBlank()) {
            seedGallery(context, id)?.let { (share, digest) ->
                synchronized(lock) {
                    get(context, id)?.let {
                        it.put("gallery", share)
                        it.put("gallery_dig", digest)
                        put(context, it)
                    }
                }
                o.put("gallery", share)
                o.put("gallery_dig", digest)
            }
        }
        val cell = o.optString("cell")
        if (cell.isBlank()) throw IllegalStateException("this listing has no area yet")
        val personas = PersonaStore(context)
        // The stored owner wins; a pre-persona listing adopts the primary
        // and has it written down from here on.
        val ownerHex = o.optString("owner").ifBlank {
            personas.personaHex().also { hex -> o.put("owner", hex); put(context, o) }
        }
        val card = Mailbox.issueCard(
            context, MyProfile(context).name(), TTL_SECONDS.toULong(), purpose = "rental",
            asPersonaHex = ownerHex,
        )
        val notice = publicNotice(o, card.uri)
        val persona = personas.secretFor(ownerHex) ?: personas.secret()

        // A notice is signed for the slot it goes into and carries the proof of
        // work for that slot, so the bytes cannot be built until the slot is
        // chosen — and have to be rebuilt for the next candidate if a slot is
        // lost to a race. That is the shape the defence requires: bytes that
        // were good for any slot could be sprayed across all of them.
        //
        // §16.18.1: the block this stamp perishes with. Taken once, before
        // the ladder starts, so every candidate slot is stamped against the
        // same block — the search has to be redone per slot, and re-reading
        // the tip between attempts would only make the earlier ones staler.
        //
        // No node, no post. There is nothing honest to put here, and a beacon
        // the poster invents is exactly the precomputation the field exists to
        // stop; a listing that went up unstamped would also be refused by
        // every reader, which is a worse way to find out.
        val beacon = Beacons.stampNow(context)
            ?: throw Beacons.NoBlock()

        // Seconds of Argon2, on the poll thread. See board.rs.
        fun seal(board: String, slot: UInt): ByteArray =
            uniffi.ducat_mobile.rentalEncode(
                notice, persona, id, board, slot,
                beacon.height.toULong(), beacon.hashHex,
            )

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
        // A tenancy on last week's board is not a tenancy. Keeping the slot is
        // the right instinct — a second copy leaves the first as a ghost — but
        // only while anybody is still reading the board it is on; past a
        // rollover the whole ladder has moved and the notice has to move with
        // it, which is the ordinary walk below.
        //
        // Nor is a tenancy whose notice has run out. A slot holding an
        // expired notice reads as free to every other writer (the occupancy
        // test below, on their phone), so a device that was off for a day
        // and refreshed "in place" wrote over whoever had honestly taken it
        // in the meantime — their notice gone, and nothing on their side to
        // say so. An hour short of the TTL, for their clock against ours.
        val existing = o.optString("board")
            .takeIf { it.isNotBlank() && !standStale(it) && stillHeld(o, now) }
        val existingSlot = o.optInt("subkey", -1).takeIf { it >= 0 }?.toUInt()
        if (existing != null && existingSlot != null) {
            // Refreshing in place: keep the tenancy rather than taking a
            // second slot and leaving the first to expire as a ghost.
            if (runCatching { uniffi.ducat_mobile.standPost(existing, existingSlot, seal(existing, existingSlot)) }
                    .isSuccess
            ) {
                placed = existing to existingSlot
            }
        }
        if (placed == null) {
            ladder@ for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
                val name = uniffi.ducat_mobile.standShardName(standNow(boardName(cell)), shard)
                val taken = runCatching { uniffi.ducat_mobile.standRead(name) }
                    .getOrDefault(emptyList())
                    // **Occupancy, not display — and deliberately the looser
                    // test.** This decides which slots are already spoken for
                    // before this device writes. A notice whose block cannot
                    // be confirmed *yet* is probably an honest poster whose
                    // node is a little ahead of ours, and overwriting it would
                    // do the damage the confirmation exists to prevent. So a
                    // slot counts as taken on the signature, the work and the
                    // window; only what is shown to somebody has to be
                    // confirmed.
                    .mapNotNull { n ->
                        runCatching {
                            uniffi.ducat_mobile.rentalDecode(
                                n.data, name, n.subkey, Beacons.tip(context).toULong(),
                            )
                        }.getOrNull()
                            ?.takeIf { it.expiry.toLong() > now }?.let { n.subkey }
                    }.toSet()
                for (free in 0u..7u) {
                    if (free in taken) continue
                    // standPost verifies its own landing, so a slot two
                    // writers raced for is a throw here rather than a notice
                    // that quietly vanished under someone else's.
                    if (runCatching { uniffi.ducat_mobile.standPost(name, free, seal(name, free)) }
                            .isSuccess
                    ) {
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
        //
        // Onto the record as it is *now*, not the copy read before the
        // seconds of Argon2 and the board walk: a price or a title edited
        // in that gap was being written back over by the stale copy, and a
        // listing taken down in it came back.
        synchronized(lock) {
            val cur = get(context, id)
            if (cur == null) {
                runCatching { uniffi.ducat_mobile.standPost(board, slot, ByteArray(0)) }
                DucatLog.i(TAG, "listing $id was removed while posting; slot cleared")
                return false
            }
            cur.put("card", card.uri)
            // Every card this listing has ever put on a board, not just the
            // one currently on it. A live notice is re-posted every few hours
            // and each posting mints a fresh card (a card is claimed once), so
            // somebody who read the board before the last refresh is holding
            // a card this device would otherwise have forgotten — and their
            // enquiry would arrive with no idea what it was about. Caught in
            // exactly that state on 2026-08-19.
            val minted = cur.optJSONArray("cards") ?: org.json.JSONArray()
            minted.put(card.uri)
            // A day's TTL over a six-hour refresh is four cards; the slack is
            // for re-posts after a failure, and anything older cannot still
            // be on a board to be read.
            while (minted.length() > 8) minted.remove(0)
            cur.put("cards", minted)
            cur.put("board", board)
            cur.put("subkey", slot.toInt())
            cur.put("postedAt", now)
            put(context, cur)
        }
        DucatLog.i(TAG, "listing ${o.optString("title")} posted to $board/$slot")
        return true
    }

    /**
     * Is the notice this record says it posted still the one in that slot?
     *
     * Only by the clock: a notice is stamped to live [TTL_SECONDS], and past
     * that every other writer treats the slot as free. Anything the slot
     * holds after the notice ran out may be somebody else's, so neither a
     * refresh nor a take-down may touch it.
     */
    private fun stillHeld(o: JSONObject, now: Long): Boolean =
        now - o.optLong("postedAt") < TTL_SECONDS - 3600

    /** Take it down: clear the slot, forget the tenancy. */
    fun unpost(context: Context, id: String) {
        val o = get(context, id) ?: return
        val board = o.optString("board")
        val slot = o.optInt("subkey", -1)
        // Only a slot that is still ours. Past the TTL it may hold a
        // stranger's notice (see stillHeld), and a stale board is one nobody
        // reads — clearing either is a write that can only do harm.
        if (board.isNotBlank() && slot >= 0 && !standStale(board) &&
            stillHeld(o, System.currentTimeMillis() / 1000)
        ) {
            runCatching { uniffi.ducat_mobile.standPost(board, slot.toUInt(), ByteArray(0)) }
                .onFailure { DucatLog.w(TAG, "clearing slot: ${it.message}") }
        }
        // Re-read before writing. `o` is from before the board write above,
        // and `put` writes the whole listing back — so a price edit, a
        // re-post stamp or a quantity change made while that round trip was
        // out was silently reverted by taking the listing down.
        val fresh = get(context, id) ?: o
        fresh.remove("board"); fresh.remove("subkey")
        fresh.remove("postedAt"); fresh.remove("card")
        // Taken down is the owner's word: the poll stops trying to put it up.
        fresh.remove("wanted"); fresh.remove("triedAt")
        put(context, fresh)
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
            // By card, not by contact. "Has this person ever asked about
            // anything" is the wrong question: a neighbour who bought
            // something last week and is now asking about a different listing
            // was skipped, and their enquiry arrived with the old subject
            // still on it.
            .filter { !Enquiries.linked(context, it.uri) }
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
            Enquiries.markLinked(context, issued.uri)
            Enquiries.remember(
                context, issued.answeredBy!!,
                Enquiries.About(
                    title = o.optString("title"),
                    pricePxmr = o.optLong("pricePxmr"),
                    depositPxmr = o.optLong("depositPxmr"),
                    kind = o.optInt("kind"),
                    listingId = o.optString("id"),
                ),
            )
        }
    }

    /**
     * Live listings whose notice is old enough to be worth re-posting — or
     * whose board has stopped being read.
     *
     * The second clause is §15.12's generation. Boards rotate weekly, and at
     * a rollover a perfectly good notice is still sitting on last week's
     * board with hours of TTL left, where nobody is looking any more. Waiting
     * out the ordinary refresh would leave it invisible for up to six hours;
     * asking whether the board is stale costs a string compare and closes
     * that to one poll.
     */
    fun needRefresh(context: Context): List<JSONObject> {
        val now = System.currentTimeMillis() / 1000
        return all(context).filter {
            val board = it.optString("board")
            if (board.isBlank()) {
                // Wanted and not up: the owner pressed Post and it did not
                // take — no node, or a full board. Spaced by RETRY_SECONDS,
                // because a try is not cheap and the board does not free
                // between polls.
                it.optBoolean("wanted") &&
                    Elapsed.dueSecs(now, it.optLong("triedAt"), RETRY_SECONDS)
            } else {
                // [Elapsed] rather than the subtraction, and this is the site
                // that matters most: a stamp written while the clock was
                // ahead never comes of age, so the notice is never re-posted,
                // falls off the board when its day runs out, and the owner's
                // screen goes on saying "Live on the board near you" for ever
                // — the exact failure the refresh was added to prevent.
                Elapsed.dueSecs(now, it.optLong("postedAt"), REFRESH_SECONDS) ||
                    standStale(board)
            }
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
        // §16.18.1: a board read judges each notice's beacon against this
        // device's own view of the chain, and a view needs a device.
        context: Context,
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
        loadCellCache(context)
        // See browseMarket: an unattached read finds nothing honestly enough
        // to display, but writing that nothing over remembered rows poisons
        // the cache every airplane-mode open.
        val attachedAtStart = runCatching {
            uniffi.ducat_mobile.nodeStatus().publicInternetReady
        }.getOrDefault(false)
        val replied = java.util.concurrent.atomic.AtomicInteger()
        val home = runCatching {
            uniffi.ducat_mobile.geohashEncode(latE7, lonE7, CELL_PRECISION)
        }.getOrNull() ?: return 0
        // What each board last said, rather than one growing pile.
        //
        // A pile could only be added to, so a board answering a second time —
        // which is exactly what the ladder wave does, and what painting from
        // the cache below does — could never take anything off the screen. A
        // listing taken down stayed up. Keyed by cell and replaced wholesale,
        // the second answer corrects the first.
        //
        // Insertion order is board order, and home is read first, so what is
        // underfoot still comes first in the list.
        val byCell = LinkedHashMap<String, List<uniffi.ducat_mobile.RentalInfo>>()

        fun publish() {
            val merged = LinkedHashMap<String, uniffi.ducat_mobile.RentalInfo>()
            synchronized(byCell) {
                // One card, one listing: a notice copied onto two boards is
                // still one thing being rented.
                byCell.values.forEach { list -> list.forEach { merged.putIfAbsent(it.card, it) } }
            }
            onFound(merged.values.toList())
        }

        fun absorb(cell: String, fresh: List<uniffi.ducat_mobile.RentalInfo>) {
            synchronized(byCell) { byCell[cell] = fresh }
            if (fresh.isNotEmpty() || attachedAtStart) {
                cellCache[cacheKey(cell, kind)] = System.currentTimeMillis() to fresh
                saveCellCache(context)
            }
            publish()
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
                    readCell(
                        context, cell, kind, false,
                        onSlots = { slots -> if (slots >= 8) full += cell },
                    )
                }.getOrNull()
                if (got != null) {
                    absorb(cell, got)
                    replied.incrementAndGet()
                }
                onProgress(answered.incrementAndGet(), boards.size)
            }

            // What these boards said last time, on screen before the first
            // read returns.
            //
            // Nothing about this is fast: a board with nothing on it costs two
            // chained ten-second timeouts inside veilid, nine of them overlap
            // to about forty-eight seconds, and none of that is ours to fix
            // from here. What was ours to fix is paying it again from an empty
            // screen every single time — switching Marketplace to Renting and
            // back, or pressing "try again", re-asked the network questions it
            // had answered a minute ago while the person watched a spinner.
            //
            // Only ever a first paint. Every cached cell is still read, and
            // its answer replaces what was painted, so a listing taken down in
            // the meantime survives on screen until its own board reports
            // back rather than until the cache ages out. Notices past their
            // own expiry are dropped here rather than shown: the cache can be
            // older than the thing it remembers.
            val nowSecs = System.currentTimeMillis() / 1000
            val warmed = boards.count { cell ->
                val hit = cellCache[cacheKey(cell, kind)]
                    ?.takeIf { System.currentTimeMillis() - it.first < CACHE_TTL_MS }
                    ?.second
                    ?.filter { it.expiry.toLong() > nowSecs }
                if (hit.isNullOrEmpty()) false
                else { synchronized(byCell) { byCell[cell] = hit }; true }
            }
            if (warmed > 0) {
                DucatLog.i(TAG, "painting from $warmed remembered board(s) while we look")
                publish()
            }
            onProgress(0, boards.size)
            // Home alone and first, still. Submitting all nine together was
            // tried and measured: same total to the second, because what
            // bounds concurrency is inside veilid-core (see node.rs) and nine
            // at once merely puts home in the queue with the rest. The barrier
            // costs nothing it was not already going to cost, and it buys the
            // one thing a searcher notices — what is underfoot, on screen
            // first.
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
                // A pool of its own for the rungs, because these tasks wait on
                // theirs — see readCell's `climb`.
                val climbPool = java.util.concurrent.Executors.newFixedThreadPool(
                    (crowded.size * LADDER_RUNG.toInt()).coerceIn(1, 12),
                )
                try {
                    waitAll(
                        crowded.map { cell ->
                            pool.submit {
                                runCatching { readCell(context, cell, kind, true, climb = climbPool) }
                                    .getOrNull()?.let { absorb(cell, it) }
                            }
                        },
                        LADDER_BUDGET_MS,
                    )
                } finally {
                    climbPool.shutdownNow()
                }
            }
        } finally {
            pool.shutdownNow()
            DucatLog.i(
                TAG,
                "search near $home: " +
                    "${synchronized(byCell) { byCell.values.flatten().map { it.card }.toSet().size }}" +
                    " listing(s) from " +
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
        context: Context,
        name: String,
        kind: Int?,
        /** How many slots the board held, before expiry and kind filtered
         *  them — which is what says whether the ladder needs climbing. */
        onSlots: (Int) -> Unit = {},
    ): List<uniffi.ducat_mobile.RentalInfo>? {
        val now = System.currentTimeMillis() / 1000
        val ttlCap = runCatching { uniffi.ducat_mobile.maxNoticeTtlSecs().toLong() }
            .getOrDefault(31L * 24 * 60 * 60)
        val raw = runCatching { uniffi.ducat_mobile.standRead(name) }.getOrNull() ?: return null
        onSlots(raw.size)
        val tip = Beacons.tip(context).toULong()
        val budget = Beacons.budget()
        return raw.mapNotNull {
            // The slot is inside the signature, so it is an argument here and
            // not a detail: a notice that verifies for slot 3 is refused at
            // slot 4, which is what stops one signed listing papering a cell.
            runCatching {
                uniffi.ducat_mobile.rentalDecode(it.data, name, it.subkey, tip)
            }.getOrNull()
                // And the half that costs a lookup. **This is a display path,
                // so nothing shows without a confirmed block.** The height
                // test above is free and forgeable on its own: Monero's
                // two-minute blocks make a height months away predictable to
                // within a few hundred, so an attacker mines a spread of
                // future heights against hashes they invented and every
                // height-only reader takes them. Unknown is held, not shown —
                // it becomes knowable within minutes, and a listing is not so
                // urgent that it is worth showing one nobody has checked.
                ?.takeIf { n ->
                    tip == 0uL ||
                        Beacons.confirm(context, n.beaconHeight.toLong(), n.beaconHash, budget) ==
                        Beacons.Verdict.CONFIRMED
                }
        }
            // A reader MUST drop an expired listing unrendered (§16.18) — and
            // one dated past the ceiling too. board.rs prices flooding by
            // making each slot cost a search, and that price only recurs
            // because notices expire; a notice good until 2100 turns one
            // payment into a permanent squat, and no honest client will ever
            // clear it because clearing somebody else's slot is refused.
            .filter { it.expiry.toLong() > now && it.expiry.toLong() <= now + ttlCap }
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
        context: Context,
        cell: String,
        kind: Int?,
        deep: Boolean,
        onSlots: (Int) -> Unit = {},
        /**
         * Threads for the climb, when there is one. Its own pool, never the
         * caller's: a deep read runs *inside* a task on the search pool, and
         * submitting the rung back into that same pool would have every
         * thread in it waiting on work queued behind itself.
         */
        climb: java.util.concurrent.ExecutorService? = null,
    ): List<uniffi.ducat_mobile.RentalInfo>? {
        val base = standNow(boardName(cell))
        // Null, not empty. readShard is careful to tell "nobody has posted
        // here" from "we could not ask", and this used to flatten the two a
        // line later — so a search run before the node had finished attaching
        // read nothing, failed at every board, and told the person with total
        // confidence that there was nothing listed near them.
        val first = readShard(context, base, kind, onSlots) ?: return null
        if (!deep) return first
        val out = first.toMutableList()
        val top = runCatching { uniffi.ducat_mobile.maxStandShards() }.getOrDefault(1u)

        // A rung at a time, not a rung by rung by rung.
        //
        // The climb used to read shard 1, wait, read shard 2, wait, and stop
        // after two empties — so proving a ladder had no more on it cost two
        // whole board reads end to end, and an empty board read is a flat
        // twenty-one seconds however little is on it. Forty seconds, tacked
        // onto the end of a search, to establish nothing.
        //
        // Shards fill from the bottom (see `post`), so a ladder is dense: the
        // question is only where it stops. Reading three at once answers that
        // in one round trip instead of two, and looks one shard further up
        // than the old rule did before giving up.
        var shard = 1u
        while (shard < top) {
            val rung = (shard until minOf(shard + LADDER_RUNG, top)).toList()
            val reads = rung.map { s ->
                val name = runCatching { uniffi.ducat_mobile.standShardName(base, s) }
                    .getOrNull()
                when {
                    name == null -> null
                    climb == null -> java.util.concurrent.CompletableFuture
                        .completedFuture(readShard(context, name, kind))
                    else -> climb.submit<List<uniffi.ducat_mobile.RentalInfo>?> {
                        readShard(context, name, kind)
                    }
                }
            }
            var anything = false
            reads.forEach { f ->
                val live = f?.let { runCatching { it.get() }.getOrNull() }
                if (live != null && live.isNotEmpty()) {
                    anything = true
                    out += live
                }
            }
            if (!anything) break
            shard += LADDER_RUNG
        }
        return out
    }

    /** Rental boards are their own namespace: a hail must never collide. */
    /**
     * One board per cell, whatever is on it.
     *
     * Was `rent:` when a board held two nouns. A board name is a DHT record
     * to find, an empty one costs a flat 21 seconds, and boards do not
     * parallelise — so a name per noun would multiply the one cost that
     * decides whether looking around is bearable. The kind is a field on the
     * notice; the reader filters after one read rather than paying for five.
     */
    private fun boardName(cell: String) = "local:$cell"

    const val KIND_PLACE = 1
    const val KIND_VEHICLE = 2

    /** A thing sold outright: a bicycle, a sofa, a drill. Nothing returns. */
    const val KIND_SALE = 3

    /** Equipment by the day: a kayak, skis, a pressure washer. */
    const val KIND_GEAR = 4

    /** Somebody's time by the hour: an electrician, a plumber, an afternoon
     *  of help moving a sofa. */
    const val KIND_SKILL = 5

    /** Every kind a board carries, in the order a person meets them. */
    val KINDS = listOf(KIND_PLACE, KIND_VEHICLE, KIND_GEAR, KIND_SALE, KIND_SKILL)

    /**
     * The three jobs the five nouns divide into (§16.18).
     *
     * One board carries all of them — that is a transport detail — but the
     * *work* is three different jobs and each is a mode. Somebody with a room
     * and a kayak to let is doing one job with two nouns in it, which is why
     * renting keeps its chips and the other two have nothing to switch
     * between.
     */
    val RENT_KINDS = listOf(KIND_PLACE, KIND_VEHICLE, KIND_GEAR)
    val SALE_KINDS = listOf(KIND_SALE)
    val SKILL_KINDS = listOf(KIND_SKILL)

    /**
     * The stake table each kind draws from.
     *
     * A deposit and a stake are the same money and different words: on
     * anything returned the deposit comes back with the thing, and on a sale
     * there is nothing to return so the pair of stakes is the whole reason to
     * turn up. See §16.18.
     */
    fun dealFor(kind: Int): Stakes.Deal = when (kind) {
        KIND_VEHICLE, KIND_GEAR -> Stakes.Deal.Vehicle
        KIND_SALE -> Stakes.Deal.Sale
        KIND_SKILL -> Stakes.Deal.Labour
        else -> Stakes.Deal.Stay
    }

    /**
     * How many top-level categories a kind recognises — core's own table,
     * mirrored so a form cannot offer one the wire will refuse.
     */
    fun subtypeTop(kind: Int): Int = when (kind) {
        KIND_PLACE -> 2
        KIND_VEHICLE -> 3
        KIND_SALE -> 9
        KIND_GEAR -> 5
        KIND_SKILL -> 12
        else -> 0
    }

    /** True for the kinds that carry no typed extras (§16.18). */
    fun isPlain(kind: Int): Boolean = kind > KIND_VEHICLE
}
