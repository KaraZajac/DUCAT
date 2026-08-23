package org.ducatproject.ducat

import android.content.Context

/**
 * Who you have seen on a board before.
 *
 * A stand's write key is the cell name hashed, so nobody owns a slot and
 * anybody can overwrite anybody. What a signed notice adds is that the *bytes*
 * have an author — and the attack that matters is substitution: copy a good
 * listing, swap in your own card, post it. Content alone cannot tell the two
 * apart, because the content is the same. The author is the difference.
 *
 * So this keeps the one fact a reader has no other way to know — that a
 * listing's author is somebody whose listings have been on this board for
 * weeks, or somebody who turned up this afternoon. Neither is proof of
 * anything. A long-standing poster can still be a fraud and a new one is
 * usually just new. It is the same signal as a shop that has been on the
 * corner for years: worth knowing, never conclusive, and the app says it that
 * way round.
 *
 * The key is per listing (see `board::listing_seed`), not a persona, so this
 * store links a listing to its own past and to nothing else — not to the
 * poster's other listings, and not to anybody's contacts.
 */
object Posters {

    private const val PREFS = "board_posters"

    /** Below this, a poster is still "new" — long enough to outlast a refresh
     *  cycle, short enough that an honest listing stops being flagged the day
     *  after it goes up. */
    const val SETTLED_MS = 3L * 24 * 60 * 60 * 1000

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /**
     * Record having seen a poster, and say when they were first seen.
     *
     * First sighting returns `now`, which reads as "new" — correctly, since
     * that is all this device knows.
     */
    fun seen(context: Context, posterHex: String, now: Long): Long {
        if (posterHex.isBlank()) return now
        val p = prefs(context)
        val first = p.getLong(posterHex, 0L)
        if (first != 0L) return first
        p.edit().putLong(posterHex, now).apply()
        return now
    }

    /** How long this poster has been known, in millis; 0 for a stranger. */
    fun knownFor(context: Context, posterHex: String, now: Long): Long =
        if (posterHex.isBlank()) 0L
        else prefs(context).getLong(posterHex, 0L).takeIf { it != 0L }
            ?.let { (now - it).coerceAtLeast(0L) } ?: 0L

    /** Has this author been around long enough to be worth mentioning? */
    fun settled(context: Context, posterHex: String, now: Long): Boolean =
        knownFor(context, posterHex, now) >= SETTLED_MS

    /**
     * Forget posters not seen for a long time.
     *
     * A listing's key dies with the listing, so without this the store is a
     * list of every notice this phone ever scrolled past — which grows without
     * end and, kept long enough, is a record of where its owner has been
     * browsing. Ninety days is well past any listing's life.
     */
    fun sweep(context: Context, now: Long): Int {
        val cutoff = now - 90L * 24 * 60 * 60 * 1000
        val p = prefs(context)
        val stale = p.all.filterValues { it is Long && it < cutoff }.keys
        if (stale.isEmpty()) return 0
        p.edit().apply { stale.forEach { remove(it) } }.apply()
        return stale.size
    }
}
