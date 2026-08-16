package org.ducatproject.ducat

import android.content.Context
import kotlin.math.roundToInt
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
    /** Measured blocks per second, or 0 when nothing has been timed yet. */
    val scanRate: Double = 0.0,
    /** Where this wallet's scan began — its restore height, or a skip-ahead. */
    val scanFrom: Long = 0,
    /** Why the last window failed, if it did. Shown even while progress exists. */
    val error: String? = null,
) {
    val syncing: Boolean get() = tip > 0 && scannedTo < tip

    val blocksLeft: Long get() = (tip - scannedTo).coerceAtLeast(0)

    /**
     * Fraction of the way there, **over the range this wallet needs**.
     *
     * Measured from where the scan began, not from the genesis block. A wallet
     * created a day ago has 720 blocks to read out of 2.18 million; against the
     * whole chain that is 99.97% before it starts and 100% when it finishes, so
     * the bar would sit still through the entire job it exists to show. The
     * earlier version did exactly that while its comment claimed otherwise.
     *
     * `kotlin.Float` spelled out: this project has its own `Float`, which is
     * §17.2's spendable balance, and the bare name resolves to that.
     */
    val progress: kotlin.Float
        get() {
            val span = tip - scanFrom
            if (tip <= 0 || span <= 0) return 0f
            return ((scannedTo - scanFrom).toFloat() / span).coerceIn(0f, 1f)
        }

    /**
     * Seconds left, or null when there is nothing honest to say.
     *
     * Null rather than zero when the rate is unknown: a screen showing "0
     * minutes remaining" for a scan that has not started is worse than one
     * showing nothing.
     */
    val secondsLeft: Long?
        get() = if (scanRate > 0.01 && blocksLeft > 0) (blocksLeft / scanRate).toLong() else null
}

/** "about 3 minutes", "about 2 hours" — never a false precision. */
fun humanDuration(context: android.content.Context, secs: Long): String {
    fun plural(res: Int, n: Int) = context.resources.getQuantityString(res, n, n)
    return when {
        secs < 90 -> context.getString(R.string.duration_under_minute)
        secs < 5400 ->
            plural(R.plurals.duration_minutes, (secs / 60.0).roundToInt().coerceAtLeast(1))
        secs < 172_800 ->
            plural(R.plurals.duration_hours, (secs / 3600.0).roundToInt().coerceAtLeast(1))
        else ->
            plural(R.plurals.duration_days, (secs / 86_400.0).roundToInt().coerceAtLeast(1))
    }
}

/** A received output, as the Activity screen shows it. */
data class WalletEntry(
    val amountPxmr: Long,
    val height: Long,
    val spent: Boolean,
    val keyImage: String,
    /** The serialized output, needed to spend it. */
    val blob: ByteArray = ByteArray(0),
    /**
     * The transaction that created it.
     *
     * Two outputs of one transaction are one event, not two, and the wallet's
     * own change is an output of a transaction the wallet sent. Without this
     * an output list cannot be read back as a history — which is how a send
     * plus its change came out as two receipts and a balance that looked
     * doubled.
     */
    val txHashHex: String = "",
    /** Block time in seconds, or 0 until it has been looked up. */
    val timestamp: Long = 0,
    /** The subaddress minor that received it: 0 primary, else a contact's
     *  (§15.10) — attribution by construction, not by believing a note. */
    val minor: Int = 0,
)

/**
 * Why the wallet is not reading the chain, when it is not.
 *
 * The screen used to say "waiting for a node" for every case, which was a guess
 * presented as a diagnosis — and wrong for the one case a user can actually do
 * something about.
 */
enum class SyncBlocker {
    /** Scanning, or caught up. Nothing wrong. */
    None,
    /** No wallet key on this device: onboarding predates wallet persistence. */
    NoWallet,
    /** No node has answered yet. Usually resolves on its own. */
    NoNode,
    /** A node answered and the scan itself failed. The reason is kept. */
    Failing,
}

/**
 * What a payment costs, before anything is signed.
 *
 * Everything here is an estimate and is labelled one. The exact fee is known
 * only once decoys are chosen and the transaction is built, which is network
 * work that cannot happen on every keystroke — but the weight model is the real
 * structure of a CLSAG transaction, so it is close rather than a guess.
 */
