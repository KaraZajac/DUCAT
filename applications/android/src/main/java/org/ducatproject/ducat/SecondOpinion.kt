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
 * an independent node is asked whether it has ever heard of the transaction.
 *
 * **Three answers, not two.** Silence is not a denial. A node that cannot be
 * reached has said nothing, and treating that as a refusal would stop honest
 * sales in every bar with bad wifi — which is most of them. A node that
 * answers "no" is not proof either, only a reason to wait: it may be a block
 * behind, and Monero blocks are two minutes apart. So a No defers rather than
 * accuses, and only a No that persists past several blocks is worth raising
 * with the person behind the counter.
 *
 * **What deferring costs.** Nothing irreversible. The tab stays billed and
 * unpaid, which is what it was a second ago; the receipt is not sent, the
 * customer is not thanked, and the next poll asks again. The merchant keeps
 * the out-of-band settle for the case where they are satisfied by other means.
 */
object SecondOpinion {

    private const val TAG = "SecondOpinion"
    private const val PREFS = "second_opinion"

    /** How many independent nodes to try before concluding nobody answered. */
    private const val TRIES = 2

    /** Don't re-ask about the same transaction faster than this. A node that
     *  is behind will not have caught up in one poll, and the reconciler runs
     *  far more often than blocks arrive. */
    private const val REASK_MS = 60_000L

    /** How long a transaction may stay unconfirmed before the merchant is told.
     *  Five blocks: long enough that an honestly lagging node has caught up,
     *  short enough that nobody is left staring at an unpaid bill. */
    private const val ALARM_AFTER_MS = 10 * 60 * 1000L

    enum class Verdict {
        /** Somebody else has it too. */
        Confirmed,
        /** Somebody else answered, and does not have it — wait, then ask again. */
        NotYet,
        /** Nobody else could be reached. No opinion either way. */
        NoAnswer,
    }

    /**
     * May this transaction be treated as settled?
     *
     * The caller has already matched an output by amount, subaddress and
     * height; this is the last question before that match becomes money. A
     * `false` means *not yet* — never *never* — so the caller should leave the
     * sale where it is and let the next poll try again.
     */
    fun settles(context: Context, txHashHex: String): Boolean {
        // No transaction id to check. The wallet matched an output without one
        // — older scans, and coinbase — and there is nothing a second node
        // could be asked. Amount, subaddress and height matching stand alone.
        if (txHashHex.isBlank()) return true

        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val key = txHashHex.lowercase()
        if (prefs.getBoolean("ok_$key", false)) return true

        val now = System.currentTimeMillis()
        // Asked recently and it was not good news. Hold without spending
        // another eight seconds of the reconciler's time on it: a node that is
        // behind will not have caught up since the last poll.
        val asked = prefs.getLong("asked_$key", 0L)
        // Elapsed, as this file already does for the alarm four lines below
        // the write. A raw subtraction here means an `asked_` stamp ahead of
        // now holds the gate closed for ever, and the only path to "settled"
        // runs through the call this returns before — so a paid sale never
        // settles, silently, with the screen still saying it is waiting.
        if (asked != 0L && !Elapsed.due(now, asked, REASK_MS)) return false

        return decide(
            context, key, onTx(context, key), now,
            R.string.notify_unconfirmed_title, R.string.notify_unconfirmed_body,
        )
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
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (prefs.getBoolean("ok_$key", false)) return true

        val now = System.currentTimeMillis()
        val asked = prefs.getLong("asked_$key", 0L)
        // Elapsed, as this file already does for the alarm four lines below
        // the write. A raw subtraction here means an `asked_` stamp ahead of
        // now holds the gate closed for ever, and the only path to "settled"
        // runs through the call this returns before — so a paid sale never
        // settles, silently, with the screen still saying it is waiting.
        if (asked != 0L && !Elapsed.due(now, asked, REASK_MS)) return false

        return decide(
            context, key, onEscrow(keys, fromHeight, claimed, nodeInUse), now,
            R.string.notify_unbacked_title, R.string.notify_unbacked_body,
        )
    }

    /**
     * What a verdict means, with the clock passed in.
     *
     * Split out from [settles] so the policy can be tested without a node on
     * the other end and without waiting ten real minutes for the alarm.
     */
    internal fun decide(
        context: Context,
        key: String,
        verdict: Verdict,
        now: Long,
        titleRes: Int = R.string.notify_unconfirmed_title,
        bodyRes: Int = R.string.notify_unconfirmed_body,
    ): Boolean {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val since = prefs.getLong("since_$key", 0L)

        return when (val v = verdict) {
            Verdict.Confirmed -> {
                prefs.edit()
                    .putBoolean("ok_$key", true)
                    .remove("asked_$key").remove("since_$key").remove("said_$key")
                    .apply()
                true
            }
            // Nobody to ask. Proceeding is the lesser risk: refusing here would
            // mean an offline till never settles a sale, which breaks the app
            // for everyone to defend against an attacker who has to already be
            // on the wire. The log line is the audit trail.
            Verdict.NoAnswer -> {
                DucatLog.i(TAG, "${key.take(12)}… settling unconfirmed — no second node reachable")
                true
            }
            Verdict.NotYet -> {
                val first = if (since == 0L) now else since
                prefs.edit().putLong("asked_$key", now).putLong("since_$key", first).apply()
                if (Elapsed.due(now, first, ALARM_AFTER_MS) &&
                    !prefs.getBoolean("said_$key", false)
                ) {
                    prefs.edit().putBoolean("said_$key", true).apply()
                    DucatLog.w(TAG, "${key.take(12)}… unknown to other nodes after ten minutes")
                    Notify.post(
                        context,
                        context.getString(titleRes),
                        context.getString(bodyRes),
                    )
                }
                DucatLog.i(TAG, "${key.take(12)}… deferring: $v")
                false
            }
        }
    }

    /**
     * Does a node other than the one we are using have this transaction?
     *
     * Candidates come from the shipped list rather than from whatever is
     * configured, and the node in use is excluded — asking the same node twice
     * is not a second opinion.
     */
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
        val inUse = nodeInUse?.trim()
        val others = runCatching {
            uniffi.ducat_mobile.moneroDefaultNodes(null)
                .map { it.url }
                .filter { it.trim() != inUse }
        }.getOrDefault(emptyList())
        if (others.isEmpty()) return Verdict.NoAnswer

        var answered = false
        for (url in others.take(TRIES)) {
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

    fun onTx(context: Context, txHashHex: String): Verdict {
        if (txHashHex.isBlank()) return Verdict.NoAnswer
        val inUse = NodeStore(context).lastGood()?.trim()
        val others = runCatching {
            uniffi.ducat_mobile.moneroDefaultNodes(null)
                .map { it.url }
                .filter { it.trim() != inUse }
        }.getOrDefault(emptyList())
        if (others.isEmpty()) return Verdict.NoAnswer

        var answered = false
        for (url in others.take(TRIES)) {
            val v = runCatching {
                uniffi.ducat_mobile.moneroTxKnown(url, txHashHex, 8_000u)
            }.getOrNull() ?: continue
            when (v) {
                uniffi.ducat_mobile.TxKnown.YES -> {
                    DucatLog.i(TAG, "${txHashHex.take(12)}… confirmed by $url")
                    return Verdict.Confirmed
                }
                uniffi.ducat_mobile.TxKnown.NO -> answered = true
                uniffi.ducat_mobile.TxKnown.UNREACHABLE -> Unit
            }
        }
        return if (answered) Verdict.NotYet else Verdict.NoAnswer
    }
}
