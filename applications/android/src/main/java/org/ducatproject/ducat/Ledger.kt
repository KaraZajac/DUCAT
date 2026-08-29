package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

private const val TAG = "Ledger"

/**
 * A history, rebuilt from outputs.
 *
 * A Monero wallet does not store transactions. It stores **outputs it can
 * spend**, because that is all scanning can find, and a list of outputs is not
 * a list of payments:
 *
 *  - Two outputs of one transaction are one event.
 *  - **Change is not income.** Spending 0.010 to send 0.0025 puts 0.0074 back
 *    in your own wallet, and an outputs-list shows that as money arriving. It
 *    is the largest number on the screen and it is the wallet paying itself.
 *  - **A send leaves no local receipt at all.** The only on-chain trace is that
 *    one of your outputs stops being unspent.
 *
 * Reading outputs directly produced exactly those three errors at once: a
 * receipt and its change shown as two deposits totalling more than the wallet
 * held, and the send between them missing.
 *
 * ## How a send is identified without a local record
 *
 * Every output names the transaction that created it. Fetch that transaction
 * and look at the key images it consumed: if any of them is ours, **we sent
 * it** — no stored record required, so a payment made before the app kept
 * records is still recoverable from the chain. Then
 *
 *     paid out = (our inputs it consumed) − (our outputs in it) − fee
 *
 * which is exact, and reconciles: summing every event's net effect gives the
 * balance the Accounts screen shows. That reconciliation is the point. Two
 * screens disagreeing about the same money is not a display bug, it is the
 * wallet failing to know what it holds.
 */
object Ledger {

    enum class Direction { Received, Sent }

    /** Where a sender's name came from, if there is one. */
    enum class Source {
        /** A contact told us in the thread, naming this transaction (§16.13). */
        Notice,
        /** Our own record of having sent it. */
        OurRecord,
        /** Monero carries no sender. Nobody said. */
        Unknown,
    }

    data class Event(
        val txid: String,
        val height: Long,
        val timestamp: Long,
        val direction: Direction,
        /** What moved between this wallet and someone else. Never the change. */
        val amountPxmr: Long,
        /** Paid to the network. Sends only. */
        val feePxmr: Long,
        /** Signed effect on the wallet's total, change already netted out. */
        val netPxmr: Long,
        /** The wallet's total after this event — what Accounts would have said. */
        val balanceAfterPxmr: Long,
        val counterparty: String?,
        val address: String?,
        /**
         * The deal this movement belongs to, when it is an escrow's.
         *
         * Empty string for an escrow whose subject was never recorded — the
         * distinction that matters is escrow or not, and "an escrow" is still
         * a better answer than a raw address. Null when it is neither.
         */
        val escrow: String? = null,
        val source: Source,
        val note: String?,
        /** Our outputs created by this transaction. Includes change. */
        val ours: List<WalletEntry>,
        /** Our outputs it consumed. */
        val consumed: List<WalletEntry>,
        val chain: ChainTx?,
        /** Broadcast, not yet seen in a block. */
        val pending: Boolean,
        /** Still inside the ten-block lock. */
        val locked: Boolean,
        val unlocksInBlocks: Long,
        /**
         * True when the transaction that spent our output could not be
         * identified — the output is gone and we cannot say where.
         *
         * Shown rather than dropped. Silently omitting it would leave the
         * running balance stepping down with nothing to explain it.
         */
        val unexplained: Boolean = false,
        /**
         * The transaction has not been read from the chain yet, so "received"
         * is an assumption rather than a finding.
         *
         * It matters because the assumption is wrong precisely in the case
         * that confuses people: change from your own send arrives as an output
         * and looks exactly like income until the transaction is fetched and
         * its inputs turn out to be yours. Rather than state a direction it
         * cannot support, the row says it is still checking.
         */
        val provisional: Boolean = false,
        /** Ordering only: where this belongs in the running balance. */
        val sortHeight: Long = 0,
        /** The bill this payment answered, line by line — taken from the
         *  receipt that names this transaction (§16.13), which is the one
         *  place the itemisation and the txid meet. */
        val items: List<BillItem> = emptyList(),
        val taxPxmr: Long? = null,
        /** A receipt naming this transaction exists in some thread. */
        val receipted: Boolean = false,
        /** The other side's persona, when a thread supplied one — what makes
         *  "open the conversation" possible from a bank-statement row. */
        val contactHex: String? = null,
        /** Who issued the receipt ("you", or their name), and when. */
        val receiptBy: String? = null,
        val receiptAt: Long = 0,
    ) {
        /** Change we paid to ourselves in this transaction, if any. */
        val changePxmr: Long get() = if (direction == Direction.Sent) ours.sumOf { it.amountPxmr } else 0
    }

