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

    // How long an unresolved send intent may hold notes hostage before an
    // all-unspent chain answer is believed to mean "never relayed". Longer
    // than any node timeout plus mempool life for a tx that will never mine.
    private const val INTENT_GIVE_UP_SECS = 30L * 60

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
) {
    /**
     * Whether [feePxmr] is a fee or an apology.
     *
     * `feeFor` returns zero when it cannot reach a node — it says so in its own
     * comment, calling a zero "a cached failure" — and a Monero transaction
     * never costs nothing. So a zero here means *not known*, and a screen that
     * prints it as a fee promises a total it cannot deliver.
     */
    val feeKnown: Boolean get() = feePxmr > 0
}

/** What a send would use and cost, before anything is signed. */
data class SendPlan(
    val notes: List<WalletEntry>,
    val amountPxmr: Long,
    val totalInPxmr: Long,
    /** What the picked notes will cost to spend, as far as anyone can know yet. */
    val feePxmr: Long = 0,
) {
    /**
     * Enough to cover the amount *and* the fee.
     *
     * It used to mean the amount alone, which is a different question from the
     * one every caller was asking. A wallet holding 0.002315 refused to send
     * 0.000978 — plenty of money, one note picked, and that note was a hair
     * too small once the fee landed on it.
     */
    val enough: Boolean get() = totalInPxmr >= amountPxmr + feePxmr
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
            // Spent is a one-way door, and this may only ever push it shut.
            //
            // `send` marks its inputs spent the moment it broadcasts, because
            // a second payment must not be offered notes already committed to
            // a first. But the chain does not know about that transaction
            // until it is mined — a couple of minutes — and this asks the
            // chain. Writing the answer back for every key image therefore
            // resurrected the notes the wallet had just spent: the balance
            // jumped back up seconds after a payment, and the next payment
            // picked the same notes and built a double spend the network
            // refused with "signed, but no node accepted it", which tells the
            // person holding the phone nothing at all.
            //
            // recordSpent leaves out whatever it is not told about, so telling
            // it only about the ones the chain confirms is the whole fix.
            val chainSpent = entries.map { it.keyImage }.zip(spent)
                .filter { (_, gone) -> gone }.map { (ki, _) -> ki }.toSet()
            store.recordSpent(chainSpent.associateWith { true })
            val chainAnswered = entries.map { it.keyImage }.toSet()
            // Dangling send intents get their verdict here, from the chain,
            // never from a thrown exception (a timeout can post-date the
            // relay). Inputs the chain shows spent mean the send happened
            // and the process died before recording it: convert the intent
            // into the record it was standing in for — txid died with the
            // process, but the balance and the double-pay guard stay honest.
            // An intent well past any relay window whose inputs the chain
            // still shows UNSPENT never made it out: drop it and the notes
            // come home.
            val now = System.currentTimeMillis() / 1000
            for (intent in store.sendIntents()) {
                val kis = intent.keyImages.toSet()
                when {
                    kis.any { it in chainSpent } -> {
                        DucatLog.w(TAG, "send intent ${intent.id} resolved by chain — recording without txid")
                        store.resolveSendIntent(intent.id, "", 0L)
                    }
                    now - intent.ts > INTENT_GIVE_UP_SECS &&
                        kis.isNotEmpty() && kis.all { it in chainAnswered && it !in chainSpent } -> {
                        DucatLog.w(TAG, "send intent ${intent.id} never relayed — releasing its notes")
                        store.dropSendIntent(intent.id)
                    }
                }
            }
        } catch (e: Exception) {
            DucatLog.w(TAG, "spent check: ${e.message}")
        }
    }

    fun balances(context: Context): Balances {
        val store = WalletStore(context)
        val tip = store.tip()
        // The same usability test `plan` applies, for the same reason: an
        // entry with no blob is an output this wallet cannot build a spend
        // from, and counting it makes "Ready to spend" a number §17.2 forbids
        // — one the wallet cannot honour. Found sweeping an old desk state,
        // where balances() said 0.000578 XMR and plan() could reach 0.000100
        // of it, so every send failed with "not enough unlocked" against a
        // balance that plainly said otherwise. Understating is the safe
        // direction; promising is not.
        // Same rule for notes pinned by an in-flight send intent: plan will
        // not offer them, so the balance must not promise them.
        val inFlight = store.sendIntents().flatMap { it.keyImages }.toSet()
        val unspent = store.entries()
            .filter { !it.spent && it.blob.isNotEmpty() && it.keyImage !in inFlight }
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
     * The exact fee is not known until the transaction is built and signed, but
     * it is *estimable*, and this used to decline to estimate it — stopping the
     * moment the picked notes covered the amount, fee uncounted. The builder
     * then added the fee, came up short, and returned NotEnoughFunds, which
     * reached a person as "not enough in the notes you picked, once the fee is
     * counted" while their balance sat there visibly covering it twice over.
     * Nothing was wrong with the wallet. The picker just stopped one note early
     * and there was no way to tell it to keep going.
     *
     * So: keep picking until the notes cover amount *plus* the fee for spending
     * exactly this many of them. The fee grows with the input count, which is
     * why it is re-asked as the set grows rather than priced once up front —
     * a fee for one input does not pay for three. [feeFor] caches per count, so
     * a two-note payment asks the node once per distinct size and no more.
     *
     * An estimate that fails returns zero, which lands this back on the old
     * behaviour rather than refusing to send at all — an offline fee estimate
     * should not become an offline wallet.
     */
    fun plan(context: Context, amountPxmr: Long, priority: Int = 1): SendPlan {
        val tip = WalletStore(context).tip()
        // Notes named by an in-flight send intent are committed money until
        // the chain rules otherwise — offering them again is how a double
        // spend gets built politely.
        val inFlight = WalletStore(context).sendIntents()
            .flatMap { it.keyImages }.toSet()
        val usable = WalletStore(context).entries()
            .filter {
                !it.spent && it.blob.isNotEmpty() && tip > 0 &&
                    it.height + LOCK_BLOCKS <= tip &&
                    it.keyImage !in inFlight
            }
            .sortedByDescending { it.amountPxmr }
        val picked = mutableListOf<WalletEntry>()
        var total = 0L
        var fee = 0L
        for (n in usable) {
            fee = feeFor(context, picked.size, priority)
            if (total >= amountPxmr + fee) break
            picked += n
            total += n.amountPxmr
        }
        // Priced for what was actually picked: the loop exits either on the
        // break above (fee already current) or by running out of notes, and in
        // the second case the last estimate was for one note fewer than we hold.
        fee = feeFor(context, picked.size, priority)
        return SendPlan(picked, amountPxmr, total, fee)
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

        // The maximum is not "everything minus the fee", because a note can be
        // worth less than it costs to spend. Sweeping a dust note in alongside
        // a whole monero *lowers* the amount that comes out the other side, so
        // the honest maximum leaves it behind.
        //
        // This used to converge by asking plan() how many notes an amount
        // needed — which worked only while plan() ignored the fee, and became a
        // loop that could never move the moment it stopped. Priced directly
        // instead: one estimate for a single input and one for every input give
        // the marginal cost of a note, and a note earns its place by being
        // worth more than that.
        val all = b.spendableOutputs
        val feeOne = feeFor(context, 1, priority)
        val feeAll = feeFor(context, all, priority)
        val perNote = if (all > 1) (feeAll - feeOne) / (all - 1) else 0

        val tip = WalletStore(context).tip()
        val worth = WalletStore(context).entries()
            .filter { !it.spent && it.blob.isNotEmpty() && tip > 0 && it.height + LOCK_BLOCKS <= tip }
            .sortedByDescending { it.amountPxmr }
            .filter { it.amountPxmr > perNote }
            .ifEmpty { return 0 }

        return (worth.sumOf { it.amountPxmr } - feeFor(context, worth.size, priority))
            .coerceAtLeast(0)
    }

    /** Cost of sending this amount, with what is left afterwards. */
    fun quote(context: Context, amountPxmr: Long, priority: Int = 1): Quote {
        val b = balances(context)
        val plan = plan(context, amountPxmr, priority)
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
            // The same question the send will ask, asked here. A quote that
            // says affordable and a send that refuses were two answers to one
            // question, and the user got them in that order.
            affordable = plan.enough,
        )
    }

    /**
     * Whether a failure is the node's rather than the wallet's.
     *
     * The bridge hands these up as the shape it got from the transport, and
     * they reach a screen as, verbatim: `v1=decoys:
     * InterfaceError(InterfaceError("timed out reading response"))`. Nobody
     * can act on that. What it means is that fetching ring members timed out —
     * `get_outs` is the heaviest call a send makes, so a public node under
     * load fails there first, having answered every cheaper request fine.
     */
    fun isNodeTrouble(t: Throwable): Boolean {
        val why = (t.message ?: "").lowercase()
        return listOf(
            "timed out", "timeout", "interfaceerror", "network error",
            "connection", "unexpected eof", "decoys",
        ).any { why.contains(it) }
    }

    /**
     * There is not enough unlocked to cover the amount and its fee.
     *
     * Typed, because the sentence it replaces was English in an app that ships
     * in nineteen languages, and because it was reporting the wrong number:
     * "not enough unlocked — 0.002315 XMR available" on a screen sending
     * 0.000978 reads as a bug in the wallet, and the reader is right that
     * something is wrong, just not about what. The fee is the missing term, so
     * both numbers travel and the screen can name it.
     */
    class NotEnough(val availablePxmr: Long, val neededPxmr: Long) :
        IllegalStateException(
            "not enough unlocked — ${formatXmr(availablePxmr)} of " +
                "${formatXmr(neededPxmr)} XMR needed with the fee",
        )

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
        val plan = plan(context, amountPxmr, priority)
        if (!plan.enough) {
            throw NotEnough(plan.totalInPxmr, amountPxmr + plan.feePxmr)
        }
        DucatLog.i(
            TAG,
            "sending ${formatXmr(amountPxmr)} XMR using ${plan.notes.size} note(s) " +
                "to ${toAddress.take(12)}…",
        )
        // The claim before the money moves — the escrow's own rule, applied
        // to every send. moneroSend builds, signs and RELAYS in one call;
        // recording only on its return meant a death in the gap left a
        // payment on chain with no local trace, which is precisely the
        // blindness the double-pay guard cannot survive. The intent also
        // pins these notes out of the next plan. A throw below does NOT
        // clear it: a timeout can post-date the relay, so only the chain
        // (refreshSpent) may decide the send never happened.
        val intent = store.recordSendIntent(
            toAddress, amountPxmr, plan.notes.map { it.keyImage }, contactHex, note,
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
            // same dying node until it re-earns its place. A read that timed
            // out is unambiguous, so it costs the node its place at once
            // instead of on the third identical failure — see nodeUnreachable.
            if (isNodeTrouble(e)) {
                NodeStore(context).nodeUnreachable()
                DucatLog.w(TAG, "node did not answer — demoted, next try re-probes")
            } else if (NodeStore(context).nodeFailed()) {
                DucatLog.w(TAG, "node demoted after repeated failures — will re-probe")
            }
            throw e
        }
        DucatLog.i(
            TAG,
            "sent ${r.txidHex.take(16)}… fee ${formatXmr(r.feePxmr.toLong())} XMR, " +
                "accepted by ${r.acceptedBy} node(s)",
        )
        // The record, the spent inputs and the intent's removal, one commit —
        // recorded here rather than on the next scan, because the outputs a
        // payment creates belong to the recipient and nothing this wallet
        // scans will ever show it happened.
        store.resolveSendIntent(intent, r.txidHex, r.feePxmr.toLong())
        return r
    }
}

