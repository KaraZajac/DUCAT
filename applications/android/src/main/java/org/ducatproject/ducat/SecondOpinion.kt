package org.ducatproject.ducat

import android.content.Context

/**
 * Asking a node we are not already trusting.
 *
 * Blocks are fetched by height and scanned locally, and nothing in that path
 * verifies proof-of-work or chain continuity — the client believes whichever
 * node it is talking to about what is on the chain. That is tolerable for a
 * balance somebody is reading. It is not tolerable at the moment a merchant is
 * told they have been paid, because there the same lie hands over the goods: a
 * node that fabricates a block containing an output to the seller manufactures
 * a settlement that never happened, and the shipped defaults are public nodes
 * reached over plain HTTP, so a network position is enough to be that node.
 *
 * A forged transaction exists nowhere else. So before a sale is called paid,
 * an independent node is asked where the transaction stands.
 *
 * **Three answers, not two.** *In a block elsewhere* settles. *In the pool*,
 * *never heard of it* and *nobody answered* all defer — they are different
 * facts and the log says which, but none of them is money.
 *
 * Two of those used to settle, and both were holes:
 *
 * - **Silence settled.** "A till with one node is not a till with a liar" is
 *   true of a café with bad wifi and false of the attacker this check is for,
 *   because every default node is plain http and the position that lets you
 *   forge a block also lets you drop the second node's packets. Settling on
 *   silence handed the attacker the answer by cutting the wire.
 * - **A pool sighting counted as known.** `get_transactions` answers about
 *   the mempool too, so a payment still replaceable corroborated itself.
 *
 * Now only [Verdict.Confirmed] — in a block, on a node that is neither the one
 * in use nor the operator's own — settles, and only that is cached: a yes from
 * the pool is not a fact yet.
 *
 * **What deferring costs.** Nothing irreversible. The tab stays billed and
 * unpaid, which is what it was a second ago; the receipt is not sent, the
 * customer is not thanked, and the next poll asks again — once a minute, not
 * once a pass. After ten minutes the merchant is told, once, and the tab, the
 * sale and the order all offer them *settle anyway*, which is the operator
 * putting their own word where the corroboration should have been.
 *
 * **The small-sale floor** is the one place the old rule survives, under the
 * operator's hand instead of by default: at or under an amount they set, one
 * block is enough and silence settles. Zero — the default — means no sale is
 * small.
 *
 * This is the phone's half of the desk's `app/src/opinion.rs`. The two are
 * deliberately the same rule, down to the ten minutes and the once-a-minute
 * re-ask, because a bar running a phone and a desk must not settle two
 * different ways.
 */
object SecondOpinion {

    private const val TAG = "SecondOpinion"
    private const val PREFS = "second_opinion"

    /** The operator's small-sale floor, in piconero. */
    private const val FLOOR_KEY = "small_sale_floor"

    /** Don't re-ask about the same transaction faster than this. A node that
     *  is behind will not have caught up in one poll, and the reconciler runs
     *  far more often than blocks arrive. */
    internal const val REASK_MS = 60_000L

    /** How long a transaction may stay uncorroborated before the merchant is
     *  told. Five blocks: long enough that an honestly lagging node has caught
     *  up, short enough that nobody is left staring at an unpaid bill. */
    internal const val ALARM_AFTER_MS = 10 * 60 * 1000L

    /** How long the bridge is given per node. */
    private const val ASK_TIMEOUT_MS = 8_000u

    /** One monero, in piconero — the step in [confirmationsNeeded]. */
    const val ONE_XMR = 1_000_000_000_000L

    enum class Verdict {
        /** Another node has it in a block. */
        Confirmed,

        /** Another node answered — the pool, or never heard of it — so wait. */
        NotYet,

        /** Nobody else could be reached. No opinion either way. */
        NoAnswer,
    }

    // ----- the rule ------------------------------------------------------------
    //
    // Pure Kotlin, no Android, no network: what the app does with an answer,
    // separated from where the answer came from and where the notes are kept.
    // A test drives [Rule.move] on a clock it walks itself rather than waiting
    // ten real minutes for the alarm, the way opinion.rs's tests do.