    /**
     * Build the history, oldest first, each row carrying the balance after it.
     *
     * Pure: it reads the store and computes. Anything needing the network is
     * [enrich], run by the poller.
     */
    fun build(context: Context): List<Event> {
        val store = WalletStore(context)
        val contacts = ContactStore(context)
        val txs = TxStore(context)
        // Read once. This used to be re-read inside the per-event loop, which
        // re-parsed the whole contact file for every row on screen.
        val everyone = contacts.all()

        // Who told us they paid us, keyed by the transaction they named. This
        // is the only way a received payment gets a name: Monero itself does
        // not carry a sender, so an unnamed receipt is honest, not a gap.
        val announced = HashMap<String, Pair<String, String?>>()
        val announcedHex = HashMap<String, String>()
        // And the paperwork: any receipt naming a transaction ties the chain
        // event to its bill — the itemisation, the tax, the thread. This is
        // what turns a row of piconero into "6 min × 0.0005, tip, receipted".
        // Receipts come from their own store, not the threads: a conversation
        // is the user's to delete, and a taxi's thread especially will be —
        // the receipt lives on in Activity the way a paper one outlives the
        // ride.
        contacts.migrateReceipts()
        val receipts = contacts.receipts()
        val papered = HashMap<String, ContactStore.ReceiptRecord>()
        // Receipts that name no transaction still name an exact amount —
        // §17.5's nomination read in reverse. Held loose and matched to the
        // chain event of the same amount in the same direction, each spent
        // once.
        class Loose(val r: ContactStore.ReceiptRecord, var spent: Boolean = false)
        val loose = ArrayList<Loose>()
        for (r in receipts) {
            val rid = r.txidHex?.lowercase()
            if (rid != null) papered[rid] = r
            // An out-of-band receipt is txid-less because the money took
            // another rail entirely — cash across the bar. There is no chain
            // event for it to match, so it never enters the loose pool, where
            // it would staple itself to an unrelated payment of the same size.
            else if (r.amountPxmr > 0 && !r.oob) loose += Loose(r)
        }
        for (c in everyone) {
            for (m in contacts.thread(c.personaHex)) {
                val id = m.txidHex?.lowercase() ?: continue
                if (!m.outgoing && m.kind == 2) {
                    announced[id] = c.displayName() to m.body.takeIf { it.isNotBlank() }
                    announcedHex[id] = c.personaHex
                }
            }
        }

        val sendsByTx = store.sends().associateBy { it.txidHex.lowercase() }
        val built = assemble(
            entries = store.entries(),
            tip = store.tip(),
            chainOf = { txs.get(it) },
            sendRecords = store.sends(),
            nameOf = { h -> everyone.firstOrNull { it.personaHex == h }?.displayName() },
            announced = announced,
        )
        // Which of these were an escrow's, and whose deal.
        //
        // Both directions read as anonymous without it. A funding shows as
        // "To 5ASXcL1JuxNY…MuXhmy", which tells nobody where six dollars
        // went, and the deposit coming home shows as "Received — sender
        // unknown" — which is *true*, Monero carries no sender, and reads as
        // a stranger sending money. This device knows both ends anyway: the
        // escrow address it paid into, and the subaddress it asked to be paid
        // back on.
        //
        // Sends match on the destination. Receipts go the other way, through
        // the minor that received the output: a ceremony's refund address is
        // allocated under the key "ride_<id>", so the reverse lookup names
        // the ceremony without guessing.
        val escrowTitle = HashMap<String, String>()
        val escrowByAddress = HashMap<String, String>()
        Ceremony.all(context).forEach { c ->
            val title = c.optString("aboutTitle")
            c.optString("id").takeIf { it.isNotEmpty() }?.let { escrowTitle[it] = title }
            c.optString("address").takeIf { it.isNotEmpty() }
                ?.let { escrowByAddress[it] = title }
        }
        // One decryption pass for the whole table, not one per output —
        // see personaByMinor. This lambda runs per row.
        val personaByMinor = store.personaByMinor()
        fun escrowOf(e: Event): String? = when (e.direction) {
            Direction.Sent -> e.address?.let { escrowByAddress[it] }
            Direction.Received -> e.ours.asSequence()
                .mapNotNull { personaByMinor[it.minor] }
                .filter { it.startsWith("ride_") }
                .mapNotNull { escrowTitle[it.removePrefix("ride_")] }
                .firstOrNull()
        }

        return built.map { e0 ->
            val e = escrowOf(e0)?.let { e0.copy(escrow = it) } ?: e0
            var paper = papered[e.txid.lowercase()]
            // The counterparty the event can already name — our send record,
            // or the notice that announced it. A loose receipt must agree
            // with it, never supply a different one.
            val knownHex = when (e.direction) {
                Direction.Sent -> sendsByTx[e.txid.lowercase()]?.contactHex
                else -> announcedHex[e.txid.lowercase()]
            }
            if (paper == null) {
                // Amount-and-direction fallback. Exact match only: a receipt
                // is a statement about a specific sum, and "close enough"
                // would staple paperwork to the wrong money. Bounded in time
                // for the same reason — a receipt and its payment happen
                // together, and among candidates the closest one wins rather
                // than whichever the list happened to serve first.
                val l = loose
                    .filter {
                        !it.spent && it.r.amountPxmr == e.amountPxmr &&
                            // A receipt someone sent us covers a payment we
                            // made; one we issued covers a payment we received.
                            it.r.mine == (e.direction == Direction.Received) &&
                            (knownHex == null || it.r.contactHex == knownHex)
                    }
                    .filter {
                        // With a known counterparty the contact match above
                        // already pins the receipt; without one, time is the
                        // only anchor there is, and waiving it for a missing
                        // timestamp let a receipt staple itself to anonymous
                        // money for ever — any same-priced transaction, years
                        // apart, first come first labelled.
                        if (knownHex != null) {
                            e.timestamp == 0L || it.r.timestamp == 0L ||
                                kotlin.math.abs(it.r.timestamp - e.timestamp) <= 86_400
                        } else {
                            e.timestamp != 0L && it.r.timestamp != 0L &&
                                kotlin.math.abs(it.r.timestamp - e.timestamp) <= 86_400
                        }
                    }
                    .minByOrNull { kotlin.math.abs(it.r.timestamp - e.timestamp) }
                if (l != null) {
                    l.spent = true
                    paper = l.r
                }
            }
            val hex = knownHex ?: paper?.contactHex
            if (paper == null && hex == null) e
            else e.copy(
                items = paper?.items ?: e.items,
                taxPxmr = paper?.taxPxmr,
                receipted = paper != null,
                contactHex = hex,
                receiptBy = paper?.let { if (it.mine) "you" else it.counterparty },
                receiptAt = paper?.timestamp ?: 0L,
            )
        }
    }