data class Quote(
    val amountPxmr: Long,
    val feePxmr: Long,
    val notes: Int,
    val estimatedBytes: Long,
    val minutesToConfirm: Int,
    /** Everything this payment removes from the balance. */
    val totalPxmr: Long,
    /** What is left afterwards, unlocked. */
    val remainingPxmr: Long,
    val affordable: Boolean,
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
        // Before anything else: outputs written under the broken key-image
        // derivation cannot be reconciled, only re-read.
        if (store.migrateOutputsIfNeeded()) {
            DucatLog.w(
                TAG,
                "cleared stored outputs — key images were wrong, rescanning from " +
                    "${store.restoreHeight()}",
            )
        }
        val spend = store.spendKeyHex()
        if (spend == null) {
            // Distinct from "no node", and the difference is everything: this
            // wallet cannot ever scan, and no amount of waiting fixes it.
            DucatLog.w(TAG, "no spend key stored — this wallet predates wallet persistence")
            return false
        }
        val from = store.scannedTo().takeIf { it > 0 } ?: store.restoreHeight().toLong()

        return try {
            // Watch every allocated per-contact minor: a payment to an
            // unregistered subaddress is invisible (§15.10).
            val r = moneroScan(nodeUrl, spend, from.toULong(), WINDOW,
                store.subaddressCount().toUInt())
            // The window overlaps the tip on purpose (a reorg can rewrite it),
            // so the same output comes back every pass until the chain moves
            // on. The store dedupes by key image; the log must too, or one
            // coffee is announced four times.
            val known = store.entries().map { it.keyImage }.toSet()
            store.recordScan(r.scannedTo.toLong(), r.tip.toLong(), r.outputs)
            store.recordScanError(null)
            NodeStore(context).nodeSucceeded()
            for (o in r.outputs.filter { it.keyImageHex !in known }) {
                DucatLog.i(
                    TAG,
                    "received ${formatXmr(o.amountPxmr.toLong())} XMR at block ${o.height}",
                )
            }
            if (r.blocksFailed > 0u) {
                DucatLog.w(
                    TAG,
                    "scanned to ${r.scannedTo} — ${r.blocksFailed} block(s) unreadable",
                )
            }
            r.scannedTo.toLong() > from
        } catch (e: Throwable) {
            // Throwable, not Exception: a panic crossing the FFI boundary
            // arrives as an Error, and catching only Exception let the real
            // cause disappear while the screen said "not started".
            DucatLog.w(TAG, "scan failed: ${e}")
            store.recordScanError(e.message ?: e.toString())
            if (NodeStore(context).nodeFailed()) {
                DucatLog.w(TAG, "node demoted after repeated failures — will re-probe")
            }
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
            DucatLog.w(TAG, "spent check: ${e.message}")
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
            scanRate = store.scanRate(),
            scanFrom = store.restoreHeight().toLong(),
            error = store.lastScanError(),
        )
    }

    fun blocker(context: Context): SyncBlocker {
        val store = WalletStore(context)
        return when {
            store.spendKeyHex() == null -> SyncBlocker.NoWallet
            store.lastScanError() != null -> SyncBlocker.Failing
            store.tip() == 0L -> SyncBlocker.NoNode
            else -> SyncBlocker.None
        }
    }

    fun lastError(context: Context): String? = WalletStore(context).lastScanError()

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

    /**
     * The fee for a shape of transaction, cached for a minute.
     *
     * Cached because the rate is a network call and the amount field changes on
     * every keystroke; a minute is far shorter than fees move and far longer
     * than someone types a number.
     */
    private var feeCache: Triple<String, Long, Long>? = null  // key, fee, at

    fun feeFor(context: Context, inputs: Int, priority: Int = 1): Long {
        val node = NodeStore(context).lastGood() ?: return 0
        val key = "$node|$inputs|$priority"
        val now = System.currentTimeMillis()
        // A zero is a cached *failure* and expires sooner, so a sick node is
        // retried in seconds — but not once per keystroke, which is what the
        // field log showed: a burst of identical estimate errors as the
        // amount field recomposed against a node that had stopped answering.
        feeCache?.let { (k, fee, at) ->
            val ttl = if (fee == 0L) 15_000 else 60_000
            if (k == key && now - at < ttl) return fee
        }
        return try {
            val e = uniffi.ducat_mobile.moneroFeeEstimate(
                node, inputs.coerceAtLeast(1).toUInt(), 2u, priority.toUInt(),
            )
            feeCache = Triple(key, e.feePxmr.toLong(), now)
            e.feePxmr.toLong()
        } catch (e: Exception) {
            DucatLog.w(TAG, "fee estimate: ${e.message}")
            feeCache = Triple(key, 0L, now)
            0
        }
    }

    fun minutesToConfirm(priority: Int): Int = when (priority) {
        0 -> 20; 1 -> 6; 2 -> 4; else -> 2
    }

    /**
     * The most that can actually be sent, fee included.
     *
     * Not the balance. Offering the balance as the maximum is how a wallet
     * lets someone type a number it will then refuse, and the refusal arrives
     * after they have decided.
     */
    fun maxSendable(context: Context, priority: Int = 1): Long {
        val b = balances(context)
        if (b.spendableOutputs == 0) return 0

        // Converged rather than assumed. The fee depends on how many notes the
        // payment consumes, and the notes consumed depend on the amount — so
        // pricing the worst case understates the maximum, sometimes badly for
        // someone holding many small notes. Start from every note, see how many
        // that amount actually needs, and price again.
        var fee = feeFor(context, b.spendableOutputs, priority)
        repeat(2) {
            val candidate = (b.spendablePxmr - fee).coerceAtLeast(0)
            val needed = plan(context, candidate).notes.size.coerceAtLeast(1)
            val next = feeFor(context, needed, priority)
            if (next == fee) return@repeat
            fee = next
        }
        return (b.spendablePxmr - fee).coerceAtLeast(0)
    }

    /** Cost of sending this amount, with what is left afterwards. */
    fun quote(context: Context, amountPxmr: Long, priority: Int = 1): Quote {
        val b = balances(context)
        val plan = plan(context, amountPxmr)
        // Fee scales with input count, and the input count comes from the
        // amount — so a quote for an unaffordable amount still prices the notes
        // it would have needed rather than pretending one would do.
        val inputs = if (plan.notes.isEmpty()) b.spendableOutputs else plan.notes.size
        val fee = feeFor(context, inputs, priority)
        val total = amountPxmr + fee
        return Quote(
            amountPxmr = amountPxmr,
            feePxmr = fee,
            notes = inputs,
            estimatedBytes = 0,
            minutesToConfirm = minutesToConfirm(priority),
            totalPxmr = total,
            remainingPxmr = (b.spendablePxmr - total).coerceAtLeast(0),
            affordable = total <= b.spendablePxmr,
        )
    }

    /** Build, sign and broadcast. Blocking; call it off the main thread. */
    fun send(
        context: Context,
        nodeUrl: String,
        toAddress: String,
        amountPxmr: Long,
        contactHex: String? = null,
        note: String? = null,
        // The speed the user picked, which was previously shown and ignored.
        priority: Int = 1,
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
        DucatLog.i(
            TAG,
            "sending ${formatXmr(amountPxmr)} XMR using ${plan.notes.size} note(s) " +
                "to ${toAddress.take(12)}…",
        )
        val r = try {
            uniffi.ducat_mobile.moneroSend(
                nodeUrl, spend, plan.notes.map { it.blob }, toAddress,
                amountPxmr.toULong(), priority.toUInt(),
            )
        } catch (e: Throwable) {
            DucatLog.e(TAG, "send failed: ${e.message ?: e}")
            // A failed send counts against the node like a failed scan does:
            // the retry the user is about to make should not be fed to the
            // same dying node until it re-earns its place.
            if (NodeStore(context).nodeFailed()) {
                DucatLog.w(TAG, "node demoted after repeated failures — will re-probe")
            }
            throw e
        }
        DucatLog.i(
            TAG,
            "sent ${r.txidHex.take(16)}… fee ${formatXmr(r.feePxmr.toLong())} XMR, " +
                "accepted by ${r.acceptedBy} node(s)",
        )
        // Recorded here, not on the next scan: the outputs a payment creates
        // belong to the recipient, so nothing the wallet scans will ever show
        // that this happened. Without it the balance drops for no visible
        // reason.
        store.recordSent(
            r.txidHex, amountPxmr, r.feePxmr.toLong(), toAddress, contactHex, note,
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
            DucatLog.w(TAG, "rate: ${e.message}")
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
