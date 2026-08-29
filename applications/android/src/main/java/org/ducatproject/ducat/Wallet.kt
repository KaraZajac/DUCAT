package org.ducatproject.ducat

/**
 * What the wallet screen needs to know, and nothing more.
 *
 * **These values come from the Rust core, not from Kotlin.** `core::float` owns
 * the arithmetic — `plan`, `payments_supported`, `OUTPUTS_PER_PAYMENT` — because
 * §17.2's capacity rule is a protocol fact and a second implementation of it in
 * Kotlin would be a second thing to keep in step. This file is a shape, not a
 * calculation, and the stub below is marked as such.
 */
data class Float(
    /** Piconero spendable right now: the unlocked outputs, summed. */
    val spendablePxmr: Long,
    /** Piconero in change still locked, with the blocks remaining. */
    val lockedPxmr: Long,
    val blocksToUnlock: Int,
    /** Unlocked outputs. §17.2: capacity is a *count*, never a balance. */
    val unlockedOutputs: Int,
)

/**
 * The one number the home screen must never overstate.
 *
 * §17.2 forbids promising an exact count: the drain test measured six unlocked
 * outputs buying four consecutive payments, because input selection belongs to
 * the wallet and a payment may consume more than one output. "About four more"
 * is honest; "four more" is not.
 */
data class Capacity(val approxPayments: Int, val exact: Boolean = false)

/** Reference-currency display (§17.7). Never piconero, never "ducats". */
data class Money(val minorUnits: Long, val symbol: String = "$", val exponent: Int = 2) {
    /**
     * Honours `exponent` and survives a negative.
     *
     * The first version divided by 100 and formatted two places regardless,
     * ignoring the field beside it — so a zero-decimal currency printed
     * hundredths of itself. It also rendered -150 as "$-1.-50", because the
     * remainder of a negative is negative. A refund is a negative amount and
     * would have shipped looking like that.
     *
     * The decimal mark asks the locale, same as [formatXmr] and for the same
     * scar: `%,d` localises the digits and the grouping, so a literal `.`
     * between the two conversions was the half-localised number again — the
     * home card's "on the way" line disagreeing with the headline figure
     * right above it about what a decimal point is.
     */
    override fun toString(): String {
        var scale = 1L
        repeat(exponent) { scale *= 10 }
        val negative = minorUnits < 0
        val abs = if (negative) -minorUnits else minorUnits
        val whole = abs / scale
        val sign = if (negative) "-" else ""
        if (exponent == 0) return "$sign$symbol%,d".format(whole)
        val frac = abs % scale
        val dot = java.text.DecimalFormatSymbols.getInstance().decimalSeparator
        return "$sign$symbol%,d$dot%0${exponent}d".format(whole, frac)
    }
}

/**
 * The three things §17.2 forbids presenting as one number.
 *
 * Its words: a client that tells the user "this is just spending money" *"has
 * described one half and mislabelled the other."*
 */
data class Accounts(
    /** On this phone, spendable. */
    val float: Float,
    /** Behind a hardware wallet (§4.4). Not spendable from here. */
    val reservePxmr: Long?,
    /** Posted collateral backing `fast/1` capacity (§17.2). Locked until withdrawn. */
    val bondPxmr: Long?,
)