    /** A request nobody has answered yet — the "uncleared" half of a bank
     *  statement. Heuristic on purpose: a request is open until a payment at
     *  or above it follows in the same thread, which is the same matching the
     *  settlement engine discloses (§15.11). */
    data class OpenRequest(
        val theyAsked: Boolean,
        val counterparty: String,
        val contactHex: String,
        val amountPxmr: Long,
        val items: List<BillItem>,
        val timestamp: Long,
    )

    /**
     * Whether a bill has been answered — paid, receipted, withdrawn or declined.
     *
     * One place, because this question is asked on three screens and each one
     * used to answer it differently. Activity listed withdrawn and declined
     * bills under "Awaiting" for ever; the take-over prompt offered to pay a
     * bill that was already settled. A predicate copied per caller is a
     * predicate that disagrees with itself per caller.
     *
     * `m` must be the kind-1 bill; `thread` is the conversation it sits in.
     */
    fun billAnswered(thread: List<StoredMessage>, m: StoredMessage): Boolean =
        thread.any { p ->
            p.kind == 2 && p.outgoing != m.outgoing &&
                // §16.14 first, arithmetic second — the till's rule (see
                // Tabs' said-sets). A payment that names a bill answers the
                // bill it names, with no timestamp condition: the two stamps
                // come from two different clocks, and a named answer sitting
                // "before" its bill is ordinary skew, not time travel. The
                // amount-and-time arm stays for notices that predate the
                // reference — and a named notice must never fall through to
                // it, or it answers every cheaper bill in the thread too.
                if (p.reSeq != null) !p.reOwn && p.reSeq == m.seq
                else p.timestamp >= m.timestamp && p.amountPxmr >= m.amountPxmr
        } || thread.any { p ->
            // A receipt at or above it also closes it (paid outside). Named
            // receipts are exact the same way: the receipt's re_own says
            // whose log the bill lives in, which is "the sender's own" only
            // when receipt and bill come from the same side.
            p.kind == 3 &&
                if (p.reSeq != null) {
                    p.reSeq == m.seq && p.reOwn == (p.outgoing == m.outgoing)
                } else {
                    p.timestamp >= m.timestamp && p.amountPxmr >= m.amountPxmr
                }
        } || thread.any { p ->
            // §16.13's Retract closes it too. Named by sequence rather than
            // matched by amount, so it is exact.
            //
            // Both directions. `reOwn` and the same side is the issuer taking
            // their own bill back; not `reOwn` and the other side is the payer
            // refusing it.
            p.kind == 5 && p.reSeq == m.seq &&
                (if (p.reOwn) p.outgoing == m.outgoing else p.outgoing != m.outgoing)
        }

