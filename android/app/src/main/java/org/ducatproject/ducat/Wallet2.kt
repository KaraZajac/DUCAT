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
)

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
}

/** Piconero to a human string. 1 XMR is 10^12 piconero. */
fun formatXmr(pxmr: Long): String {
    val whole = pxmr / 1_000_000_000_000L
    val frac = pxmr % 1_000_000_000_000L
    // Six places: enough to show a stagenet dust payment, few enough to read.
    val micro = frac / 1_000_000L
    return if (whole == 0L && micro == 0L && pxmr > 0) "<0.000001" else "%d.%06d".format(whole, micro)
}
