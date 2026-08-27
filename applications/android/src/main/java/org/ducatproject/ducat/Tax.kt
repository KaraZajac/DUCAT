package org.ducatproject.ducat

import android.content.Context
import java.math.BigDecimal
import java.math.RoundingMode

/**
 * The counter's sales tax, as a standing percentage.
 *
 * Tax existed on the wire long before this — a bill's `tax_pxmr`, checked by
 * core to add up — but the till asked for it as a typed *amount*, per sale.
 * A business does not know the amount; it knows the rate, and typing 8.25%
 * of a subtotal into a phone at a counter is arithmetic a machine should be
 * doing in front of the customer, not the seller doing behind one.
 *
 * Stored as **basis points** (8.25% = 825), because a float percent drifts
 * and a price must not. Plain prefs rather than securePrefs: a tax rate is
 * business configuration, not a secret, and it belongs in the same tier as
 * the display currency.
 *
 * One rate for the whole phone. A till is one business in one place; the
 * day DUCAT meets a business straddling two tax jurisdictions on one phone,
 * this becomes per-mode — and not before, because every knob is a thing a
 * counter can set wrongly at closing time.
 */
object Tax {
    private fun prefs(context: Context) =
        context.getSharedPreferences("ducat_business", Context.MODE_PRIVATE)

    fun enabled(context: Context): Boolean = prefs(context).getBoolean("tax_on", false)

    /** Basis points: 825 is 8.25%. Zero when unset. */
    fun basisPoints(context: Context): Int = prefs(context).getInt("tax_bp", 0)

    fun set(context: Context, enabled: Boolean, basisPoints: Int) {
        prefs(context).edit()
            .putBoolean("tax_on", enabled)
            .putInt("tax_bp", basisPoints.coerceIn(0, 100_00))
            .apply()
        ContactStore.bump()
    }

    /** "8.25" ⇄ 825. Parsed through the same folding every typed number gets. */
    fun parsePercent(typed: String): Int? {
        val v = Amounts.parse(typed) ?: return null
        if (v < BigDecimal.ZERO || v > BigDecimal(100)) return null
        return v.movePointRight(2).setScale(0, RoundingMode.HALF_UP).toInt()
    }

    fun percentText(bp: Int): String =
        BigDecimal(bp).movePointLeft(2).stripTrailingZeros().toPlainString()

    /**
     * The tax on a subtotal, in piconero.
     *
     * BigDecimal, rounded **down**: `2.01 * 1e12` through a double comes out
     * a piconero short and a customer holding an itemised bill is being shown
     * arithmetic, not an estimate — and where a rounding direction must be
     * picked, undercharging by one piconero is the side to err on.
     */
    fun on(context: Context, subtotalPxmr: Long): Long {
        if (!enabled(context) || subtotalPxmr <= 0) return 0L
        val bp = basisPoints(context)
        if (bp <= 0) return 0L
        return BigDecimal(subtotalPxmr)
            .multiply(BigDecimal(bp))
            .divide(BigDecimal(10_000), 0, RoundingMode.DOWN)
            .toLong()
    }
}