    fun openRequests(context: Context): List<OpenRequest> {
        val contacts = ContactStore(context)
        val out = ArrayList<OpenRequest>()
        for (c in contacts.all()) {
            val thread = contacts.thread(c.personaHex)
            for (m in thread) {
                if (m.kind != 1) continue
                if (!billAnswered(thread, m)) {
                    out += OpenRequest(
                        theyAsked = !m.outgoing,
                        counterparty = c.displayName(),
                        contactHex = c.personaHex,
                        amountPxmr = m.amountPxmr,
                        items = m.items,
                        timestamp = m.timestamp,
                    )
                }
            }
        }
        return out.sortedByDescending { it.timestamp }
    }

    /**
     * The arithmetic, with nothing Android in it.
     *
     * Split out so it can be tested against real transactions rather than only
     * looked at. The reconciliation this has to satisfy — every event's net
     * effect summing to the wallet's balance — is not something you can check
     * by reading the code.
     */
    internal fun assemble(
        entries: List<WalletEntry>,
        tip: Long,
        chainOf: (String) -> ChainTx?,
        sendRecords: List<SentPayment>,
        nameOf: (String?) -> String?,
        announced: Map<String, Pair<String, String?>> = emptyMap(),
    ): List<Event> {
        val sends = sendRecords.associateBy { it.txidHex.lowercase() }
        val ourKeyImages = entries.mapNotNull { it.keyImage.takeIf { k -> k.isNotEmpty() } }.toSet()
        val byKeyImage = entries.associateBy { it.keyImage }

        // By transaction — except for outputs that have no transaction id yet,
        // which each get their own row keyed by key image. Lumping those into
        // one group keyed by the empty string would merge unrelated payments
        // into a single row whose amount is their sum.
        val grouped = entries.groupBy {
            if (it.txHashHex.isEmpty()) "ki:${it.keyImage}" else it.txHashHex.lowercase()
        }

        val out = ArrayList<Event>()
        val explainedSpends = HashSet<String>()

        for ((key, group) in grouped) {
            val txid = if (key.startsWith("ki:")) "" else key
            val chain = if (txid.isEmpty()) null else chainOf(txid)
            val received = group.sumOf { it.amountPxmr }
            val height = group.minOf { it.height }
            val ts = group.maxOf { it.timestamp }

            val consumedKis = chain?.keyImages.orEmpty().filter { it in ourKeyImages }
            if (consumedKis.isNotEmpty()) {
                // Ours. The inputs it spent were ours to spend.
                val consumed = consumedKis.mapNotNull { byKeyImage[it] }
                explainedSpends += consumedKis
                val spentTotal = consumed.sumOf { it.amountPxmr }
                val fee = chain?.feePxmr ?: 0L
                val paid = (spentTotal - received - fee).coerceAtLeast(0L)
                val rec = sends[txid]
                out += Event(
                    txid = txid,
                    height = height,
                    // the send record already stores seconds; dividing again put a
                    // payment made this morning three weeks after the epoch.
                    timestamp = if (ts > 0) ts else (rec?.timestamp ?: 0L),
                    direction = Direction.Sent,
                    amountPxmr = paid,
                    feePxmr = fee,
                    netPxmr = received - spentTotal,
                    balanceAfterPxmr = 0,
                    counterparty = nameOf(rec?.contactHex),
                    address = rec?.toAddress,
                    source = if (rec != null) Source.OurRecord else Source.Unknown,
                    note = rec?.note,
                    ours = group,
                    consumed = consumed,
                    chain = chain,
                    pending = false,
                    locked = false,
                    unlocksInBlocks = 0,
                    sortHeight = height,
                )
            } else {
                val named = announced[txid]
                out += Event(
                    txid = txid,
                    height = height,
                    timestamp = ts,
                    direction = Direction.Received,
                    amountPxmr = received,
                    feePxmr = 0,
                    netPxmr = received,
                    balanceAfterPxmr = 0,
                    counterparty = named?.first,
                    address = null,
                    source = if (named != null) Source.Notice else Source.Unknown,
                    note = named?.second,
                    ours = group,
                    consumed = emptyList(),
                    chain = chain,
                    pending = false,
                    locked = tip > 0 && height > 0 && height + LOCK_BLOCKS > tip,
                    unlocksInBlocks = (height + LOCK_BLOCKS - tip).coerceAtLeast(0),
                    // An unread transaction cannot be called a receipt yet —
                    // *unless* we can already rule out the other thing it
                    // might be. The hedge exists because an output that
                    // arrived could be change from our own send, and until the
                    // transaction is fetched the output alone does not say.
                    //
                    // But our own sends are recorded right here, and `sends`
                    // is keyed by their transaction. An output from a
                    // transaction we never sent is not our change, and no
                    // amount of waiting for the chain will make it so. Without
                    // this, somebody who had never sent anything in their life
                    // was told that every payment they received "may be change
                    // from your own payment" — on a slow phone, for as long as
                    // the fetch took, and offline, for ever.
                    provisional = txid.isNotEmpty() && chain == null &&
                        sends.containsKey(txid),
                    sortHeight = height,
                )
            }
        }

        // Outputs that are gone with nothing to account for them. Either the
        // transaction has not been fetched yet, or it spent everything and left
        // no change for the scanner to find. Either way the money left, and the
        // running balance has to step down somewhere visible.
        for (e in entries.filter { it.spent && it.keyImage !in explainedSpends }) {
            // No attempt to guess which local record this was. An earlier
            // version matched on `amount + fee == output`, which is only true
            // when a send produced no change — so it was arithmetic that looked
            // like identification and was wrong in the ordinary case.
            out += Event(
                txid = "",
                // The output was *created* at e.height and spent later, so its
                // own height is not where this belongs in the history. Placing
                // it one block on at least keeps it after its own receipt; the
                // real height arrives with the transaction.
                height = 0,
                timestamp = 0,
                direction = Direction.Sent,
                amountPxmr = e.amountPxmr,
                feePxmr = 0,
                netPxmr = -e.amountPxmr,
                balanceAfterPxmr = 0,
                counterparty = null,
                address = null,
                source = Source.Unknown,
                note = null,
                ours = emptyList(),
                consumed = listOf(e),
                chain = null,
                pending = false,
                locked = false,
                unlocksInBlocks = 0,
                unexplained = true,
                sortHeight = e.height + 1,
            )
        }

        // Broadcast but not yet on chain: our own record is the only evidence.
        val onChain = out.map { it.txid }.filter { it.isNotEmpty() }.toSet()

        // A spend we can see on chain but cannot attribute already accounts for
        // the money, so a local record must not add a second row for it. The
        // two describe one payment: the observed spend has the arithmetic (the
        // whole note left the spendable pool, change not yet seen) and our
        // record has the meaning (who, how much, what for). Keep the row that
        // has the numbers and give it the record's labels — dropping the record
        // outright is what made a payment you had just sent, by name, read as a
        // red "Spent, but we cannot say where" until the chain caught up.
        val pendingRecords = sendRecords
            .filter { it.txidHex.lowercase() !in onChain }
            .sortedByDescending { it.timestamp }
        val unattributedIdx = out.indices
            .filter { out[it].unexplained }
            .sortedByDescending { out[it].sortHeight }
            .toMutableList()
        // Newest record to newest spend, but only where the arithmetic
        // permits: the observed spend is a whole note leaving, and a note
        // cannot have paid a bill bigger than itself plus nothing — the
        // amount and the fee both came out of it. Purely positional pairing
        // put a small tip's label on a large rent payment whenever two were
        // in flight, and the sums beside the name belonged to neither.
        val leftover = ArrayList<SentPayment>()
        for (s in pendingRecords) {
            val slot = unattributedIdx.indexOfFirst {
                out[it].amountPxmr >= s.amountPxmr + s.feePxmr
            }
            if (slot < 0) { leftover += s; continue }
            val i = unattributedIdx.removeAt(slot)
            out[i] = out[i].copy(
                txid = s.txidHex,
                timestamp = s.timestamp,
                // What was paid, not what left the spendable pool. Monero
                // spends whole notes, so the observed outflow is the note —
                // and printing that beside a person's name would say you sent
                // them the change too. `netPxmr` is left alone, so the running
                // balance still steps down by what actually moved; the two
                // reconcile when the change is scanned back in.
                amountPxmr = s.amountPxmr,
                feePxmr = s.feePxmr,
                counterparty = nameOf(s.contactHex),
                address = s.toAddress,
                source = Source.OurRecord,
                note = s.note,
                contactHex = s.contactHex,
                // It is ours and in flight, not a mystery: the row says
                // "sending" rather than accusing the wallet of losing track.
                unexplained = false,
                pending = true,
            )
        }

        for (s in leftover) {
            out += Event(
                txid = s.txidHex,
                height = 0,
                timestamp = s.timestamp,
                direction = Direction.Sent,
                amountPxmr = s.amountPxmr,
                feePxmr = s.feePxmr,
                // Nothing has moved on chain yet as far as the wallet can see,
                // so claiming a balance change here would double-count the
                // moment the spend is observed.
                netPxmr = 0,
                balanceAfterPxmr = 0,
                counterparty = nameOf(s.contactHex),
                address = s.toAddress,
                source = Source.OurRecord,
                note = s.note,
                ours = emptyList(),
                consumed = emptyList(),
                chain = null,
                pending = true,
                locked = false,
                unlocksInBlocks = 0,
                sortHeight = Long.MAX_VALUE,
            )
        }

        // Oldest first to accumulate, newest first to display.
        val ordered = out.sortedWith(compareBy({ it.sortHeight }, { it.timestamp }))
        var running = 0L
        val withBalance = ordered.map {
            running += it.netPxmr
            it.copy(balanceAfterPxmr = running)
        }
        return withBalance.reversed()
    }