    /** Everything the rule remembers about one transaction, between asks. */
    data class Memo(
        /** Corroborated, or settled on the operator's word. Never re-asked. */
        var ok: Boolean = false,
        /** When it was last asked, so the ask is throttled to once a minute. */
        var asked: Long = 0,
        /** When it was *first* asked — the ten minutes run from here. */
        var since: Long = 0,
        /** Whether the merchant has already been told. Once, not every pass. */
        var said: Boolean = false,
    )

    /** What the caller should do, and whether to say so out loud. */
    enum class Move {
        /** Hand over, send the receipt, call it paid. */
        Settle,

        /** Leave the sale where it is; the next poll asks again. */
        Hold,

        /** Hold, and tell the merchant — this is the ten-minute mark. */
        HoldAndSay,
    }

    object Rule {
        /**
         * Is it time to ask again?
         *
         * [Elapsed.due], not a bare subtraction: an `asked` stamp ahead of now
         * — a clock wound forward and back — would otherwise hold the gate
         * closed for ever, and the only road to *settled* runs through here.
         */
        fun due(memo: Memo, now: Long): Boolean =
            memo.asked == 0L || Elapsed.due(now, memo.asked, REASK_MS)

        /**
         * What a verdict does to a sale of [amountPxmr], with the operator's
         * [floorPxmr]. Updates [memo]; the caller persists it.
         *
         * [silenceSettles] is the escrow caller's exception, documented at
         * [holdsEscrow].
         */
        fun move(
            memo: Memo,
            verdict: Verdict,
            now: Long,
            amountPxmr: Long = 0,
            floorPxmr: Long = 0,
            silenceSettles: Boolean = false,
        ): Move {
            if (memo.ok) return Move.Settle
            when (verdict) {
                Verdict.Confirmed -> {
                    memo.ok = true
                    memo.asked = 0
                    memo.since = 0
                    memo.said = false
                    return Move.Settle
                }
                // Nobody to ask, and the operator has said sales this small
                // may ride on our own node's word. Their risk, their size.
                // Not cached: the next sale asks again.
                Verdict.NoAnswer ->
                    if (silenceSettles || (amountPxmr in 1..floorPxmr && floorPxmr > 0)) {
                        return Move.Settle
                    }
                Verdict.NotYet -> Unit
            }
            val first = if (memo.since == 0L) now else memo.since
            memo.asked = now
            memo.since = first
            if (Elapsed.due(now, first, ALARM_AFTER_MS) && !memo.said) {
                memo.said = true
                return Move.HoldAndSay
            }
            return Move.Hold
        }

        /**
         * Ten minutes of deferral with nothing to show for it — the state the
         * screens offer *settle anyway* in.
         */
        fun stalled(memo: Memo, now: Long): Boolean =
            !memo.ok && memo.since != 0L && Elapsed.due(now, memo.since, ALARM_AFTER_MS)

        /**
         * How many blocks a payment of this size must have before it settles a
         * sale: one under the operator's floor, three up to a monero, ten above
         * that — Monero's own lock.
         *
         * A block is two minutes; a reorg deeper than three has not been seen
         * in years, and nothing worth more than a monero should ride on fewer.
         * An amount of zero is *unknown*, and unknown is never small.
         */
        fun confirmationsNeeded(amountPxmr: Long, floorPxmr: Long): Int = when {
            amountPxmr in 1..floorPxmr && floorPxmr > 0 -> 1
            amountPxmr <= ONE_XMR -> 3
            else -> 10
        }

        /** Blocks on top of an output at [height] when the tip is [tip]; zero
         *  while it is not in a block. */
        fun confirmationsOf(height: Long, tip: Long): Int =
            if (height > 0 && tip >= height) (tip - height + 1).coerceAtMost(Int.MAX_VALUE.toLong()).toInt() else 0
    }

    // ----- the operator's floor ------------------------------------------------

    /**
     * Sales at or under this settle on one block, and on our own node's word
     * alone when no second node can be reached. Zero — the default — means no
     * sale is small.
     */
    fun floorPxmr(context: Context): Long =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getLong(FLOOR_KEY, 0L)

