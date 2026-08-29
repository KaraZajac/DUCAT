package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

private const val TAG = "Tabs"

/**
 * A running account with one person, settled as one bill (§15.11).
 *
 * The state lives here, on the payee's device — the network sees nothing while
 * lines accumulate, and one itemised request when the tab settles. The thread
 * is the tab's identity: opened by a card claim or by picking an existing
 * contact (which is what a regular is), and still reachable after the customer
 * has gone home, which is §16.12's whole reason to exist.
 *
 * A taxi ride is the same object with one line and a different origin: a
 * settlement waiting for its payment. Sharing the store means the poller's
 * reconcile loop — payment seen, receipt sent — is written once.
 */
data class RunningTab(
    val id: String,
    /** "bar" or "taxi" — only for display; the machinery is identical. */
    val origin: String,
    val personaHex: String,
    val openedAt: Long,
    val lines: List<BillItem>,
    val taxPxmr: Long?,
    /** open → settled (billed, unpaid) → paid | paid_oob | cancelled. */
    val state: String,
    val settledTotal: Long = 0,
    val settledAt: Long = 0,
    /** Key images already in the wallet when the bill went out, so an old
     *  output can never be mistaken for this payment. */
    val knownKis: List<String> = emptyList(),
    /** The output that settled it, so no other tab can claim the same one. */
    val paidKi: String? = null,
    /** A matching transaction sighted in the mempool — §17.5's *seen*, not
     *  settled. UX state only: the receipt waits for the chain. */
    val seenTx: String? = null,
    /** The chain tip when the bill went out. The key-image snapshot only
     *  covers outputs the scanner had already reached; an output mined at or
     *  below this height existed before the bill, however late a catch-up
     *  scan surfaces it, and cannot be its payment. */
    val tipAtBill: Long = 0,
    /**
     * The subaddress the bill named as the place to pay, so only an output
     * that landed *there* can settle it.
     *
     * Matching used to admit minor 0 as well — the wallet's main address —
     * because bills predating per-contact addressing named no address and
     * were paid there. But minor 0 is also where donations, top-ups and
     * escrow payouts arrive, and a payer names the amount, so an unrelated
     * payment of the right size closed somebody else's tab and posted them a
     * receipt for goods they never paid for. Every bill now carries the minor
     * it billed to; null means a tab settled before this field existed, and
     * only those keep the old permissive rule.
     */
    val billedMinor: Int? = null,
    /**
     * The sequence of the bill this tab sent, in our own outbox.
     *
     * Recorded because two places used to recover it by searching the thread
     * for "the last outgoing bill with this total", which is a guess that a
     * second identical bill makes wrong. The receipt names it (§16.14), and
     * so do the two screens that need to know whether the customer refused
     * it. Zero for a tab settled before this was kept.
     */
    val billSeq: Long = 0,
    /**
     * What actually arrived, which is the bill plus whatever was tipped on
     * top of it. Zero until the payment is matched, and on tabs written
     * before this field existed.
     *
     * Kept apart from [settledTotal] rather than replacing it: what was
     * *billed* is what the customer holds on paper and what reconciliation
     * matches against, and neither may drift. But the takings are what came
     * in, and reconcile knew the tip — it put it on the receipt and in the
     * notification — and then dropped it. So the day's total on the sales
     * screen was the sum of the bills, and a shop where tipping is most of
     * the margin read its own till short by every tip it took.
     */
    val paidPxmr: Long = 0,
) {
    val totalPxmr: Long get() =
        if (state == "open") lines.sumOf { it.amountPxmr } + (taxPxmr ?: 0L) else settledTotal

    /** What this tab actually brought in — the bill, or the payment when one
     *  has landed and may have carried a tip. */
    val takePxmr: Long get() = if (paidPxmr > 0) paidPxmr else totalPxmr

    /** Paid over the bill. Zero unless a payment has landed above it. */
    val tipPxmr: Long get() = (paidPxmr - settledTotal).coerceAtLeast(0)

    fun toJson(): JSONObject = JSONObject().apply {
        put("id", id); put("origin", origin); put("persona", personaHex)
        put("opened", openedAt); put("state", state)
        put("total", settledTotal); put("settled_at", settledAt)
        put("tax", taxPxmr ?: JSONObject.NULL)
        put("lines", JSONArray().also { a ->
            lines.forEach { a.put(JSONObject().put("d", it.description).put("a", it.amountPxmr)) }
        })
        put("known", JSONArray(knownKis))
        put("paid_ki", paidKi ?: JSONObject.NULL)
        put("seen_tx", seenTx ?: JSONObject.NULL)
        put("tip_at_bill", tipAtBill)
        put("paid_total", paidPxmr)
        billedMinor?.let { put("billed_minor", it) }
        if (billSeq > 0) put("bill_seq", billSeq)
    }

    companion object {
        fun from(o: JSONObject) = RunningTab(
            id = o.getString("id"),
            origin = o.optString("origin", "bar"),
            personaHex = o.getString("persona"),
            openedAt = o.getLong("opened"),
            state = o.getString("state"),
            settledTotal = o.optLong("total", 0),
            settledAt = o.optLong("settled_at", 0),
            taxPxmr = if (o.isNull("tax")) null else o.getLong("tax"),
            lines = o.getJSONArray("lines").let { a ->
                (0 until a.length()).map {
                    val i = a.getJSONObject(it)
                    BillItem(i.getString("d"), i.getLong("a"))
                }
            },
            knownKis = o.optJSONArray("known")?.let { a ->
                (0 until a.length()).map { a.getString(it) }
            } ?: emptyList(),
            paidKi = if (o.isNull("paid_ki")) null else o.optString("paid_ki"),
            seenTx = if (o.isNull("seen_tx")) null else o.optString("seen_tx"),
            tipAtBill = o.optLong("tip_at_bill", 0),
            paidPxmr = o.optLong("paid_total", 0),
            billedMinor = o.optInt("billed_minor", -1).takeIf { it >= 0 },
            billSeq = o.optLong("bill_seq", 0L),
        )
    }
}

