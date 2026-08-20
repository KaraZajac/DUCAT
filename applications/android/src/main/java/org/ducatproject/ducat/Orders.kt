package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Orders placed at a kiosk: somebody who walked up and tapped what they
 * wanted, and the number the shop calls out when it is ready.
 *
 * **A kiosk customer becomes a contact, briefly.** This began the other way
 * round — a bare `monero:` code and an anonymous stranger — on the reasoning
 * that a queue for coffee has no time for a handshake. That reasoning was
 * wrong, and it cost the customer everything the protocol is for: a bare
 * address buys a payment and nothing else. No itemised bill, so they pay a
 * number with no idea what it is made of. No conversation, so the shop cannot
 * tell them the order is ready. No transaction named by the payer, so the
 * till has to *guess* which payment was whose from the amount. And no
 * receipt, so their Activity records money leaving and never what for.
 *
 * So the counter shows a card. They tap it or scan it, which is one gesture
 * either way, and from that moment this is an ordinary [RunningTab] — the
 * same bill, chain-watch and receipt the till and the bar tab have always
 * used, with `origin` of [ORIGIN]. See [begin] and [bind].
 *
 * **The old path is still here**, as the fallback it should always have been:
 * [place] and [payUri] serve somebody standing at the counter with a Monero
 * wallet and no DUCAT. Those orders carry their own subaddress and a few
 * piconero of noise in the total, because a mempool sighting reports an
 * amount and a hash and cannot say which subaddress an unconfirmed output
 * landed on — so with nobody to name the transaction, the amount is all there
 * is to match on, and two people ordering the same coffee would otherwise be
 * indistinguishable. That is the shape of the compromise, and the reason it
 * is not the default.
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

    /**
     * Guards read-modify-write of the whole order list.
     *
     * Every write here rewrites the entire array, and two threads do it: the
     * poller sights payments and gives up on abandoned baskets while a screen
     * is beginning and binding orders. Without this, whichever wrote second
     * wrote from a list it had read before the other's change — so a payment
     * already sighted could revert to awaiting, or an order just billed could
     * vanish along with the tab still pointing at it.
     *
     * ContactStore learned this the hard way and says so at its own lock; the
     * stores added since did not inherit the lesson. On the companion, because
     * callers make a fresh store per call and a per-instance lock guards
     * nothing.
     */
    private val lock = Any()

    /** Read, change, write — with nobody else in between. */
    private fun mutate(context: Context, f: (List<Order>) -> List<Order>) =
        synchronized(lock) { save(context, f(all(context))) }

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
        /**
         * The tab this order was billed through, once a customer has paired.
         *
         * Present means this order went the way the protocol intends: they
         * tapped or scanned, they became somebody this till can talk to, and
         * the bill went into that conversation itemised — so the payment is
         * identified by the transaction they name in their notice, not
         * guessed at from an amount, and they get a receipt in their Activity
         * rather than a line in a wallet that says nothing about what they
         * bought. [TabStore] owns everything after this point; the order keeps
         * only the number the shop calls out.
         */
        val tabId: String? = null,
        val personaHex: String? = null,
        /**
         * When the shop said it was ready, or 0.
         *
         * The whole argument for making a kiosk customer pair is that the
         * counter can then talk to them. Calling a number across a room is
         * what a counter did before it could.
         */
        val readyAt: Long = 0,
    ) {
        /** True while this order is still waiting for somebody to pair. */
        val unpaired: Boolean get() = tabId == null && address.isEmpty()
    }

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
                    tabId = o.optString("tab").takeIf { it.isNotBlank() },
                    personaHex = o.optString("who").takeIf { it.isNotBlank() },
                    readyAt = o.optLong("ready"),
                )
            }.getOrNull()?.takeIf { it.id.isNotBlank() }
        }.sortedByDescending { it.placedAt }
    }

    fun update(context: Context, order: Order) =
        mutate(context) { it.filter { o -> o.id != order.id } + order }

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
                    .put("seen", o.seenTx ?: "")
                    .put("tab", o.tabId ?: "").put("who", o.personaHex ?: "")
                    .put("ready", o.readyAt),
            )
        }
        prefs(context).edit().putString("orders", arr.toString()).apply()
        ContactStore.bump()
    }

    /**
     * How many addresses the counter rotates through.
     *
     * Not one per order. A subaddress here is allocated a permanent minor
     * index, and that index is what the wallet scanner and the pool scan have
     * to check every output against — so a stall doing two hundred orders a
     * day was signing the wallet up to seventy thousand subaddress checks per
     * output within the year, plus a preference key per order in a document
     * rewritten whole on every write. Both grow for ever and neither ever
     * shrinks.
     *
     * A ring gets what the addresses were for: no two customers in a queue
     * are shown the same one, which is the privacy that matters at a counter.
     * An address comes round again after sixty-four orders, by which time the
     * customer who used it is long gone — and attribution never depended on
     * the address anyway, only on the amount.
     */
    private const val ADDRESS_SLOTS = 64

    /**
     * Turn a basket into something a stranger with any Monero wallet can pay.
     *
     * The fallback, not the path: see [begin] for the one that gets them a
     * bill and a receipt. The address is this order's alone and the total
     * carries its noise, so that when a payment appears there is no question
     * which order it was for.
     */
    fun place(context: Context, lines: List<BillItem>): Order {
        val id = java.util.UUID.randomUUID().toString()
        val plain = lines.sumOf { it.amountPxmr }
        val noise = java.security.SecureRandom().nextInt(TAG_RANGE.toInt()).toLong()
        val next = (all(context).maxOfOrNull { it.number } ?: 0) % 999 + 1
        val slot = synchronized(lock) {
            val n = prefs(context).getInt("slot_next", 0)
            prefs(context).edit().putInt("slot_next", (n + 1) % ADDRESS_SLOTS).apply()
            n
        }
        val order = Order(
            id = id,
            number = next,
            lines = lines,
            totalPxmr = plain + noise,
            address = WalletStore(context).addressFor("order_slot_$slot") ?: "",
            state = State.Awaiting,
            placedAt = System.currentTimeMillis() / 1000,
        )
        update(context, order)
        DucatLog.i(TAG, "order #${order.number}: ${formatXmr(order.totalPxmr)} XMR")
        return order
    }

    /** The origin every kiosk tab carries, so a shop can tell them apart. */
    const val ORIGIN = "kiosk"

    /**
     * A basket, waiting for whoever is about to tap or scan.
     *
     * No address and no noise: this order does not know yet who it belongs
     * to, and it is not supposed to guess. The screen shows a card, somebody
     * claims it, and [bind] turns that into a conversation with a bill in it.
     */
    fun begin(context: Context, lines: List<BillItem>): Order {
        val order = Order(
            id = java.util.UUID.randomUUID().toString(),
            number = (all(context).maxOfOrNull { it.number } ?: 0) % 999 + 1,
            lines = lines,
            totalPxmr = lines.sumOf { it.amountPxmr },
            address = "",
            state = State.Awaiting,
            placedAt = System.currentTimeMillis() / 1000,
        )
        update(context, order)
        DucatLog.i(TAG, "order #${order.number} waiting for a customer to pair")
        return order
    }

    /**
     * Somebody claimed the card. Bill them.
     *
     * From here the order is a tab like any other, which is the whole point:
     * the bill is itemised inside a conversation, the address is theirs
     * alone, the payment is identified by the transaction their notice names
     * rather than guessed at from an amount, and the receipt the poller sends
     * lands in their Activity beside the payment it is for. None of that is
     * available to a bare address in a QR code.
     */
    fun bind(context: Context, order: Order, personaHex: String): Order {
        val tabs = TabStore(context)
        val opened = tabs.open(personaHex, ORIGIN)
        val settled = tabs.settle(tabs.mutate(opened.id) { it.copy(lines = order.lines) }!!)
        val bound = order.copy(
            tabId = opened.id,
            personaHex = personaHex,
            totalPxmr = settled.settledTotal,
            state = State.Awaiting,
        )
        update(context, bound)
        DucatLog.i(TAG, "order #${order.number} billed to ${personaHex.take(8)}…")
        return bound
    }

    /**
     * Tell them it is ready.
     *
     * The counter has a conversation with this customer — that is what the
     * card bought, and until now the only thing it carried was the bill. A
     * number called across a room only reaches whoever is still standing in
     * it; this reaches somebody who stepped outside.
     *
     * Written in the shop's language, like the bill and the receipt: the shop
     * is the one speaking, and it cannot know what its customer reads.
     */
    fun sayReady(context: Context, order: Order) {
        // Both refusals, not one. An order nobody paired to has nobody to
        // tell, and returning quietly there would have the shop press Ready
        // and watch nothing happen — the same non-answer the line below
        // already declines to give.
        val hex = order.personaHex
            ?: throw IllegalStateException("nobody claimed this order")
        val contact = ContactStore(context).all().firstOrNull { it.personaHex == hex }
            ?: throw IllegalStateException("that customer is gone")
        Mailbox.send(
            context, contact,
            context.getString(R.string.kiosk_ready_message, order.number),
            PersonaStore(context).personaHex(),
        )
        update(context, order.copy(readyAt = System.currentTimeMillis() / 1000))
        DucatLog.i(TAG, "order #${order.number} called as ready")
    }

    /**
     * Where a bound order has got to, read from the tab that owns it.
     *
     * Derived rather than copied, deliberately: two records of one fact
     * drift, and the one the customer's receipt depends on is the tab's.
     */
    fun stateOf(context: Context, order: Order): State {
        val tab = order.tabId?.let { TabStore(context).get(it) } ?: return order.state
        return when {
            tab.state == "paid" || tab.state == "paid_oob" -> State.Confirmed
            tab.state == "cancelled" -> State.Abandoned
            tab.seenTx != null -> State.Seen
            else -> State.Awaiting
        }
    }

    /**
     * What a customer's wallet scans: address and exact amount.
     *
     * The fallback path, for somebody at the counter with a Monero wallet and
     * no DUCAT. It buys them a payment and nothing else — no itemised bill, no
     * receipt, no record of what they bought — which is why [begin] and [bind]
     * are what the Order button reaches for first.
     *
     * [exactXmr], not [formatXmr] — the latter rounds to six places, which is
     * precisely where this order's identifying noise begins. Asking for
     * `0.038000` instead of `0.038000235547` means the payment that arrives
     * matches no order at all.
     */
    fun payUri(order: Order): String =
        "monero:${order.address}?tx_amount=${exactXmr(order.totalPxmr)}"

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
        val everything = all(context)
        // Only the ones paying a bare address. A bound order's money is the
        // tab's business — it is identified by the transaction the customer's
        // notice names, which is exact, rather than by an amount that has to
        // be tagged with noise to be told apart at all.
        val waiting = everything.filter {
            it.state == State.Awaiting && it.tabId == null && it.address.isNotEmpty()
        }
        if (waiting.isEmpty()) return
        val spend = WalletStore(context).spendKeyHex() ?: return
        val hits = runCatching {
            uniffi.ducat_mobile.moneroScanPool(
                node, spend, 40u, WalletStore(context).subaddressCount().toUInt(),
            )
        }.getOrElse { return }
        if (hits.isEmpty()) return

        // One payment settles one order. Amounts are what a mempool sighting
        // can be matched on, and the noise in each total is what keeps two
        // four-pound coffees apart — but noise collides eventually, and when
        // it does the same transaction would otherwise mark two orders paid
        // and hand somebody a free one. So a hash is spent once: not by an
        // order already sighted on it (it stays in the pool for minutes after,
        // and a later order could collide with it), and not twice in this
        // sweep.
        // Our own change sits in the same mempool. It cannot pay for an
        // order, and an order marked paid by it would hand out the goods.
        val ours = WalletStore(context).ourTxids()
        val claimed = everything.mapNotNull { it.seenTx }.toMutableSet()
        for (order in waiting) {
            val hit = hits.firstOrNull {
                it.amountPxmr.toLong() == order.totalPxmr &&
                    it.txHashHex !in claimed &&
                    it.txHashHex.lowercase() !in ours
            } ?: continue
            claimed += hit.txHashHex
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
     * How long an unpaid order keeps being looked for.
     *
     * The customer is standing at the counter; if they were going to pay they
     * did so within a minute. What this really bounds is the scanning: while
     * any order is awaiting payment the poller reads the mempool every pass,
     * at a round trip per transaction in it, and a stall that took forty
     * orders across a Saturday would otherwise be searching for all the ones
     * that walked away until somebody force-stopped the app.
     */
    private const val ABANDON_AFTER_SECS = 30L * 60

    /** Give up on the ones nobody paid, so the till stops looking for them. */
    fun expire(context: Context) {
        val cutoff = System.currentTimeMillis() / 1000 - ABANDON_AFTER_SECS
        all(context)
            .filter {
                // A bound order is the tab's to finish or cancel; an unpaired
                // one is a basket somebody walked away from mid-tap.
                it.state == State.Awaiting && it.tabId == null &&
                    it.placedAt in 1 until cutoff
            }
            .forEach {
                update(context, it.copy(state = State.Abandoned))
                DucatLog.i(TAG, "order #${it.number} abandoned — nobody paid")
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
