package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Orders placed at a kiosk: somebody who walked up, tapped what they wanted,
 * and paid — without ever having met this shop or installed anything.
 *
 * A kiosk customer is not a contact. There is no thread, no card, no
 * handshake: they scan a code with whatever Monero wallet they already have
 * and walk away. So an order cannot be a [RunningTab], which is a
 * conversation with somebody, and it cannot be settled by a receipt travelling
 * a thread that does not exist. What identifies the payment instead is the
 * money itself.
 *
 * **How an order knows its own payment.** Each one gets its own subaddress,
 * so two customers never pay the same address. But the pool scanner reports a
 * sighting as an amount and a hash — it cannot say which subaddress an
 * unconfirmed output landed on — so an amount is what a sighting can be
 * matched against, and two people ordering the same coffee would otherwise be
 * indistinguishable. Every order therefore carries a few piconero of noise in
 * its total: invisible to a customer (a millionth of a monero at the outside)
 * and enough to tell one £4 order from the next.
 */
object Orders {
    private const val TAG = "DucatOrders"

    /**
     * How much noise to add to a total so it identifies itself. A millionth
     * of a monero is far below anything a price is quoted in and far above
     * the odds of two open orders colliding.
     */
    private const val TAG_RANGE = 1_000_000L

    private fun prefs(context: Context) = securePrefs(context, "ducat_orders")

    /** Where an order has got to. */
    enum class State {
        /** Shown to the customer, waiting for their payment. */
        Awaiting,

        /** Seen in the mempool: real bytes, zero confirmations (§17.5). */
        Seen,

        /** On the chain. */
        Confirmed,

        /** Nobody paid, and the customer walked away. */
        Abandoned,
    }

    data class Order(
        val id: String,
        /** What the shop calls it out as — small, and reused across days. */
        val number: Int,
        val lines: List<BillItem>,
        /** Including the noise: this is what the customer is asked for. */
        val totalPxmr: Long,
        val address: String,
        val state: State,
        val placedAt: Long,
        val seenTx: String? = null,
    )

    fun all(context: Context): List<Order> {
        val raw = prefs(context).getString("orders", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return emptyList()
        return (0 until arr.length()).mapNotNull { i ->
            runCatching {
                val o = arr.getJSONObject(i)
                val lines = o.optJSONArray("lines") ?: JSONArray()
                Order(
                    id = o.optString("id"),
                    number = o.optInt("number"),
                    lines = (0 until lines.length()).map {
                        val l = lines.getJSONObject(it)
                        BillItem(l.optString("d"), l.optLong("a"))
                    },
                    totalPxmr = o.optLong("total"),
                    address = o.optString("address"),
                    state = runCatching { State.valueOf(o.optString("state")) }
                        .getOrDefault(State.Awaiting),
                    placedAt = o.optLong("at"),
                    seenTx = o.optString("seen").takeIf { it.isNotBlank() },
                )
            }.getOrNull()?.takeIf { it.id.isNotBlank() }
        }.sortedByDescending { it.placedAt }
    }

    fun update(context: Context, order: Order) =
        save(context, all(context).filter { it.id != order.id } + order)

    private fun save(context: Context, orders: List<Order>) {
        val arr = JSONArray()
        // A kiosk runs all day; keeping every order for ever would grow
        // without bound on a device nobody ever clears out.
        orders.sortedByDescending { it.placedAt }.take(500).forEach { o ->
            val lines = JSONArray()
            o.lines.forEach {
                lines.put(JSONObject().put("d", it.description).put("a", it.amountPxmr))
            }
            arr.put(
                JSONObject()
                    .put("id", o.id).put("number", o.number).put("lines", lines)
                    .put("total", o.totalPxmr).put("address", o.address)
                    .put("state", o.state.name).put("at", o.placedAt)
                    .put("seen", o.seenTx ?: ""),
            )
        }
        prefs(context).edit().putString("orders", arr.toString()).apply()
        ContactStore.bump()
    }

    /**
     * Turn a basket into something a stranger can pay.
     *
     * The address is this order's alone, and the total carries its noise, so
     * that when a payment appears there is no question which order it was
     * for.
     */
    fun place(context: Context, lines: List<BillItem>): Order {
        val id = java.util.UUID.randomUUID().toString()
        val plain = lines.sumOf { it.amountPxmr }
        val noise = java.security.SecureRandom().nextInt(TAG_RANGE.toInt()).toLong()
        val next = (all(context).maxOfOrNull { it.number } ?: 0) % 999 + 1
        val order = Order(
            id = id,
            number = next,
            lines = lines,
            totalPxmr = plain + noise,
            address = WalletStore(context).addressFor("order_$id") ?: "",
            state = State.Awaiting,
            placedAt = System.currentTimeMillis() / 1000,
        )
        update(context, order)
        DucatLog.i(TAG, "order #${order.number}: ${formatXmr(order.totalPxmr)} XMR")
        return order
    }

    /** What a customer's wallet scans: address and exact amount. */
    fun payUri(order: Order): String =
        "monero:${order.address}?tx_amount=${formatXmr(order.totalPxmr)}"

    /**
     * Look for the money in the mempool (§17.5's *seen*, not settled).
     *
     * A queue for coffee cannot wait ten blocks, so a kiosk accepts a
     * sighting: real bytes, zero confirmations, and the ordinary risk that
     * carries on a few pounds. The order says *seen* rather than *paid* until
     * the chain agrees, so nobody counting the day's takings is misled about
     * which of them have actually settled.
     */
    fun poolSight(context: Context, node: String) {
        val waiting = all(context).filter { it.state == State.Awaiting }
        if (waiting.isEmpty()) return
        val spend = WalletStore(context).spendKeyHex() ?: return
        val hits = runCatching {
            uniffi.ducat_mobile.moneroScanPool(
                node, spend, 40u, WalletStore(context).subaddressCount().toUInt(),
            )
        }.getOrElse { return }
        if (hits.isEmpty()) return
        for (order in waiting) {
            val hit = hits.firstOrNull { it.amountPxmr.toLong() == order.totalPxmr } ?: continue
            update(context, order.copy(state = State.Seen, seenTx = hit.txHashHex))
            Notify.post(
                context,
                context.getString(R.string.kiosk_notify_title, order.number),
                context.getString(R.string.kiosk_notify_body, formatXmr(order.totalPxmr)),
            )
            DucatLog.i(TAG, "order #${order.number} seen — ${hit.txHashHex.take(16)}…")
        }
    }

    /**
     * Promote sighted orders once their money is actually on the chain.
     *
     * By transaction hash, not by amount: the sighting already learned which
     * transaction it was, and the wallet records the same hash against the
     * output when the block carrying it is scanned. Matching on the amount
     * again would be guessing twice about something already known.
     */
    fun reconcile(context: Context) {
        val seen = all(context).filter { it.state == State.Seen && it.seenTx != null }
        if (seen.isEmpty()) return
        val landed = WalletStore(context).entries().map { it.txHashHex }.toSet()
        seen.forEach { order ->
            if (order.seenTx in landed) {
                update(context, order.copy(state = State.Confirmed))
                DucatLog.i(TAG, "order #${order.number} confirmed on chain")
            }
        }
    }
}