    /**
     * Fill in what only the network can answer: which transactions we sent, and
     * when each block was mined.
     *
     * Bounded per call. This runs on the poll loop, and an unbounded backfill on
     * a wallet with a long history is a phone that stops answering.
     */
    fun enrich(context: Context, node: String, budget: Int = 6): Boolean {
        val store = WalletStore(context)
        val txs = TxStore(context)
        var changed = false

        // Cheapest first, and it needs no network at all: the transaction id was
        // already inside the blob the wallet kept in order to be able to spend.
        // A wallet that scanned before the field existed does not have to read
        // the chain again for it.
        val entries = store.entries()
        if (entries.any { it.txHashHex.isEmpty() && it.blob.isNotEmpty() }) {
            var recovered = 0
            // Re-read inside the lock rather than writing back the list read
            // above: a scan landing in between would otherwise be erased by
            // this backfill, taking a received output with it.
            //
            // And it returns null when nothing was recovered, because writing
            // unconditionally meant a blob that would not parse rewrote
            // identical data and raised the change flag every ten seconds —
            // every screen watching the store redrawing forever over nothing.
            store.mutateEntries { current ->
                var got = 0
                val filled = current.map { e ->
                    if (e.txHashHex.isNotEmpty() || e.blob.isEmpty()) e
                    else runCatching {
                        val id = uniffi.ducat_mobile.moneroOutputMeta(e.blob).txHashHex
                        if (id.isNotEmpty()) got++
                        e.copy(txHashHex = id)
                    }.getOrElse { e }
                }
                recovered = got
                if (got > 0) filled else null
            }
            if (recovered > 0) {
                DucatLog.i(TAG, "recovered $recovered transaction id(s) from stored outputs")
                changed = true
            }
        }

        var spent = 0
        for (txid in store.entries().map { it.txHashHex.lowercase() }.distinct()) {
            if (spent >= budget) break
            if (txid.isEmpty() || txs.get(txid) != null) continue
            spent++
            runCatching { uniffi.ducat_mobile.moneroTxDetails(node, txid) }
                .onSuccess { txs.put(ChainTx.of(it)); changed = true }
                .onFailure { DucatLog.w(TAG, "tx $txid: ${it.message}") }
        }

        // Block times, for the outputs that have none.
        val needTime = store.entries().filter { it.timestamp == 0L && it.height > 0 }
            .map { it.height }.distinct().take((budget - spent).coerceAtLeast(0))
        if (needTime.isNotEmpty()) {
            val times = HashMap<Long, Long>()
            for (h in needTime) {
                runCatching { uniffi.ducat_mobile.moneroBlockTime(node, h.toULong()) }
                    .onSuccess { times[h] = it.toLong() }
                    .onFailure { DucatLog.w(TAG, "block $h time: ${it.message}") }
            }
            if (times.isNotEmpty()) {
                store.mutateEntries { current ->
                    current.map {
                        if (it.timestamp == 0L) it.copy(timestamp = times[it.height] ?: 0L) else it
                    }
                }
                DucatLog.i(TAG, "filled in ${times.size} block time(s)")
                changed = true
            }
        }
        if (changed) ContactStore.bump()
        return changed
    }

