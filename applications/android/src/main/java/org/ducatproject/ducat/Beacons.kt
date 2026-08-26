package org.ducatproject.ducat

import android.content.Context

/**
 * The chain's clock, as a board sees it (§16.18.1).
 *
 * **Why a board notice needs one at all.** Everything else in a stamp's
 * preimage is the poster's own — the cell, the slot, the body, the signature —
 * and a board name's generation is a floor division of the wall clock. So the
 * whole of next year was mineable in an afternoon: fifty-two epochs of every
 * slot of every cell in a region, then a year of posting at no marginal cost.
 * §15.12 rotates boards weekly on the understanding that re-poisoning is paid
 * for again each week, and that was not true of anybody who had done the
 * arithmetic. A recent block hash inside the work is what makes it true.
 *
 * **Two questions, and they cost very different amounts.** Is the height
 * plausible — free, against a tip this device already has. Does that height
 * really carry that hash — one node round trip, and the only one that actually
 * secures anything, since a beacon nobody looks up is thirty-two bytes the
 * attacker chose. So the height is tested on every notice and the hash is
 * looked up per *height*, cached: every honest poster in a cell stamps against
 * roughly the same block, so a sweep of eighteen boards asks about a handful.
 *
 * **Missing is not stale.** With no reachable node this returns nothing and
 * every caller carries on without the test — reading a board has never needed
 * Monero and a marketplace that goes dark because a daemon is down is a worse
 * answer than the spam it was avoiding. Posting is the exception: there is
 * nothing honest to stamp with, and a beacon the poster invents is the
 * precomputation this exists to stop.
 */
object Beacons {
    private const val TAG = "Beacons"

    /**
     * There is no block to stamp with, so nothing can go on a board.
     *
     * Typed rather than a sentence to match on: this is thrown by our own
     * Kotlin, it reaches a screen, and `moneyFailure` turns it into words in
     * the reader's language — the same rule the ceremony's own two
     * circumstantial failures follow.
     */
    class NoBlock : IllegalStateException("no recent Monero block to stamp a notice against")

    /** How long a tip reading is worth reusing. A block is two minutes. */
    private const val TIP_FRESH_MS = 3L * 60 * 1000

    /** Heights whose hash this device has already asked about. */
    private val hashes = HashMap<Long, String>()

    private var tipHeight: Long = 0
    private var tipAt: Long = 0

    private fun prefs(context: Context) = securePrefs(context, "ducat_contacts")

    /**
     * The chain height this device believes in, or 0 for "no idea".
     *
     * Zero is a real answer and callers must treat it as one: it means skip the
     * freshness test, not that every notice is stale.
     */
    fun tip(context: Context): Long {
        val now = System.currentTimeMillis()
        synchronized(this) {
            if (tipHeight > 0 && now - tipAt < TIP_FRESH_MS) return tipHeight
        }
        // Persisted across restarts so a phone that has just come back does not
        // spend its first sweep with no opinion — a height a few minutes old is
        // well inside the day of slack the window allows.
        val stored = prefs(context).getLong("beacon_tip", 0L)
        val url = NodeStore(context).lastGood() ?: return stored
        val got = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, 0uL) }.getOrNull()
            ?: return stored
        synchronized(this) {
            tipHeight = got.tipHeight.toLong()
            tipAt = now
        }
        prefs(context).edit().putLong("beacon_tip", got.tipHeight.toLong()).apply()
        return got.tipHeight.toLong()
    }

    /** The tip and its hash, for stamping something about to be posted. */
    data class Stamp(val height: Long, val hashHex: String)

    /**
     * What to stamp a notice with, or null when this device cannot say.
     *
     * The tip itself rather than a block or two back: a reader tolerates a
     * couple of blocks either way, and reaching backwards would spend part of
     * the freshness window before the notice is even written.
     */
    fun stampNow(context: Context): Stamp? {
        val url = NodeStore(context).lastGood() ?: return null
        val tip = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, 0uL) }.getOrNull()
            ?: return null
        if (tip.tipHeight == 0uL) return null
        val at = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, tip.tipHeight) }
            .getOrNull() ?: return null
        if (at.hashHex.isBlank()) return null
        synchronized(this) {
            tipHeight = tip.tipHeight.toLong()
            tipAt = System.currentTimeMillis()
            hashes[tip.tipHeight.toLong()] = at.hashHex
        }
        prefs(context).edit().putLong("beacon_tip", tip.tipHeight.toLong()).apply()
        return Stamp(tip.tipHeight.toLong(), at.hashHex)
    }

    /**
     * Does this height really carry this hash?
     *
     * True when it does, and true when this device cannot find out — the same
     * rule as the height test, for the same reason. False only on a real
     * disagreement, which is a notice claiming a block that is not the block.
     */
    fun agrees(context: Context, height: Long, hashHex: String): Boolean {
        if (height <= 0 || hashHex.isBlank()) return true
        synchronized(this) { hashes[height] }?.let { return it.equals(hashHex, true) }
        val url = NodeStore(context).lastGood() ?: return true
        val got = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, height.toULong()) }
            .getOrNull() ?: return true
        if (got.hashHex.isBlank()) return true
        synchronized(this) {
            // Bounded: a sweep sees a handful of distinct heights, but a board
            // full of notices each naming a different one would otherwise be a
            // way to make this map grow without limit.
            if (hashes.size > 256) hashes.clear()
            hashes[height] = got.hashHex
        }
        val ok = got.hashHex.equals(hashHex, true)
        if (!ok) {
            DucatLog.w(
                TAG,
                "a notice claims block $height with a hash that block does not have",
            )
        }
        return ok
    }
}