    fun setFloorPxmr(context: Context, pxmr: Long) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putLong(FLOOR_KEY, pxmr.coerceAtLeast(0L)).apply()
        ContactStore.bump()
    }

    /** [Rule.confirmationsNeeded] with this till's floor. */
    fun confirmationsNeeded(context: Context, amountPxmr: Long): Int =
        Rule.confirmationsNeeded(amountPxmr, floorPxmr(context))

    /** [Rule.confirmationsOf], for screens that hold a height and a tip. */
    fun confirmationsOf(height: Long, tip: Long): Int = Rule.confirmationsOf(height, tip)

    /**
     * How deep the payment behind [txHashHex] is, and how deep it must get.
     *
     * `null` when this wallet has no output carrying that transaction — which
     * is every sighting still in the mempool, and is exactly `0 of N`.
     */
    fun blocksSoFar(context: Context, txHashHex: String?): Int {
        if (txHashHex.isNullOrBlank()) return 0
        val wallet = WalletStore(context)
        val height = wallet.entries()
            .firstOrNull { it.txHashHex.equals(txHashHex, ignoreCase = true) }
            ?.height ?: return 0
        return Rule.confirmationsOf(height, wallet.tip())
    }

    /** Deep enough for a sale of this size to settle. */
    fun deepEnough(context: Context, txHashHex: String?, amountPxmr: Long): Boolean =
        blocksSoFar(context, txHashHex) >= confirmationsNeeded(context, amountPxmr)

    // ----- the questions -------------------------------------------------------

    /**
     * May this transaction be treated as settled?
     *
     * The caller has already matched an output by amount, subaddress and
     * height; this is the last question before that match becomes money. A
     * `false` means *not yet* — never *never* — so the caller should leave the
     * sale where it is and let the next poll try again.
     *
     * [amountPxmr] is the sale, for the floor and nothing else. Zero is
     * *unknown*, which is treated as above any floor: the ceremony's escrow
     * releases pass no amount and must never ride on a small-sale exception.
     */
    fun settles(context: Context, txHashHex: String, amountPxmr: Long = 0L): Boolean {
        // No transaction id to check. The wallet matched an output without one
        // — older scans, and coinbase — and there is nothing a second node
        // could be asked. Amount, subaddress and height matching stand alone.
        if (txHashHex.isBlank()) return true

        val key = txHashHex.lowercase()
        val memo = read(context, key)
        if (memo.ok) return true
        val now = System.currentTimeMillis()
        // Asked recently and it was not good news. Hold without spending
        // another eight seconds of the reconciler's time on it.
        if (!Rule.due(memo, now)) return false

        return decide(context, key, onTx(context, key), now, amountPxmr, floorPxmr(context))
    }

    /**
     * May this escrow be believed to hold what our node says it holds?
     *
     * The same exposure as [settles] and the same stakes, one step earlier: a
     * driver who is shown *fare secured* drives, and a renter who is shown the
     * host's stake landed then funds their own side. Both act on a balance
     * that came from one node's account of the chain, and both are expensive
     * to be wrong about.
     *
     * There is no transaction id to ask about here — the escrow is found by
     * scanning its own address — so the second node is asked the same question
     * instead: scan it yourself, what do you see? An amount at least as large
     * is corroboration. Less is a reason to wait, because a node a block
     * behind sees less and so does an honest one mid-scan.
     *
     * Only growth is checked. Money leaving an escrow is a release, and
     * holding that back would strand the record showing a balance already
     * spent.
     *
     * **Silence still settles here**, and deliberately, unlike [settles]: this
     * question has no "in a block" answer to wait for — the scan either sees
     * the deposit or does not — so deferring on an unreachable node is a
     * permanent stall on a ceremony with a countdown, not a wait for a block
     * two minutes away. The exposure is bounded by the ceremony's own steps,
     * which is not true of goods leaving a counter.
     */
    fun holdsEscrow(
        context: Context,
        idHex: String,
        keys: ByteArray,
        fromHeight: Long,
        claimed: Long,
        nodeInUse: String?,
    ): Boolean {
        if (claimed <= 0) return true
        // Keyed by amount, not by escrow: every increase is its own claim, and
        // corroborating a deposit says nothing about the next one.
        val key = "esc_${idHex}_$claimed"
        val memo = read(context, key)
        if (memo.ok) return true
        val now = System.currentTimeMillis()
        if (!Rule.due(memo, now)) return false

        return decide(
            context, key, onEscrow(keys, fromHeight, claimed, nodeInUse), now,
            silenceSettles = true,
            titleRes = R.string.notify_unbacked_title, bodyRes = R.string.notify_unbacked_body,
            silenceTitleRes = R.string.notify_unbacked_title,
            silenceBodyRes = R.string.notify_unbacked_body,
        )
    }

    /**
     * Asked for ten minutes and still not corroborated — the state the screens
     * show "no second node has confirmed this" for, beside the button that
     * puts the operator's word in its place.
     */
    fun stalled(context: Context, txHashHex: String?): Boolean {
        if (txHashHex.isNullOrBlank()) return false
        return Rule.stalled(read(context, txHashHex.lowercase()), System.currentTimeMillis())
    }

    /**
     * The operator's word in place of the missing corroboration.
     *
     * Recorded as a forcing, not as a corroboration, so the log says who
     * decided — and cached like a yes, because nothing is gained by asking the
     * same unreachable network again about a sale the counter has closed.
     */
    fun settleAnyway(context: Context, txHashHex: String) {
        val key = txHashHex.lowercase()
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putBoolean("ok_$key", true)
            .putLong("forced_$key", System.currentTimeMillis())
            .remove("asked_$key").remove("since_$key").remove("said_$key")
            .apply()
        DucatLog.w(TAG, "${key.take(12)}… settled on the operator's word, uncorroborated")
        ContactStore.bump()
    }

    // ----- the notes -----------------------------------------------------------

    private fun read(context: Context, key: String): Memo {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        return Memo(
            ok = prefs.getBoolean("ok_$key", false),
            asked = prefs.getLong("asked_$key", 0L),
            since = prefs.getLong("since_$key", 0L),
            said = prefs.getBoolean("said_$key", false),
        )
    }

    private fun write(context: Context, key: String, memo: Memo) {
        val e = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
        if (memo.ok) {
            e.putBoolean("ok_$key", true)
                .remove("asked_$key").remove("since_$key").remove("said_$key")
        } else {
            e.putLong("asked_$key", memo.asked).putLong("since_$key", memo.since)
                .putBoolean("said_$key", memo.said)
        }
        e.apply()
    }

    /**
     * The rule, the notes and the notification in one place, so both questions
     * above read as one line each — and so the desk's headless test can walk a
     * timeline through the stored notes without a node on the other end.
     */
    internal fun decide(
        context: Context,
        key: String,
        verdict: Verdict,
        now: Long,
        amountPxmr: Long = 0,
        floorPxmr: Long = 0,
        silenceSettles: Boolean = false,
        titleRes: Int = R.string.notify_unconfirmed_title,
        bodyRes: Int = R.string.notify_unconfirmed_body,
        silenceTitleRes: Int = R.string.notify_uncorroborated_title,
        silenceBodyRes: Int = R.string.notify_uncorroborated_body,
    ): Boolean {
        val memo = read(context, key)
        if (memo.ok) return true
        val move = Rule.move(memo, verdict, now, amountPxmr, floorPxmr, silenceSettles)
        val short = key.take(12)
        if (move == Move.Settle && verdict == Verdict.NoAnswer) {
            DucatLog.i(TAG, "$short… settling with no second node reachable")
        }
        // Settling on silence writes nothing: it is not a fact, and caching it
        // would answer for the next sale as well.
        if (move != Move.Settle || verdict == Verdict.Confirmed) write(context, key, memo)
        if (move == Move.HoldAndSay) {
            val noAnswer = verdict == Verdict.NoAnswer
            DucatLog.w(
                TAG,
                if (noAnswer) "$short… no second node reachable for ten minutes"
                else "$short… not in a block elsewhere after ten minutes",
            )
            Notify.post(
                context,
                context.getString(if (noAnswer) silenceTitleRes else titleRes),
                context.getString(if (noAnswer) silenceBodyRes else bodyRes),
            )
        }
        if (move != Move.Settle) {
            DucatLog.i(
                TAG,
                "$short… deferring: " + if (verdict == Verdict.NoAnswer) {
                    "no second node reachable"
                } else {
                    "not in a block elsewhere yet"
                },
            )
        }
        return move == Move.Settle
    }

    // ----- asking --------------------------------------------------------------

    /**
     * Does a node other than the one we are using also see this much in the
     * escrow?
     *
     * More than one node is tried, and the first that agrees ends it. Asking
     * only one would put every rental in the country behind whichever node
     * happens to sit at the top of the list: one node stuck a few blocks back
     * would stall escrows that are perfectly well funded, and the alarm would
     * be telling people their money is missing when it is not. A single node's
     * agreement is enough to corroborate; it takes all of them failing to see
     * the money before this is worth deferring on.
     */
    private fun onEscrow(
        keys: ByteArray,
        fromHeight: Long,
        claimed: Long,
        nodeInUse: String?,
    ): Verdict {
        val others = candidates(nodeInUse, null)
        if (others.isEmpty()) return Verdict.NoAnswer

        var answered = false
        for (url in others) {
            val seen = runCatching {
                uniffi.ducat_mobile.escrowBalance(keys, url, fromHeight.toULong()).toLong()
            }.getOrNull() ?: continue
            answered = true
            if (seen >= claimed) {
                DucatLog.i(TAG, "escrow corroborated by $url: ${formatXmr(seen)} XMR")
                return Verdict.Confirmed
            }
            DucatLog.i(TAG, "escrow: $url sees ${formatXmr(seen)} of ${formatXmr(claimed)}")
        }
        return if (answered) Verdict.NotYet else Verdict.NoAnswer
    }

    /**
     * Where a transaction stands on a node that is not ours.
     *
     * The bridge hashes the transaction the node hands back and refuses to
     * call it a match unless the hash is the one asked about, so a node that
     * answers with *some* transaction has not answered this question. What
     * comes back is therefore one of four facts about the id we asked for, and
     * three of them are not money.
     */
    fun onTx(context: Context, txHashHex: String): Verdict {
        if (txHashHex.isBlank()) return Verdict.NoAnswer
        val nodes = NodeStore(context)
        val others = candidates(nodes.lastGood(), nodes.ownUrl())
        if (others.isEmpty()) return Verdict.NoAnswer

        val short = txHashHex.take(12)
        var answered = false
        for (url in others) {
            val status = runCatching {
                uniffi.ducat_mobile.moneroTxStatus(url, txHashHex, ASK_TIMEOUT_MS)
            }.getOrNull() ?: continue
            when (status) {
                is uniffi.ducat_mobile.TxStatus.InBlock -> {
                    DucatLog.i(TAG, "$short… in block ${status.height} at $url")
                    return Verdict.Confirmed
                }
                // Seen elsewhere, which rules out a forgery by our node — but
                // not settled: a transaction in the pool can still be
                // replaced, and §15.11 says goods do not leave on a sighting.
                // Wait for the block.
                is uniffi.ducat_mobile.TxStatus.InPool -> {
                    DucatLog.i(TAG, "$short… in the pool at $url — not settled yet")
                    answered = true
                }
                is uniffi.ducat_mobile.TxStatus.Unknown -> answered = true
                is uniffi.ducat_mobile.TxStatus.Unreachable -> Unit
            }
        }
        return if (answered) Verdict.NotYet else Verdict.NoAnswer
    }

    /**
     * The nodes worth asking: never the one in use, never the operator's own,
     * https before http, and one of them rather than two when there is an own
     * node — every extra question hands a public stranger a transaction id
     * this wallet cares about. The bridge owns that rule; both clients ask it
     * the same way.
     */
    private fun candidates(inUse: String?, own: String?): List<String> = runCatching {
        uniffi.ducat_mobile.moneroSecondOpinionNodes(
            inUse?.trim()?.ifBlank { null }, own?.trim()?.ifBlank { null },
        ).map { it.url }
    }.getOrDefault(emptyList())
}