    /**
     * The whole ledger as CSV — the statement this screen never gave back.
     *
     * A shop that has taken money through the till all year has had no way to
     * get its takings out for tax; the app held every figure and returned
     * none of them. One row per settled event, machine-readable on purpose:
     *
     * - Numbers are XMR with a plain ASCII decimal point, never the display
     *   formatter — `formatXmr` localises its digits, and a CSV in Persian
     *   numerals with comma decimals is a file no spreadsheet can sum.
     * - Timestamps are ISO 8601 UTC, because "27/08/26" is three different
     *   days in three countries and an accountant gets no chance to ask.
     * - **No fiat column.** Bills are settled in piconero and no historical
     *   rate is stored; converting at today's rate would print numbers that
     *   were never true on the day. The XMR figures and the dates are the
     *   facts; valuation is the accountant's job, with their jurisdiction's
     *   own rate source.
     * - Tax is its own column — the till stamps it per sale (see [Tax]) and
     *   this is the half a business actually files.
     */
    fun exportCsv(context: android.content.Context): String {
        fun xmr(pxmr: Long): String =
            java.math.BigDecimal(pxmr).movePointLeft(12).toPlainString()
        fun esc(v: String): String =
            if (v.any { it == ',' || it == '"' || it == '\n' || it == '\r' })
                "\"" + v.replace("\"", "\"\"") + "\""
            else v
        val fmt = java.time.format.DateTimeFormatter.ISO_INSTANT
        val sb = StringBuilder()
        sb.append("date_utc,direction,counterparty,note,items,amount_xmr,fee_xmr,")
        sb.append("net_xmr,tax_xmr,txid,height,balance_after_xmr\n")
        // Oldest first: a statement reads forward, and the running balance
        // column only adds up in the order the money moved.
        for (e in build(context).asReversed()) {
            if (e.pending) continue
            sb.append(esc(fmt.format(java.time.Instant.ofEpochSecond(e.timestamp)))).append(',')
            sb.append(if (e.direction == Direction.Sent) "out" else "in").append(',')
            sb.append(esc(e.counterparty ?: "")).append(',')
            sb.append(esc(e.note ?: "")).append(',')
            sb.append(esc(e.items.joinToString("; ") {
                "${it.description} ${xmr(it.amountPxmr)}"
            })).append(',')
            sb.append(xmr(e.amountPxmr)).append(',')
            sb.append(xmr(e.feePxmr)).append(',')
            sb.append(xmr(e.netPxmr)).append(',')
            sb.append(xmr(e.taxPxmr ?: 0L)).append(',')
            sb.append(esc(e.txid)).append(',')
            sb.append(e.height).append(',')
            sb.append(xmr(e.balanceAfterPxmr)).append('\n')
        }
        return sb.toString()
    }
}

