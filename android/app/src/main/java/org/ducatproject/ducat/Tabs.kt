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
) {
    val totalPxmr: Long get() =
        if (state == "open") lines.sumOf { it.amountPxmr } + (taxPxmr ?: 0L) else settledTotal

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

    fun open(personaHex: String, origin: String): RunningTab {
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
        return t
    }

    fun update(t: RunningTab) = save(all().map { if (it.id == t.id) t else it })

    fun delete(id: String) = save(all().filterNot { it.id == id })

    /**
     * Key images ever matched to a bill, kept apart from the tabs themselves:
     * deleting a paid tab must not release its output back into the matching
     * pool, or a still-billed tab for the same amount could later "find" a
     * payment that already bought someone else's evening.
     */
    fun claimedKis(): Set<String> =
        prefs.getStringSet("claimed_kis_v1", emptySet()) ?: emptySet()

    fun addClaimedKi(ki: String) =
        prefs.edit().putStringSet("claimed_kis_v1", claimedKis() + ki).apply()

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
        Mailbox.send(
            context, contact,
            if (tab.origin == "taxi") "Your fare" else "Your tab",
            PersonaStore(context).personaHex(),
            kind = 1, amountPxmr = total,
            payto = WalletStore(context).addressFor(tab.personaHex),
            items = tab.lines, taxPxmr = tab.taxPxmr,
        )
        val settled = tab.copy(
            state = "settled",
            settledTotal = total,
            settledAt = System.currentTimeMillis(),
            knownKis = WalletStore(context).entries().map { it.keyImage },
            tipAtBill = WalletStore(context).tip(),
        )
        update(settled)
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
        update(tab.copy(state = "paid_oob"))
        val contact = ContactStore(context).all()
            .firstOrNull { it.personaHex == tab.personaHex } ?: return
        runCatching {
            Mailbox.send(
                context, contact, "Receipt — settled outside DUCAT. Thank you",
                PersonaStore(context).personaHex(),
                kind = 3, amountPxmr = tab.settledTotal,
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
                "That bill for ${formatXmr(tab.settledTotal)} XMR is cancelled — " +
                    "nothing to pay.",
                PersonaStore(context).personaHex(),
            )
        }.onFailure { DucatLog.w(TAG, "cancel notice: ${it.message}") }
        DucatLog.i(TAG, "${tab.origin} tab cancelled (${formatXmr(tab.settledTotal)} XMR)")
    }

    companion object {
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
            val contacts = ContactStore(context)
            for (tab in waiting.sortedBy { it.settledAt }) {
                val said = contacts.thread(tab.personaHex)
                    .filter { !it.outgoing && it.kind == 2 && it.amountPxmr >= tab.settledTotal }
                    .map { it.amountPxmr }.toSet()
                val hit = hits.firstOrNull {
                    it.amountPxmr.toLong() == tab.settledTotal ||
                        it.amountPxmr.toLong() in said
                } ?: continue
                store.update(tab.copy(seenTx = hit.txHashHex))
                Notify.post(
                    context, "Payment on its way",
                    "${formatXmr(hit.amountPxmr.toLong())} XMR seen — settling now",
                )
                DucatLog.i(TAG, "pool sighting for ${tab.origin} tab: ${hit.txHashHex.take(16)}…")
            }
        }

        fun reconcile(context: Context) {
            val store = TabStore(context)
            val contacts = ContactStore(context)
            val settled = store.all().filter { it.state == "settled" }.sortedBy { it.settledAt }
            if (settled.isEmpty()) return
            val entries = WalletStore(context).entries()
            val claimed = (store.all().mapNotNull { it.paidKi } + store.claimedKis())
                .toMutableSet()

            for (tab in settled) {
                // §15.10's attribution: a bill billed to this contact's
                // subaddress can only be settled by an output that landed on
                // it. Minor 0 stays admissible for bills that predate the
                // per-contact address; an output on someone *else's* minor is
                // never this tab's money, whatever the amount says.
                val wantMinor = WalletStore(context).minorOf(tab.personaHex)
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
                            it.amountPxmr >= tab.settledTotal
                    }
                    .map { it.amountPxmr }
                    .toSet()

                val hit = entries.firstOrNull {
                    it.keyImage.isNotEmpty() &&
                        it.keyImage !in tab.knownKis &&
                        it.keyImage !in claimed &&
                        // Mined after the bill, not merely scanned after it: a
                        // wallet catching up surfaces old outputs the key-image
                        // snapshot never saw, and an exact-amount coincidence
                        // from last week must not settle tonight's tab.
                        (tab.tipAtBill == 0L || it.height > tab.tipAtBill) &&
                        (it.minor == 0 || wantMinor == null || it.minor == wantMinor) &&
                        (it.amountPxmr == tab.settledTotal || it.amountPxmr in said)
                } ?: continue
                claimed += hit.keyImage
                val contact = contacts.all()
                    .firstOrNull { it.personaHex == tab.personaHex } ?: continue

                // The receipt covers what actually arrived, and §16.13's sum
                // rule still has to hold — so a tip becomes a visible line,
                // which is also simply the truth of what was paid for.
                val tip = hit.amountPxmr - tab.settledTotal
                val receiptLines =
                    if (tip > 0) tab.lines + BillItem("Tip — thank you", tip) else tab.lines
                runCatching {
                    Mailbox.send(
                        context, contact, "Receipt — thank you",
                        PersonaStore(context).personaHex(),
                        kind = 3, amountPxmr = hit.amountPxmr,
                        items = receiptLines, taxPxmr = tab.taxPxmr,
                        txidHex = hit.txHashHex.ifEmpty { null },
                    )
                }.onSuccess {
                    store.update(tab.copy(state = "paid", paidKi = hit.keyImage))
                    store.addClaimedKi(hit.keyImage)
                    Notify.post(
                        context,
                        "${contact.displayName()} paid",
                        "${formatXmr(hit.amountPxmr)} XMR" +
                            (if (tip > 0) " (tip ${formatXmr(tip)})" else "") +
                            " — receipt sent",
                    )
                    DucatLog.i(
                        TAG,
                        "${tab.origin} paid ${formatXmr(hit.amountPxmr)} XMR" +
                            (if (tip > 0) " (tip ${formatXmr(tip)})" else "") +
                            " — receipt sent",
                    )
                }.onFailure {
                    // The payment is real either way; the receipt retries on the
                    // next poll rather than marking paid and losing it.
                    DucatLog.w(TAG, "receipt failed, will retry: ${it.message}")
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