class TabStore(private val context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")


    fun all(): List<RunningTab> {
        val raw = prefs.getString("tabs_v1", null) ?: return emptyList()
        return runCatching {
            val a = JSONArray(raw)
            (0 until a.length()).map { RunningTab.from(a.getJSONObject(it)) }
        }.getOrElse { emptyList() }
    }

    fun get(id: String): RunningTab? = all().firstOrNull { it.id == id }

    fun open(personaHex: String, origin: String): RunningTab = guarded {
        val t = RunningTab(
            id = java.util.UUID.randomUUID().toString(),
            origin = origin,
            personaHex = personaHex,
            openedAt = System.currentTimeMillis(),
            lines = emptyList(),
            taxPxmr = null,
            state = "open",
        )
        save(all() + t)
        t
    }

    fun update(t: RunningTab) = guarded { save(all().map { if (it.id == t.id) t else it }) }

    /**
     * Change one tab, from whatever it says *now* rather than from a snapshot.
     *
     * [update] takes a whole tab, so it writes back every field — including the
     * ones somebody else changed while the caller was holding its copy. A tab
     * has two writers who cannot see each other: the till, in the bartender's
     * hands, and the reconciler, on a background thread with seconds of network
     * between reading a tab and writing it back. Both of the orderings lost
     * money.
     *
     * A drink poured while the receipt was going out was erased by the
     * reconciler's stale copy landing after it — the bar served it and never
     * charged for it. And a drink added just after a tab was marked paid put
     * `state` back to "open" and `paidKi` back to null, which sounds like the
     * lesser bug and is the worse one: the key image had already been recorded
     * as claimed, so the scan would never match that payment again, and the tab
     * stayed open forever on a bill that was actually settled.
     *
     * So each writer touches only its own fields, and reads them from the tab
     * as it stands inside the lock. Returns the new tab, or null if it is gone
     * — deleted while the caller was working, which is not an error.
     */
    fun mutate(id: String, f: (RunningTab) -> RunningTab): RunningTab? = guarded {
        val cur = all()
        val i = cur.indexOfFirst { it.id == id }
        if (i < 0) return@guarded null
        val next = f(cur[i])
        save(cur.mapIndexed { j, t -> if (j == i) next else t })
        next
    }

    fun delete(id: String) = guarded { save(all().filterNot { it.id == id }) }

    /**
     * Key images ever matched to a bill, kept apart from the tabs themselves:
     * deleting a paid tab must not release its output back into the matching
     * pool, or a still-billed tab for the same amount could later "find" a
     * payment that already bought someone else's evening.
     */
    fun claimedKis(): Set<String> =
        prefs.getStringSet("claimed_kis_v1", emptySet()) ?: emptySet()

    fun addClaimedKi(ki: String) = guarded {
        prefs.edit().putStringSet("claimed_kis_v1", claimedKis() + ki).apply()
    }

    /**
     * Paid-state and the claimed key image, one commit.
     *
     * As two writes, a death between them left the output covered only by
     * the tab's own paidKi — and deleting that paid tab then released a
     * spent output back into the matching pool, the exact leak the claimed
     * set's docstring forbids. One editor closes the gap for good.
     */
    fun markPaid(id: String, ki: String, paidPxmr: Long): RunningTab? = guarded {
        val cur = all()
        val i = cur.indexOfFirst { it.id == id }
        if (i < 0) return@guarded null
        val next = cur[i].copy(state = "paid", paidKi = ki, paidPxmr = paidPxmr)
        prefs.edit()
            .putString(
                "tabs_v1",
                org.json.JSONArray().also { a ->
                    cur.mapIndexed { j, t -> if (j == i) next else t }
                        .forEach { a.put(it.toJson()) }
                }.toString(),
            )
            .putStringSet("claimed_kis_v1", claimedKis() + ki)
            .apply()
        ContactStore.bump()
        next
    }

    /**
     * Bill the tab: one itemised request into the thread, then wait for chain.
     *
     * The wallet's current key images are snapshotted here so reconciliation
     * can never match an output that predates the bill.
     */
    fun settle(tab: RunningTab): RunningTab {
        val contact = ContactStore(context).all().firstOrNull { it.personaHex == tab.personaHex }
            ?: throw IllegalStateException("that contact is gone")
        val total = tab.lines.sumOf { it.amountPxmr } + (tab.taxPxmr ?: 0L)
        val wallet = WalletStore(context)
        // Held, because what the bill names is what the tab has to be settled
        // by, and the two must not be derived separately.
        val payto = wallet.addressFor(tab.personaHex)
        // Settled BEFORE the bill leaves — markPaidOutside's rule, applied
        // here at last. Bill-first meant a death in the gap left a customer
        // holding a real bill against a tab still reading "open": both
        // reconcilers filter on settled, so their payment could never match,
        // and the bartender's natural next tap billed the drinks again. The
        // bill's own seq cannot exist yet, so it lands in a second write
        // just after the send; a death between the two costs only the seq
        // (the amount-match fallback still finds the bill), never the state.
        mutate(tab.id) {
            it.copy(
                state = "settled",
                lines = tab.lines,
                taxPxmr = tab.taxPxmr,
                settledTotal = total,
                settledAt = System.currentTimeMillis(),
                knownKis = wallet.entries().map { e -> e.keyImage },
                tipAtBill = wallet.tip(),
            )
        } ?: throw IllegalStateException("that tab is gone")
        Mailbox.send(
            context, contact,
            // The shop's own language, not the reader's. This line is the
            // shop speaking — a paper receipt from a Berlin cafe is in
            // German — and the sender cannot know what the payer reads
            // anyway. Every label around it is already localised for them.
            context.getString(
                when (tab.origin) {
                    "taxi" -> R.string.bill_note_fare
                    Orders.ORIGIN -> R.string.bill_note_order
                    // A counter sale is not a tab. Only the bar opens one of
                    // those; the till, the taxi and the kiosk each hand over
                    // something with its own name, and "Your tab" was what a
                    // shop's customer saw above a flat white they had already
                    // paid for at the counter.
                    "pos" -> R.string.bill_note_sale
                    else -> R.string.bill_note_tab
                },
            ),
            PersonaStore(context).personaHex(),
            kind = 1, amountPxmr = total,
            payto = payto,
            items = tab.lines, taxPxmr = tab.taxPxmr,
        )
        // Pinned to what was billed, not to what the tab says now: the customer
        // has the itemised request in their hand, and a drink poured between
        // that going out and this landing is not on it. The tab must agree with
        // the paper, so the lines and the total travel together.
        // The bill's own sequence, read back rather than derived: `send`
        // re-reads the contact and takes the seq it finds, so a counter held
        // from before the call can be one behind the message it just sent.
        val billSeq = ContactStore(context).thread(tab.personaHex)
            .lastOrNull { it.outgoing && it.kind == 1 }?.seq ?: 0L
        val settled = mutate(tab.id) {
            it.copy(
                billSeq = billSeq,
                // The minor of the address the bill above **actually named** —
                // not the one allocation happened to reserve.
                //
                // Recorded rather than derived at match time, so what settles
                // the tab is checked against what the customer was told to
                // pay. And checked against `payto`, because `addressFor`
                // allocates a minor *before* it can still fall back to the
                // main address — a subaddress it could not derive is an
                // allocated index nobody was given, and recording that would
                // leave a bill demanding payment at an address that was never
                // on it, unsettleable for ever. Null then, which is the
                // permissive rule, and correct: minor 0 is where it will land.
                // The settle state itself was committed before the send; this
                // second write adds only what the send could tell us.
                billedMinor = wallet.minorOf(tab.personaHex)
                    ?.takeIf { payto != null && payto != wallet.address() },
            )
        } ?: throw IllegalStateException("that tab is gone")
        DucatLog.i(TAG, "settled ${tab.origin} tab with ${contact.displayName()}: ${formatXmr(total)} XMR")
        return settled
    }

    /**
     * The bill was settled outside DUCAT — cash across the bar, a card.
     *
     * Still gets a receipt, because the customer's record should not depend on
     * which rail the money took: a `RECEIPT` without a transaction is legal on
     * the wire (the txid is optional) and simply says what the payee is
     * acknowledging. Best-effort — an offline node must not stop the bartender
     * from closing out the night, so the tab is marked first and the receipt
     * failure is logged rather than blocking.
     */
    fun markPaidOutside(tab: RunningTab) {
        // Cash across the bar settles the bill exactly; there is no chain
        // output to read a tip from.
        update(tab.copy(state = "paid_oob", paidPxmr = tab.settledTotal))
        val contact = ContactStore(context).all()
            .firstOrNull { it.personaHex == tab.personaHex } ?: return
        runCatching {
            Mailbox.send(
                context, contact, context.getString(R.string.receipt_note_oob),
                PersonaStore(context).personaHex(),
                kind = 3, amountPxmr = tab.settledTotal,
                // §16.14: the request this receipts. `reOwn`, because the
                // party issuing a receipt is the party that sent the bill.
                reSeq = tab.billSeq.takeIf { it > 0 }, reOwn = tab.billSeq > 0,
                items = tab.lines, taxPxmr = tab.taxPxmr,
                // The money took another rail; the record must say so, or the
                // ledger goes looking for a chain event that does not exist.
                oob = true,
            )
        }.onFailure { DucatLog.w(TAG, "oob receipt: ${it.message}") }
        DucatLog.i(TAG, "${tab.origin} tab settled outside DUCAT (${formatXmr(tab.settledTotal)} XMR)")
    }

    /**
     * Withdraw the bill.
     *
     * The counterparty's client still holds an actionable request — a
     * "Review payment" button pointing at money the payee no longer expects —
     * so cancelling MUST say so in the thread (§15.11), or they can pay a
     * bill nobody is watching for. The message is the cancellation; the state
     * change just stops reconciliation matching it.
     */
    fun cancel(tab: RunningTab) {
        update(tab.copy(state = "cancelled"))
        val contact = ContactStore(context).all()
            .firstOrNull { it.personaHex == tab.personaHex } ?: return
        runCatching {
            Mailbox.send(
                context, contact,
                // The shop's language, like the bill and the receipt. This
                // one was missed when those were done — its literal ran past
                // the length the sweep was looking for.
                context.getString(
                    R.string.bill_note_cancelled,
                    Amounts.show(context, tab.settledTotal).primary,
                ),
                PersonaStore(context).personaHex(),
            )
        }.onFailure { DucatLog.w(TAG, "cancel notice: ${it.message}") }
        DucatLog.i(TAG, "${tab.origin} tab cancelled (${formatXmr(tab.settledTotal)} XMR)")
    }

    companion object {
        /**
         * Guards read-modify-write of the tab list.
         *
         * The poller reconciles payments and sights the mempool on its own
         * thread while a screen opens, settles or cancels a tab on another.
         * Both rewrite the whole array, so without this the loser wrote from a
         * list read before the winner's change — and the states that get lost
         * are the expensive ones: a tab marked `paid`, with the key image that
         * settled it, reverting to `settled` and becoming eligible to match a
         * second payment.
         *
         * Here rather than on the instance, like ContactStore's, because
         * callers make a fresh TabStore per operation.
         */
        private val lock = Any()

        internal fun <T> guarded(f: () -> T): T = synchronized(lock) { f() }

        /**
         * Payment seen → receipt sent, for every settled tab (§15.11).
         *
         * Runs on the poller rather than a screen, because the payment lands
         * when it lands — the bartender is pouring, the driver is driving, and
         * a receipt that depends on somebody watching a screen is a receipt
         * that does not get sent.
         *
         * Oldest-settled-first, one output per tab, never an output the tab
         * already knew: §15.11's matching rules. Two tabs settled for the same
         * amount in the same window remain genuinely ambiguous — the spec says
         * so — and oldest-first is the disclosed tie-break rather than a
         * hidden guess.
         */
        /**
         * Look for settled bills in the mempool (§17.5's *seen*).
         *
         * Pure UX: the customer's payment left their phone seconds ago, and a
         * till that stares blankly for two minutes reads as broken even while
         * being exactly right. The sighting flips the screen to "settling";
         * the receipt and the paid state still wait for the chain, because an
         * unconfirmed transaction is a claim, not a settlement — accepting
         * one on sight is §8.6's bonded mode, which this is not.
         */
        /**
         * Did this output land where the bill asked for it?
         *
         * A tab that recorded the subaddress it billed to is settled by that
         * subaddress and nothing else. Minor 0 — the wallet's main address —
         * used to be admissible for every tab, and it is where donations,
         * top-ups and escrow releases all arrive: with a payer-named amount
         * beside it, money that had nothing to do with a bill could close it
         * and send the customer a receipt. Tabs written before the field
         * existed keep the old rule, and empty themselves within a day.
         */
        fun paidWhereBilled(tab: RunningTab, wantMinor: Int?, minor: Int): Boolean =
            if (tab.billedMinor != null) minor == tab.billedMinor
            else minor == 0 || wantMinor == null || minor == wantMinor

        fun poolSight(context: Context, node: String) {
            val store = TabStore(context)
            val waiting = store.all().filter { it.state == "settled" && it.seenTx == null }
            if (waiting.isEmpty()) return
            val spend = WalletStore(context).spendKeyHex() ?: return
            val hits = runCatching {
                uniffi.ducat_mobile.moneroScanPool(node, spend, 40u,
                    WalletStore(context).subaddressCount().toUInt())
            }.getOrElse { return }
            if (hits.isEmpty()) return
            // Our own change is in the mempool too, and it is an output to us
            // like any other. Sighting it would tell the payer their money was
            // on its way when what was on its way was our own.
            val ours = WalletStore(context).ourTxids()
            val contacts = ContactStore(context)
            // One transaction sights one tab. Orders.poolSight has kept this
            // set since it was written, and says why: "noise collides
            // eventually, and when it does the same transaction would
            // otherwise mark two orders paid and hand somebody a free one."
            // The till had the same loop without the set.
            //
            // A coffee stall is where it bites, because a stall's whole menu
            // is a handful of identical prices: two customers order the same
            // thing, both tabs settle at the same total, one of them pays, and
            // the single mempool hit sighted both. Kiosk renders anything past
            // Awaiting as paid and offers staff the Ready button on `Seen`, so
            // the second customer walks out with it. Chain reconciliation does
            // catch up — the key image is claimed once, so the second tab never
            // reaches `paid` — but the goods left the counter at `Seen`, which
            // is the entire point of kiosk mode.
            val claimedTx = store.all().mapNotNull { it.seenTx }.toMutableSet()
            for (tab in waiting.sortedBy { it.settledAt }) {
                // Notices about *this* bill, not every amount this person has
                // ever claimed to send. reconcile has had the window since it
                // was written; the mempool sighting beside it did not, so a
                // notice from a visit last month still widened what would
                // match — and a payer chooses both the amount and the
                // timestamp, so without the bound they choose the set.
                val said = contacts.thread(tab.personaHex)
                    .filter {
                        !it.outgoing && it.kind == 2 &&
                            it.timestamp * 1000 >= tab.settledAt - 60_000 &&
                            it.amountPxmr >= tab.settledTotal &&
                            // §16.14: a notice that names a bill names *its*
                            // bill. Found live: two billed tabs for one
                            // person, and the older swallowed the newer's
                            // payment because the amount fit its window —
                            // while the notice was pointing at the other
                            // bill the whole time. A named notice widens
                            // only the tab it names; an unnamed one keeps
                            // the old behaviour.
                            (it.reSeq == null ||
                                (!it.reOwn && tab.billSeq > 0 && it.reSeq == tab.billSeq))
                    }
                    .map { it.amountPxmr }.toSet()
                // The same subaddress rule reconcile applies. It matters more
                // here, not less: kiosk mode renders anything past Awaiting as
                // paid and offers staff the Ready button on `Seen`, so a
                // sighting is what goods leave the counter on. Matching on
                // amount alone let a donation of the right size do that.
                val wantMinor = tab.billedMinor ?: WalletStore(context).minorOf(tab.personaHex)
                val hit = hits.firstOrNull {
                    it.txHashHex !in claimedTx &&
                        it.txHashHex.lowercase() !in ours &&
                        paidWhereBilled(tab, wantMinor, it.minor.toInt()) &&
                        (it.amountPxmr.toLong() == tab.settledTotal ||
                            it.amountPxmr.toLong() in said)
                } ?: continue
                claimedTx += hit.txHashHex
                store.mutate(tab.id) { it.copy(seenTx = hit.txHashHex) }
                Notify.post(
                    context,
                    context.getString(R.string.notify_seen_title),
                    context.getString(
                        R.string.notify_seen_body,
                        Amounts.show(context, hit.amountPxmr.toLong()).primary,
                    ),
                )
                DucatLog.i(TAG, "pool sighting for ${tab.origin} tab: ${hit.txHashHex.take(16)}…")
            }
        }

        fun reconcile(context: Context) {
            val store = TabStore(context)
            val contacts = ContactStore(context)
            val settled = store.all().filter { it.state == "settled" }.sortedBy { it.settledAt }
            if (settled.isEmpty()) return
            // Same subtraction as the sighting above, and it matters more
            // here: minor 0 stays admissible for bills that predate per-contact
            // addresses, and minor 0 is exactly where our own change lands. An
            // output of our own must never close a customer's tab and fire a
            // receipt at somebody who has not paid.
            val ours = WalletStore(context).ourTxids()
            val entries = WalletStore(context).entries()
                .filterNot { it.txHashHex.lowercase() in ours }
            val claimed = (store.all().mapNotNull { it.paidKi } + store.claimedKis())
                .toMutableSet()

            for (tab in settled) {
                // §15.10's attribution: a bill billed to this contact's
                // subaddress can only be settled by an output that landed on
                // it. Minor 0 stays admissible for bills that predate the
                // per-contact address; an output on someone *else's* minor is
                // never this tab's money, whatever the amount says.
                val wantMinor = tab.billedMinor ?: WalletStore(context).minorOf(tab.personaHex)
                // Amounts this payer *said* they sent for this bill, from
                // their PAYMENT_SENT notices in the thread after it went out.
                // The notice is why a tip works at all: a tipped payment
                // arrives larger than the bill, so exact-amount matching alone
                // would never find it. Still §17.5 — the notice nominates an
                // amount, the chain confirms it; a notice with no matching
                // output settles nothing.
                val said = contacts.thread(tab.personaHex)
                    .filter {
                        !it.outgoing && it.kind == 2 &&
                            it.timestamp * 1000 >= tab.settledAt - 60_000 &&
                            it.amountPxmr >= tab.settledTotal &&
                            // §16.14: a notice that names a bill names *its*
                            // bill. Found live: two billed tabs for one
                            // person, and the older swallowed the newer's
                            // payment because the amount fit its window —
                            // while the notice was pointing at the other
                            // bill the whole time. A named notice widens
                            // only the tab it names; an unnamed one keeps
                            // the old behaviour.
                            (it.reSeq == null ||
                                (!it.reOwn && tab.billSeq > 0 && it.reSeq == tab.billSeq))
                    }
                    .map { it.amountPxmr }
                    .toSet()

                // Amounts whose notice names a DIFFERENT bill: that money is
                // spoken for. The exact-amount arm below used to claim any
                // output of the right size even while the only notice in the
                // thread pointed at another obligation on the same
                // subaddress — a recurring bill, a split share — closing two
                // debts with one payment and shorting the shop the other.
                val namedElsewhere = contacts.thread(tab.personaHex)
                    .filter {
                        !it.outgoing && it.kind == 2 && it.reSeq != null &&
                            !it.reOwn &&
                            !(tab.billSeq > 0 && it.reSeq == tab.billSeq) &&
                            it.timestamp * 1000 >= tab.settledAt - 60_000
                    }
                    .map { it.amountPxmr }.toSet()
                fun matches(e: org.ducatproject.ducat.WalletEntry): Boolean =
                    e.keyImage.isNotEmpty() &&
                        e.keyImage !in tab.knownKis &&
                        e.keyImage !in claimed &&
                        // Mined after the bill, not merely scanned after it: a
                        // wallet catching up surfaces old outputs the key-image
                        // snapshot never saw, and an exact-amount coincidence
                        // from last week must not settle tonight's tab.
                        (tab.tipAtBill == 0L || e.height > tab.tipAtBill) &&
                        paidWhereBilled(tab, wantMinor, e.minor) &&
                        ((e.amountPxmr == tab.settledTotal &&
                            e.amountPxmr !in namedElsewhere) ||
                            e.amountPxmr in said)
                // The sighting already learned which transaction this was —
                // matching on the amount again would be guessing twice about
                // something known (Orders' rule, adopted). The arithmetic
                // remains for tabs that were never sighted.
                val hit = entries.firstOrNull {
                    tab.seenTx != null &&
                        it.txHashHex.equals(tab.seenTx, ignoreCase = true) &&
                        matches(it)
                } ?: entries.firstOrNull { matches(it) } ?: continue
                // Everything above this line trusted one node's account of the
                // chain. Amount, subaddress and height all matched — but they
                // matched against blocks that node handed us, and no part of
                // the scan checks the work behind them. Before that becomes a
                // receipt and a customer walking out with the goods, ask
                // somebody else whether the transaction exists at all.
                //
                // Ahead of the claim, not after it: a deferral has to leave
                // this pass exactly as it found it, and a claimed key image
                // would lock the output out of the retry.
                if (!SecondOpinion.settles(context, hit.txHashHex)) continue
                claimed += hit.keyImage
                val contact = contacts.all()
                    .firstOrNull { it.personaHex == tab.personaHex } ?: continue

                // The receipt covers what actually arrived, and §16.13's sum
                // rule still has to hold — so a tip becomes a visible line,
                // which is also simply the truth of what was paid for.
                val tip = hit.amountPxmr - tab.settledTotal
                val receiptLines =
                    if (tip > 0) {
                        tab.lines + BillItem(
                            context.getString(R.string.bartab_tip_line), tip,
                        )
                    } else {
                        tab.lines
                    }
                // The mark first, the receipt second — markPaidOutside's own
                // rule, finally applied here too. Receipt-first meant a death
                // in between left the tab settled and the key image
                // unclaimed, so the next pass matched the same output again
                // and sent a second receipt for one payment. The mark and
                // the claim ride one commit (markPaid) for the same reason.
                store.markPaid(tab.id, hit.keyImage, hit.amountPxmr)
                    ?: continue
                runCatching {
                    Mailbox.send(
                        context, contact, context.getString(R.string.receipt_note),
                        PersonaStore(context).personaHex(),
                        kind = 3, amountPxmr = hit.amountPxmr,
                        items = receiptLines, taxPxmr = tab.taxPxmr,
                        txidHex = hit.txHashHex.ifEmpty { null },
                        // §16.14: the bill this settles, ours to name.
                        reSeq = tab.billSeq.takeIf { it > 0 }, reOwn = tab.billSeq > 0,
                    )
                }.onSuccess {
                    Notify.post(
                        context,
                        context.getString(
                            R.string.notify_paid_title, contact.displayName(),
                        ),
                        if (tip > 0) {
                            context.getString(
                                R.string.notify_paid_body_tip,
                                Amounts.show(context, hit.amountPxmr).primary,
                                Amounts.show(context, tip).primary,
                            )
                        } else {
                            context.getString(
                                R.string.notify_paid_body,
                                Amounts.show(context, hit.amountPxmr).primary,
                            )
                        },
                    )
                    DucatLog.i(
                        TAG,
                        "${tab.origin} paid ${formatXmr(hit.amountPxmr)} XMR" +
                            (if (tip > 0) " (tip ${formatXmr(tip)})" else "") +
                            " — receipt sent",
                    )
                }.onFailure {
                    // The payment is real and now recorded; a receipt that
                    // failed to leave is logged rather than blocking — the
                    // same trade markPaidOutside documents. (Retrying it here
                    // would mean matching the paid output a second time.)
                    DucatLog.w(TAG, "receipt failed after mark: ${it.message}")
                }
            }
        }
    }

    private fun save(tabs: List<RunningTab>) {
        prefs.edit().putString(
            "tabs_v1",
            JSONArray().also { a -> tabs.forEach { a.put(it.toJson()) } }.toString(),
        ).apply()
        ContactStore.bump()
    }
}