/** What the chain says about one transaction. Cached: it never changes. */
data class ChainTx(
    val txid: String,
    val version: Int,
    val feePxmr: Long,
    val keyImages: List<String>,
    val inputCount: Int,
    val outputCount: Int,
    val ringSize: Int,
    val additionalTimelock: Long,
    val extraLen: Int,
    val coinbase: Boolean,
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("txid", txid); put("v", version); put("fee", feePxmr)
        put("ki", JSONArray(keyImages)); put("in", inputCount); put("out", outputCount)
        put("ring", ringSize); put("lock", additionalTimelock)
        put("extra", extraLen); put("cb", coinbase)
    }

    companion object {
        fun of(d: uniffi.ducat_mobile.TxDetails) = ChainTx(
            txid = d.txHashHex.lowercase(),
            version = d.version.toInt(),
            feePxmr = d.feePxmr.toLong(),
            keyImages = d.keyImagesHex,
            inputCount = d.inputCount.toInt(),
            outputCount = d.outputCount.toInt(),
            ringSize = d.ringSize.toInt(),
            additionalTimelock = d.additionalTimelock.toLong(),
            extraLen = d.extraLen.toInt(),
            coinbase = d.coinbase,
        )

        fun from(o: JSONObject): ChainTx {
            val kis = o.optJSONArray("ki") ?: JSONArray()
            return ChainTx(
                txid = o.getString("txid"),
                version = o.optInt("v", 2),
                feePxmr = o.optLong("fee", 0L),
                keyImages = (0 until kis.length()).map { kis.getString(it) },
                inputCount = o.optInt("in", 0),
                outputCount = o.optInt("out", 0),
                ringSize = o.optInt("ring", 0),
                additionalTimelock = o.optLong("lock", 0L),
                extraLen = o.optInt("extra", 0),
                coinbase = o.optBoolean("cb", false),
            )
        }
    }
}

/**
 * Transactions we have already looked up.
 *
 * Cached because a confirmed transaction is immutable — re-fetching it every
 * time the Activity screen redraws would be one network round trip per row per
 * recomposition.
 */
class TxStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

    fun get(txid: String): ChainTx? =
        prefs.getString("tx_${txid.lowercase()}", null)?.let {
            runCatching { ChainTx.from(JSONObject(it)) }.getOrNull()
        }

    fun put(tx: ChainTx) {
        prefs.edit().putString("tx_${tx.txid}", tx.toJson().toString()).apply()
    }


}
