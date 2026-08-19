package org.ducatproject.ducat

import android.content.Context

/**
 * One amount, shown the way this user asked to see it.
 *
 * Named `Amounts` rather than `Money` because §17.2's `Money` already exists as
 * a minor-units value type; this is presentation, not a quantity.
 *
 * Every screen that shows money goes through here, so the switch is one
 * preference rather than a habit each screen has to remember. A balance card
 * that honours it and a confirm dialog that does not is worse than neither —
 * the moment someone acts on a number is the moment the unit has to be the one
 * they were reading a second earlier.
 */
data class Shown(
    /** What the user asked for, large. */
    val primary: String,
    /** The other unit, small, so the number is always checkable. */
    val secondary: String?,
    /** True when the primary is a currency figure that stagenet makes fictional. */
    val notional: Boolean,
)

object Amounts {

    /** Whether amounts lead with the user's currency rather than XMR. */
    fun preferFiat(context: Context): Boolean = RateStore(context).preferFiat()

    fun setPreferFiat(context: Context, v: Boolean) = RateStore(context).setPreferFiat(v)

    /** Is there a rate to convert with at all? */
    fun canConvert(context: Context): Boolean {
        val r = RateStore(context)
        return r.enabled() && r.cached() != null
    }

    /**
     * Format an amount for display.
     *
     * Falls back to XMR whenever there is no rate rather than showing a stale
     * or invented one. A payment screen that guesses at a conversion is worse
     * than one that declines to.
     */
    fun show(context: Context, pxmr: Long, stagenet: Boolean = true): Shown {
        val store = RateStore(context)
        val xmr = "${formatXmr(pxmr)} XMR"
        val rate = if (store.enabled()) store.cached()?.first else null
        if (rate == null) return Shown(xmr, null, false)

        val fiat = "%s %,.2f".format(store.currency(), pxmr / 1_000_000_000_000.0 * rate)
        return if (store.preferFiat()) {
            Shown(fiat, xmr, stagenet)
        } else {
            Shown(xmr, fiat, false)
        }
    }

    /** The currency code in use, for labelling a switch. */
    fun currency(context: Context): String = RateStore(context).currency()

    /**
     * A figure in XMR as piconero, or null if it is not one.
     *
     * The one rule, in one place. Five screens had written this out
     * themselves — the till, the tip field, the taxi meter, the hail offer,
     * and the catalogue — and all five said `.toLong()`, which on a
     * [java.math.BigDecimal] is `longValue()`: on overflow it does not throw,
     * it returns the low sixty-four bits. Typing 18446744073709551617 into a
     * payment field therefore produced 1000000000000 piconero — exactly one
     * monero, positive and plausible, sailing past every `> 0` guard the
     * screens put after it.
     *
     * `longValueExact` throws instead, and every caller already treats null as
     * "that is not an amount". The [setScale] before it keeps the old
     * tolerance for more than twelve decimal places: those are truncated, as
     * they always were, rather than being rejected as inexact.
     */
    fun toPxmr(xmr: java.math.BigDecimal): Long? = runCatching {
        xmr.movePointRight(12).setScale(0, java.math.RoundingMode.DOWN).longValueExact()
    }.getOrNull()
}