/**
 * Piconero to a human string. 1 XMR is 10^12 piconero.
 *
 * **The separator follows the digits.** `%d` formats in the default locale, so
 * on an Arabic phone the digits come out Arabic-Indic — while the `.` between
 * them was a literal, and stayed ASCII. The balance screen showed
 * `USD ٣٦٫٢٣` above `٠.٠٨٣٩٠٦ XMR`: two figures side by side, disagreeing
 * about what a decimal point is, in a half-localised number that is neither
 * convention. Either whole answer would do; this is the one that matches the
 * fiat line beside it and the rule [Amounts.isNumberChar] already states —
 * *"the languages this ships in write their decimal point that way"*.
 *
 * Safe for the one place this is not merely read: [ui.Pay] prefills an
 * editable amount with it, and `Amounts.typedNumber` folds every separator
 * this could produce — and every digit shape — back to ASCII before parsing.
 */
fun formatXmr(pxmr: Long): String {
    val whole = pxmr / 1_000_000_000_000L
    val frac = pxmr % 1_000_000_000_000L
    // Six places: enough to show a stagenet dust payment, few enough to read.
    val micro = frac / 1_000_000L
    val dot = java.text.DecimalFormatSymbols.getInstance().decimalSeparator
    return if (whole == 0L && micro == 0L && pxmr > 0) "<%d%c%06d".format(0, dot, 1)
    else "%d%c%06d".format(whole, dot, micro)
}

