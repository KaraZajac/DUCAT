package org.ducatproject.desk

import org.ducatproject.ducat.ui.moneroUri

/**
 * A code that names an amount is read for it.
 * `./gradlew :desktop:monerouri`.
 *
 * DUCAT writes `tx_amount` on every kiosk order and never read it back. The
 * scanner took `substringBefore("?")` and dropped the query with it, so a
 * payer who scanned the till's own "no DUCAT? pay with a Monero wallet" code
 * arrived at a pay screen with an empty amount box and had to read the figure
 * off the merchant's screen and type it in again.
 *
 * That is worse than clumsy at a kiosk. An order is attributed by its exact
 * amount — down to a sub-microXMR tag that makes it unique among identical
 * baskets — so a hand-typed round number is a payment that arrives, leaves the
 * customer believing they have paid, and can never be matched to the order it
 * paid for.
 *
 * The tag is why the parse has to be exact rather than to two decimals: it
 * lives in the seventh decimal place and rounding it away rejoins the
 * collision it exists to prevent.
 */
private const val ADDR =
    "72g1LuCiK5aBEA8g8kXwSc2fnD858dcYD2kD8yaUcokAbYGwvSTs6Gx8ZYJ3gq41JGRkUdzZPLuPnVdxaMVcqPFhRZuNuQs"
private const val STD =
    "52YkVazrksg3ZGtDGwjBAr2z12pU1RJBWgVbbaPFsj6MVwgSkup3Yqi6HYW72WmTSm95M8HHcH3XvZwSqi6apEAfDPj8FkV"

fun main() {
    // A kiosk order, verbatim off the screen this was found on.
    val (a, amt) = moneroUri("monero:$ADDR?tx_amount=0.016508807000")!!
    check(a == ADDR) { "URI_FAIL address" }
    check(amt == 16_508_807_000L) { "URI_FAIL the attribution tag was rounded off: $amt" }

    // A donation code names no amount: the payer decides.
    val (b, none) = moneroUri("monero:$STD")!!
    check(b == STD && none == 0L) { "URI_FAIL invented an amount" }

    // A bare address, no scheme — what a pasted address looks like.
    check(moneroUri(STD) == (STD to 0L)) { "URI_FAIL bare address" }
    check(moneroUri("  $STD  ") == (STD to 0L)) { "URI_FAIL whitespace" }

    // Other parameters, in any order, and the amount still found.
    check(moneroUri("monero:$ADDR?tx_description=Order%20%2312&tx_amount=1.5")?.second
        == 1_500_000_000_000L) { "URI_FAIL amount after another field" }
    check(moneroUri("monero:$ADDR?tx_amount=1.5&tx_description=x")?.second
        == 1_500_000_000_000L) { "URI_FAIL amount before another field" }

    // A parameter that merely ends in the same letters is not the amount.
    check(moneroUri("monero:$ADDR?not_tx_amount=9")?.second == 0L) { "URI_FAIL prefix match" }

    // Nothing usable: refuse rather than pay a truncated address.
    check(moneroUri("monero:") == null) { "URI_FAIL empty" }
    check(moneroUri("monero:${ADDR.take(40)}?tx_amount=1") == null) { "URI_FAIL short address" }
    check(moneroUri("ducat:card/abc") == null) { "URI_FAIL a card is not an address" }
    check(moneroUri("") == null) { "URI_FAIL blank" }

    // Garbage in the amount leaves the payer to type one, rather than
    // refusing an address that is perfectly good.
    check(moneroUri("monero:$ADDR?tx_amount=abc") == (ADDR to 0L)) { "URI_FAIL unparseable amount" }
    check(moneroUri("monero:$ADDR?tx_amount=-1") == (ADDR to 0L)) { "URI_FAIL negative" }
    check(moneroUri("monero:$ADDR?tx_amount=0") == (ADDR to 0L)) { "URI_FAIL zero" }

    // More precision than piconero is not a number Monero can send.
    check(moneroUri("monero:$ADDR?tx_amount=0.0000000000001")?.second == 0L) {
        "URI_FAIL sub-piconero"
    }

    println("URI_OK a code that names an amount is read for it")
}
