package org.ducatproject.ducat

import android.content.Context
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import uniffi.ducat_mobile.OwnedOutput
import uniffi.ducat_mobile.moneroScan
import uniffi.ducat_mobile.moneroSpent

private const val TAG = "DucatWallet"

/** Monero's lock: an output cannot be spent for ten blocks after it lands. */
const val LOCK_BLOCKS: Long = 10

/** One scan step. Each block is a request to someone else's node, so a step is
 *  a few seconds of work rather than "until finished". */
private const val WINDOW: UInt = 200u

/**
 * What the wallet knows about its own money.
 *
 * §17.2 forbids collapsing this into one number, and the reason is visible
 * here: an output that has arrived is not an output that can be spent, and a
 * screen that adds them together tells someone they can pay when they cannot.
 */
data class Balances(
    /** Unspent and past the lock. What can actually be handed over. */
    val spendablePxmr: Long,
    /** Unspent but still locked. Arriving, not arrived. */
    val lockedPxmr: Long,
    /** How many separate unspent outputs are past the lock — §17.2's real
     *  constraint, since one output pays one person at a time. */
    val spendableOutputs: Int,
    val blocksToUnlock: Long,
    val scannedTo: Long,
    val tip: Long,
) {
    val syncing: Boolean get() = tip > 0 && scannedTo < tip
}

/** A received output, as the Activity screen shows it. */
data class WalletEntry(
    val amountPxmr: Long,
    val height: Long,
    val spent: Boolean,
    val keyImage: String,
    /** The serialized output, needed to spend it. */
    val blob: ByteArray = ByteArray(0),
)

/** What a send would use and cost, before anything is signed. */
data class SendPlan(
    val notes: List<WalletEntry>,
    val amountPxmr: Long,
    val totalInPxmr: Long,
) {
    val enough: Boolean get() = totalInPxmr >= amountPxmr
}

object Wallet {

    /**
     * Advance the scan by one window. Returns true if it moved.
     *
     * Incremental by necessity: a wallet restored from an old height has
     * hundreds of thousands of blocks to walk at one request each. Doing that
     * in a single call would be a screen that hangs for an hour, so progress is
     * persisted every window and shown as it goes.
     */
    fun scanStep(context: Context, nodeUrl: String): Boolean {
        val store = WalletStore(context)
        val spend = store.spendKeyHex() ?: return false
        val from = store.scannedTo().takeIf { it > 0 } ?: store.restoreHeight().toLong()

        return try {
            val r = moneroScan(nodeUrl, spend, from.toULong(), WINDOW)
            store.recordScan(r.scannedTo.toLong(), r.tip.toLong(), r.outputs)
            if (r.outputs.isNotEmpty()) {
                Log.i(TAG, "found ${r.outputs.size} output(s) up to ${r.scannedTo}")
            }
            r.scannedTo.toLong() > from
        } catch (e: Exception) {
            Log.w(TAG, "scan: ${e.message}")
            false
        }
    }

    /**
     * Refresh which outputs are already spent.
     *
     * Separate from scanning: one request for the whole set rather than one per
     * block. Without it the wallet knows what arrived and not what is left, and
     * would offer money it has already handed over.
     */
    fun refreshSpent(context: Context, nodeUrl: String) {
        val store = WalletStore(context)
        val entries = store.entries().filter { it.keyImage.isNotEmpty() }
        if (entries.isEmpty()) return
        try {
            val spent = moneroSpent(nodeUrl, entries.map { it.keyImage })
            store.recordSpent(entries.map { it.keyImage }.zip(spent).toMap())
        } catch (e: Exception) {
            Log.w(TAG, "spent check: ${e.message}")
        }
    }

    fun balances(context: Context): Balances {
        val store = WalletStore(context)
        val tip = store.tip()
        val unspent = store.entries().filter { !it.spent }
        val unlocked = unspent.filter { tip > 0 && it.height + LOCK_BLOCKS <= tip }
        val locked = unspent - unlocked.toSet()
        // The nearest unlock, because "in about N minutes" needs the soonest
        // one rather than an average nobody experiences.
        val soonest = locked.minOfOrNull { it.height + LOCK_BLOCKS - tip }?.coerceAtLeast(0) ?: 0
        return Balances(
            spendablePxmr = unlocked.sumOf { it.amountPxmr },
            lockedPxmr = locked.sumOf { it.amountPxmr },
            spendableOutputs = unlocked.size,
            blocksToUnlock = soonest,
            scannedTo = store.scannedTo(),
            tip = tip,
        )
    }

