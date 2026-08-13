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
    /** open → settled (billed, unpaid) → paid. */
    val state: String,
    val settledTotal: Long = 0,
    val settledAt: Long = 0,
    /** Key images already in the wallet when the bill went out, so an old
     *  output can never be mistaken for this payment. */
    val knownKis: List<String> = emptyList(),
    /** The output that settled it, so no other tab can claim the same one. */
    val paidKi: String? = null,
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
        )
    }
}

class TabStore(private val context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

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
            payto = WalletStore(context).address(),
            items = tab.lines, taxPxmr = tab.taxPxmr,
        )
        val settled = tab.copy(
            state = "settled",
            settledTotal = total,
            settledAt = System.currentTimeMillis(),
            knownKis = WalletStore(context).entries().map { it.keyImage },
        )
        update(settled)
        DucatLog.i(TAG, "settled ${tab.origin} tab with ${contact.displayName()}: ${formatXmr(total)} XMR")
        return settled
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
        fun reconcile(context: Context) {
            val store = TabStore(context)
            val settled = store.all().filter { it.state == "settled" }.sortedBy { it.settledAt }
            if (settled.isEmpty()) return
            val entries = WalletStore(context).entries()
            val claimed = store.all().mapNotNull { it.paidKi }.toMutableSet()

            for (tab in settled) {
                val hit = entries.firstOrNull {
                    it.keyImage.isNotEmpty() &&
                        it.keyImage !in tab.knownKis &&
                        it.keyImage !in claimed &&
                        it.amountPxmr == tab.settledTotal
                } ?: continue
                claimed += hit.keyImage
                val contact = ContactStore(context).all()
                    .firstOrNull { it.personaHex == tab.personaHex } ?: continue
                runCatching {
                    Mailbox.send(
                        context, contact, "Receipt — thank you",
                        PersonaStore(context).personaHex(),
                        kind = 3, amountPxmr = tab.settledTotal,
                        items = tab.lines, taxPxmr = tab.taxPxmr,
                        txidHex = hit.txHashHex.ifEmpty { null },
                    )
                }.onSuccess {
                    store.update(tab.copy(state = "paid", paidKi = hit.keyImage))
                    DucatLog.i(TAG, "${tab.origin} tab paid — receipt sent (${formatXmr(tab.settledTotal)} XMR)")
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
