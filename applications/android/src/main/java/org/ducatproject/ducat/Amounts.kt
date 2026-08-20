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

    /**
     * How old the rate behind a currency figure is, or null when none is.
     *
     * The till has said this about its own prices for a while — a stall with
     * no signal keeps selling at the last rate it saw and says which. Nothing
     * said it about a *balance*, which mattered less while amounts led in XMR:
     * piconero is piconero however old the rate is. Now that the local
     * currency leads, the headline figure on the home screen is a conversion,
     * and a conversion from a rate nobody could refresh for two days is a
     * number that looks exactly as confident as a fresh one.
     */
    fun rateAgeSecs(context: Context): Long? {
        val store = RateStore(context)
        if (!store.enabled() || !store.preferFiat()) return null
        val at = store.cached()?.second ?: return null
        return (System.currentTimeMillis() / 1000 - at).coerceAtLeast(0)
    }

    /** The currency code in use, for labelling a switch. */
    fun currency(context: Context): String = RateStore(context).currency()

    /**
     * The unit a money *entry* field should open in.
     *
     * [preferFiat] alone is not enough. Every one of these fields converts what
     * was typed by dividing by a cached rate, and returns null when there is
     * none — which the screens above render as a disabled button. So opening in
     * the local currency on a phone that has never reached a price source gives
     * somebody a field they can type into and a button that will not light,
     * with nothing on screen connecting the two. Ask for both, and a phone with
     * no rate simply asks for XMR, which it can always convert.
     */
    fun enterFiat(context: Context): Boolean = preferFiat(context) && canConvert(context)

    /**
     * Everything a money field should let somebody type.
     *
     * Not `Char.isDigit() || c == '.' || c == ','`, which is what every amount
     * field used to say. That filter is two mistakes at once:
     *
     *  - `isDigit()` is a Unicode test and passes ٣ and ३ and ๓, but the
     *    separator beside them is not `.` or `,`. Arabic and Persian write
     *    their decimal point as `٫` (U+066B), so `٣٫٢٠` was **filtered down to
     *    `٣٢٠`** and priced a coffee at three hundred and twenty. The
     *    separator the user typed was deleted as they typed it.
     *
     *  - Renting's field went further and allowed only `.`, so a German
     *    pricing a room at `25,50` got a listing for 2550 — eleven of the
     *    languages this ships in write their decimal point that way.
     *
     * Grouping marks are accepted here and dropped in [typedNumber], because
     * refusing the key someone's keyboard puts under their thumb is not a
     * validation strategy.
     */
    fun isNumberChar(c: Char): Boolean =
        Character.digit(c, 10) in 0..9 || c in ".,٫٬．，"

    /**
     * What they typed, as something [java.math.BigDecimal] reads the same way
     * they meant it.
     *
     * Digits fold to ASCII — BigDecimal accepts Persian and Devanagari digits
     * on its own, but Double's parser does not and some of these fields still
     * use it, so one shape for both. Every decimal separator becomes `.`, and
     * grouping marks vanish.
     *
     * A comma is a decimal point, not a thousands mark. That is the existing
     * rule here and it stays: the app has never asked anyone to type a
     * grouped figure, and reading `3,20` as three hundred and twenty would be
     * the same hundredfold mistake in the other direction.
     */
    fun typedNumber(s: String): String = buildString {
        for (c in s.trim()) {
            val d = Character.digit(c, 10)
            when {
                d in 0..9 -> append('0' + d)
                c == '.' || c == ',' || c == '٫' || c == '．' || c == '，' ->
                    append('.')
                // Grouping: dropped, never a decimal point.
                c == '٬' || c == ' ' || c == ' ' || c == ' ' -> Unit
                // Anything else is left for the parser to refuse, so that
                // "12abc" stays an error rather than quietly becoming 12.
                else -> append(c)
            }
        }
    }

    /** The number they typed, or null if it is not one. */
    fun parse(s: String): java.math.BigDecimal? =
        typedNumber(s).takeIf { it.isNotEmpty() }?.toBigDecimalOrNull()

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