/**
 * The same amount, to the piconero, for something that has to pay it.
 *
 * [formatXmr] is for eyes: it rounds to six places and will happily answer
 * `<0.000001`, neither of which belongs in a payment request. A kiosk order
 * identifies its own payment by the exact figure it asked for — the few
 * piconero of noise that tell one four-pound coffee from the next live in the
 * seventh decimal place and further down — so rounding the request is the same
 * as not tagging it at all, and every order sits unpaid while its customer
 * stands there having paid.
 *
 * Trailing zeroes are kept. A wallet parses the number, not its width, and
 * twelve places is what a piconero is.
 *
 * [java.util.Locale.ROOT], non-negotiably. `%d` is one of the conversions
 * Java localizes, and this app ships Persian and Arabic: on those devices the
 * default locale renders 38000235547 as ۳۸۰۰۰۲۳۵۵۴۷, and a payment request
 * carrying Persian digits is one no wallet on earth can read. (`%02x` is not
 * localized, which is why every persona and ceremony id in this codebase has
 * been safe all along.)
 */
fun exactXmr(pxmr: Long): String = "%d.%012d".format(
    java.util.Locale.ROOT, pxmr / 1_000_000_000_000L, pxmr % 1_000_000_000_000L,
)


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
            // The last rate we trusted rides along: a lone quote nothing
            // corroborates is believed only if it is close to it, and on a
            // first run with no history it is not believed at all. A refused
            // rate leaves the cache alone, which is the safe direction — the
            // screens say they have no price rather than inventing one.
            val r = uniffi.ducat_mobile.moneroRate(
                store.currency(), 12_000u, store.cached()?.first,
            )
            store.store(r.perXmr, r.fetchedAt.toLong(), r.source)
            // The dollar too, because §15.12's fare table is in dollars and a
            // dollar figure needs the dollar's rate. One extra call, and none
            // at all for somebody already reading in dollars.
            if (store.currency().equals("USD", ignoreCase = true)) {
                store.storeUsd(r.perXmr)
            } else {
                runCatching {
                    uniffi.ducat_mobile.moneroRate("USD", 12_000u, store.usdPerXmr())
                }
                    .onSuccess { store.storeUsd(it.perXmr) }
                    .onFailure { DucatLog.w(TAG, "usd rate: ${it.message}") }
            }
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