    fun entries(context: Context): List<WalletEntry> =
        WalletStore(context).entries().sortedByDescending { it.height }

    /**
     * Choose notes to cover an amount.
     *
     * Largest first, so a payment uses as few notes as possible. §17.2's
     * constraint is the count rather than the total: every note spent is one
     * fewer person you can pay before waiting for change to unlock, and
     * sweeping five small notes to pay for a coffee leaves you unable to buy
     * the next one.
     *
     * The fee is not known until the transaction is built, so this deliberately
     * does not pretend to: it reports what it selected and lets the build come
     * back with the real number.
     */
    fun plan(context: Context, amountPxmr: Long): SendPlan {
        val tip = WalletStore(context).tip()
        val usable = WalletStore(context).entries()
            .filter { !it.spent && it.blob.isNotEmpty() && tip > 0 && it.height + LOCK_BLOCKS <= tip }
            .sortedByDescending { it.amountPxmr }
        val picked = mutableListOf<WalletEntry>()
        var total = 0L
        for (n in usable) {
            if (total >= amountPxmr) break
            picked += n
            total += n.amountPxmr
        }
        return SendPlan(picked, amountPxmr, total)
    }

    /** Build, sign and broadcast. Blocking; call it off the main thread. */
    fun send(
        context: Context,
        nodeUrl: String,
        toAddress: String,
        amountPxmr: Long,
    ): uniffi.ducat_mobile.SendResult {
        val store = WalletStore(context)
        val spend = store.spendKeyHex()
            ?: throw IllegalStateException("no wallet on this device")
        val plan = plan(context, amountPxmr)
        if (!plan.enough) {
            throw IllegalStateException(
                "not enough unlocked — ${formatXmr(plan.totalInPxmr)} XMR available"
            )
        }
        val r = uniffi.ducat_mobile.moneroSend(
            nodeUrl, spend, plan.notes.map { it.blob }, toAddress, amountPxmr.toULong(),
        )
        // Mark the inputs spent immediately rather than waiting for a rescan.
        // The daemon will confirm it, but until then a second send must not be
        // offered the same notes — that builds a double spend the network will
        // reject and the user will not understand.
        store.recordSpent(plan.notes.associate { it.keyImage to true })
        return r
    }
}

/** Piconero to a human string. 1 XMR is 10^12 piconero. */
fun formatXmr(pxmr: Long): String {
    val whole = pxmr / 1_000_000_000_000L
    val frac = pxmr % 1_000_000_000_000L
    // Six places: enough to show a stagenet dust payment, few enough to read.
    val micro = frac / 1_000_000L
    return if (whole == 0L && micro == 0L && pxmr > 0) "<0.000001" else "%d.%06d".format(whole, micro)
}


/**
 * A balance as a currency figure, and whether it means anything.
 *
 * **A stagenet balance is worth nothing.** Converting test coins to a currency
 * amount would put a number on the screen that someone could act on, and there
 * is no reading under which it is true. So the conversion is computed and the
 * caller is told it is notional; the screen must say so rather than showing a
 * figure that looks like money.
 */
data class FiatView(
    val text: String,
    val notional: Boolean,
    val stale: Boolean,
)

object Rates {
    /** Refresh if the cache is stale and the user has not turned it off. */
    fun refresh(context: Context) {
        val store = RateStore(context)
        if (!store.enabled() || !store.isStale()) return
        try {
            val r = uniffi.ducat_mobile.moneroRate(store.currency(), 12_000u)
            store.store(r.perXmr, r.fetchedAt.toLong(), r.source)
        } catch (e: Exception) {
            Log.w(TAG, "rate: ${e.message}")
        }
    }

    fun view(context: Context, pxmr: Long, stagenet: Boolean): FiatView? {
        val store = RateStore(context)
        if (!store.enabled()) return null
        val (rate, _) = store.cached() ?: return null
        val xmr = pxmr.toDouble() / 1_000_000_000_000.0
        val amount = xmr * rate
        return FiatView(
            text = "%s %.2f".format(store.currency(), amount),
            notional = stagenet,
            stale = store.isStale(),
        )
    }
}
