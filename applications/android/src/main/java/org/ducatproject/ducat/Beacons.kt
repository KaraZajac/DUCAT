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

    /**
     * How many *new* heights one board read may ask the node about.
     *
     * A confirmation costs one 119-byte call, and the heights an honest cell
     * names are few and repeat — but the heights an *attacker* names are
     * whatever they like, one per slot, and a lookup per notice would make a
     * doctored board into network amplification pointed at every reader of it.
     * So a board gets a budget, anything past it is [Verdict.UNKNOWN], and
     * unknown means held rather than shown.
     *
     * **Eight because a board has eight slots.** That is the number that makes
     * a wholly honest board confirmable in a single pass however its notices
     * are spread — there is no arrangement of eight slots that names more than
     * eight heights — so the budget can only ever bite on a board somebody is
     * doctoring, and then only for the first sweep.
     *
     * Nothing honest is lost after that either. A height, once asked about, is
     * answered for good — including the answer "that is not its hash" — so a
     * doctored slot costs one lookup ever and the budget goes to real notices
     * from the next sweep on. The ceiling for a whole session is the window
     * itself, 720 lookups and 86 KB, and only an attacker pays to reach it.
     */
    private const val LOOKUPS_PER_BOARD = 8

    /** What this device can say about a notice's block. */
    enum class Verdict {
        /** This height carries this hash. Show it. */
        CONFIRMED,

        /**
         * Cannot say yet — the height is above this device's tip, the node
         * would not answer, or the board's lookup budget is spent.
         *
         * **Not a synonym for yes.** §16.18.1's freshness rests on somebody
         * checking the hash, and Monero's two-minute blocks make heights
         * predictable months out: an attacker can pre-mine across a spread of
         * future heights with hashes they invented, and any reader that runs
         * only the cheap height comparison takes them. Three answers rather
         * than two is the rule this codebase has learned before — unreachable
         * settles, unknown defers — and collapsing them here would hand the
         * whole of the precomputation back.
         */
        UNKNOWN,

        /** That height does not carry that hash. Drop it and do not ask again. */
        WRONG,
    }

    /** Heights this device has an answer for: the block's real hash. */
    private val hashes = HashMap<Long, String>()

    private var tipHeight: Long = 0
    private var tipAt: Long = 0

    private fun prefs(context: Context) = securePrefs(context, "ducat_contacts")

    /**
     * The chain height this device believes in, or 0 for "no idea".
     *
     * Zero is a real answer and callers must treat it as one: it means this
     * device has no chain view at all and skips the beacon tests entirely,
     * not that every notice is stale. That is the one degradation §16.18.1
     * allows, and it is not the same thing as [Verdict.UNKNOWN].
     */
    fun tip(context: Context): Long {
        val now = System.currentTimeMillis()
        synchronized(this) {
            if (tipHeight > 0 && now - tipAt < TIP_FRESH_MS) return tipHeight
        }
        // **A tip this device could not refresh is not a tip.**
        //
        // The window's day of slack runs *backwards* — it forgives a notice
        // older than the reader's tip. Forwards there are two blocks, and a
        // stale tip is behind by far more than that: a phone out of a drawer
        // after a week, holding last week's height, would read every honest
        // notice on the board as stamped ahead of the chain and refuse the
        // lot. An empty marketplace with no explanation, which is the exact
        // failure §16.18.1 gives up the hour-long window to avoid.
        //
        // So a height is carried across a restart with the moment it was
        // read, and honoured only while it is current. Past that, this device
        // has no chain view — which is a state the rules already describe,
        // and which shows notices on their signature and their work rather
        // than refusing them on a number it has no confidence in.
        val storedAt = prefs(context).getLong("beacon_tip_at", 0L)
        val stored = usableTip(prefs(context).getLong("beacon_tip", 0L), storedAt, now)
        if (stored > 0) {
            synchronized(this) {
                tipHeight = stored
                tipAt = storedAt
            }
            return stored
        }
        val url = NodeStore(context).lastGood() ?: return 0L
        // The tip's own hash comes back in the same call, so every poll adds
        // one height to the cache for nothing — and the tip is exactly the
        // height honest posters are stamping against right now. A reader that
        // has been running a while has already answered most of what a board
        // will name before it reads one.
        val got = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, 0uL) }.getOrNull()
            ?: return 0L
        val h = got.tipHeight.toLong()
        if (h <= 0) return 0L
        synchronized(this) {
            tipHeight = h
            tipAt = now
            if (got.hashHex.isNotBlank()) remember(h, got.hashHex)
        }
        prefs(context).edit()
            .putLong("beacon_tip", h)
            .putLong("beacon_tip_at", now)
            .apply()
        return h
    }

    /**
     * A height carried across a restart, or 0 if it is too old to be trusted
     * as a *current* tip.
     *
     * Its own function because the rule is easy to state, easy to get wrong,
     * and invisible when it is wrong: a reader judging against a stale height
     * does not crash, it quietly refuses the whole board. Pinned by
     * `:desktop:boardnotice`.
     *
     * A reading stamped *ahead* of now is not current either. A clock wound
     * forward and back (RateStore.isStale has the live case) leaves a stamp
     * the phone will not catch up with for days, and a plain "younger than
     * three minutes" test held a week-old height as the tip for as long as
     * the skew lasted. A stamp this clock has not reached is one it cannot
     * vouch for; a few seconds of slack covers an honest nudge.
     */
    internal fun usableTip(stored: Long, storedAt: Long, now: Long): Long {
        if (stored <= 0 || storedAt <= 0) return 0L
        val age = now - storedAt
        return if (age < TIP_FRESH_MS && age > -STAMP_SLACK_MS) stored else 0L
    }

    /** How far ahead of now a stored reading may sit before it is disbelieved. */
    private const val STAMP_SLACK_MS = 60_000L

    /**
     * Whether this device has a chain view *right now*, answered from state
     * alone — no network, safe anywhere.
     *
     * For screens that need to say the degraded mode exists, not decide it:
     * with no reachable node every stamp shows on its signature and its work,
     * which is the accepted trade (§16.18.1) — but a trade somebody standing
     * in it should be able to see. A marketplace quietly running unverified
     * looks identical to a healthy one, and "cut this reader off, then spam
     * the board" is exactly the play that identical look invites. The browser
     * precedent: an insecure connection still loads, marked.
     */
    fun hasChainView(): Boolean = synchronized(this) {
        tipHeight > 0 && System.currentTimeMillis() - tipAt < TIP_FRESH_MS
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
        // One call: a height of zero means the tip, and the tip's hash comes
        // back with it. Two calls raced the chain — the second could land a
        // block later than the first, and a poster would stamp against a
        // height it had not been given the hash for.
        val at = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, 0uL) }.getOrNull()
            ?: return null
        val h = at.tipHeight.toLong()
        if (h <= 0 || at.hashHex.isBlank()) return null
        synchronized(this) {
            tipHeight = h
            tipAt = System.currentTimeMillis()
            remember(h, at.hashHex)
        }
        prefs(context).edit()
            .putLong("beacon_tip", h)
            .putLong("beacon_tip_at", System.currentTimeMillis())
            .apply()
        return Stamp(h, at.hashHex)
    }

    /**
     * A budget for one board read. Held by the caller for the length of a
     * board, so a doctored cell cannot spend the whole sweep's allowance.
     */
    class Budget internal constructor(internal var left: Int)

    fun budget() = Budget(LOOKUPS_PER_BOARD)

    /**
     * Does this height really carry this hash?
     *
     * The cheap half of §16.18.1 — is the height inside the window — is done
     * by the decoder against a tip. This is the half that costs something and
     * the half that actually secures the stamp: the work is bound to the
     * *hash*, so a beacon nobody looks up is thirty-two bytes the attacker
     * chose, and a height they can predict months ahead.
     */
    fun confirm(context: Context, height: Long, hashHex: String, budget: Budget): Verdict {
        if (height <= 0 || hashHex.isBlank()) return Verdict.UNKNOWN
        synchronized(this) { hashes[height] }?.let {
            return if (it.equals(hashHex, true)) Verdict.CONFIRMED else Verdict.WRONG
        }
        // Above what this device has seen: unknowable *yet*, and a few minutes
        // from being knowable. Held rather than refused — the notice is
        // probably honest and posted by somebody whose node is ahead of ours.
        if (height > tip(context)) return Verdict.UNKNOWN
        // The node before the budget: with nothing to ask, spending an
        // allowance would only shrink what the *next* board gets to check.
        val url = NodeStore(context).lastGood() ?: return Verdict.UNKNOWN
        synchronized(this) {
            if (budget.left <= 0) return Verdict.UNKNOWN
            budget.left -= 1
        }
        val got = runCatching { uniffi.ducat_mobile.moneroBlockRef(url, height.toULong()) }
            .getOrNull() ?: return Verdict.UNKNOWN
        if (got.hashHex.isBlank()) return Verdict.UNKNOWN
        synchronized(this) { remember(height, got.hashHex) }
        if (got.hashHex.equals(hashHex, true)) return Verdict.CONFIRMED
        DucatLog.w(TAG, "a notice claims block $height with a hash that block does not have")
        return Verdict.WRONG
    }

    /** Caller already holds the lock. */
    private fun remember(height: Long, hashHex: String) {
        // Bounded by the window it serves, with room for the churn either
        // side of it. Cleared wholesale rather than evicted one at a time:
        // this is a cache of public facts, and losing it costs lookups.
        //
        // "Facts" as one chain view has them, which can shift by a block or
        // two: Monero reorgs occasionally, so an entry near the tip can name
        // a hash that ends up orphaned, and a poster who stamped the losing
        // side reads as WRONG here after the reorg. Both outcomes are
        // harmless and neither is a bug to hunt — the work was still paid
        // against a real recent block, and the refused poster re-mines for
        // under a second at the next refresh. Not worth expiring entries
        // over: only the last block or two can shift, and a stale *negative*
        // for an orphaned stamp is the correct answer anyway.
        if (hashes.size > 4 * 720) hashes.clear()
        hashes[height] = hashHex
    }
}
