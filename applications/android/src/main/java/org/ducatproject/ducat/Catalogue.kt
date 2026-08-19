package org.ducatproject.ducat

import android.content.Context
import java.math.BigDecimal
import java.math.RoundingMode
import org.json.JSONArray
import org.json.JSONObject

/**
 * What this device sells: a saved list of things, tapped instead of typed.
 *
 * A till already builds a bill out of [BillItem]s, and core already refuses
 * one whose lines do not add up to its total — so nothing about the wire
 * changes here. This is only the difference between typing "Flat white" and
 * "4.00" forty times a shift and tapping a button that means both.
 *
 * **Priced in the seller's own money, not in XMR.** A flat white is £3.20 and
 * stays £3.20; what that is worth in monero is a question asked at the moment
 * of the sale, which is the only moment it can be answered honestly. Storing
 * the XMR figure instead would silently re-price the whole menu every time
 * the rate moved, and the person holding the till would be the last to know.
 *
 * The price is kept as the decimal text the seller typed rather than a
 * floating-point number: money in a `Double` is a rounding error waiting for
 * a busy Saturday, and `PosAddLine` has always parsed with [BigDecimal] for
 * the same reason.
 */
object Catalogue {
    private const val TAG = "DucatCatalogue"

    private fun prefs(context: Context) = securePrefs(context, "ducat_catalogue")

    /** One thing that can be sold. */
    data class Item(
        val id: String,
        val name: String,
        /** Decimal text in [currency] — "3.20", exactly as it was typed. */
        val price: String,
        /** The currency it was priced in, so a later change of currency is
         *  noticed rather than silently reinterpreted. */
        val currency: String,
        /** Free-text grouping ("Coffee", "Pastries"); blank is fine. */
        val category: String = "",
        /** Kept for its history but off the till. */
        val archived: Boolean = false,
        val sort: Long = 0,
    )

    fun all(context: Context): List<Item> {
        val raw = prefs(context).getString("items", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return emptyList()
        return (0 until arr.length()).mapNotNull { i ->
            runCatching {
                val o = arr.getJSONObject(i)
                Item(
                    id = o.optString("id"),
                    name = o.optString("name"),
                    price = o.optString("price"),
                    currency = o.optString("currency"),
                    category = o.optString("category"),
                    archived = o.optBoolean("archived"),
                    sort = o.optLong("sort"),
                )
            }.getOrNull()?.takeIf { it.id.isNotBlank() && it.name.isNotBlank() }
        }.sortedWith(compareBy({ it.sort }, { it.name.lowercase() }))
    }

    /** What the till shows: everything not archived. */
    fun live(context: Context): List<Item> = all(context).filter { !it.archived }

    fun put(context: Context, item: Item) {
        val kept = all(context).filter { it.id != item.id }
        save(context, kept + item)
    }

    fun remove(context: Context, id: String) =
        save(context, all(context).filter { it.id != id })

    private fun save(context: Context, items: List<Item>) {
        val arr = JSONArray()
        items.forEach {
            arr.put(
                JSONObject()
                    .put("id", it.id)
                    .put("name", it.name)
                    .put("price", it.price)
                    .put("currency", it.currency)
                    .put("category", it.category)
                    .put("archived", it.archived)
                    .put("sort", it.sort),
            )
        }
        prefs(context).edit().putString("items", arr.toString()).apply()
        ContactStore.bump()
    }

    fun draft(context: Context, name: String, price: String): Item = Item(
        id = java.util.UUID.randomUUID().toString(),
        name = name,
        price = price,
        currency = Amounts.currency(context),
        sort = System.currentTimeMillis(),
    )

    /** Why an item cannot be rung up right now, if it cannot. */
    enum class Snag {
        /** No exchange rate has ever been fetched, so a price in pounds
         *  cannot be turned into one in monero at all. */
        NoRate,

        /** Priced in a currency this device no longer shows. Re-price it
         *  rather than guess what the seller meant. */
        WrongCurrency,

        /** The stored text is not a number, or is not positive. */
        Unpriceable,
    }

    /**
     * What this costs in piconero at this moment, and how sure we are.
     *
     * [staleSecs] is how old the rate behind it is: a till with no signal
     * keeps working on the last rate it saw, and says which, rather than
     * refusing to sell anything. That is the trade a market stall wants —
     * the alternative is a queue and a phone that will not take money.
     */
    data class Priced(val pxmr: Long, val staleSecs: Long)

    fun price(context: Context, item: Item): Result<Priced> {
        if (item.currency.isNotBlank() && item.currency != Amounts.currency(context)) {
            return Result.failure(SnagException(Snag.WrongCurrency))
        }
        val (rate, at) = RateStore(context).cached()
            ?: return Result.failure(SnagException(Snag.NoRate))
        if (rate <= 0) return Result.failure(SnagException(Snag.NoRate))
        // Through the one reader, so a price typed on an Arabic keyboard means
        // what its author meant. `replace(',', '.')` handled exactly one of
        // the world's decimal separators.
        val decimal = Amounts.parse(item.price)
            ?: return Result.failure(SnagException(Snag.Unpriceable))
        val pxmr = Amounts
            .toPxmr(decimal.divide(BigDecimal.valueOf(rate), 12, RoundingMode.DOWN))
            ?.takeIf { it > 0 }
            ?: return Result.failure(SnagException(Snag.Unpriceable))
        // `at` is epoch **seconds** — monero_rate stamps it with as_secs().
        return Result.success(
            Priced(pxmr, (System.currentTimeMillis() / 1000 - at).coerceAtLeast(0)),
        )
    }

    class SnagException(val snag: Snag) : IllegalStateException(snag.name)
}
