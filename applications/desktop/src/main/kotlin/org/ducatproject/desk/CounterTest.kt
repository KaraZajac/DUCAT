package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.Catalogue
import org.ducatproject.ducat.Orders
import org.ducatproject.ducat.Pin
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.WalletStore
import java.io.File

/**
 * The counter's three new pieces, driven the only way that proves anything:
 * by running them.
 *
 * A saved menu, a kiosk order and a PIN went in together, and every one of
 * them is logic a screen sits on top of — which means every one of them can be
 * wrong in a way that renders perfectly. This exercises the parts a Compose
 * preview cannot reach: what a price does when the rate is missing, whether an
 * order can be paid twice, whether a lockout actually locks.
 *
 * `./gradlew :desktop:countertest` — throwaway directory, like the vault's.
 */
fun main() {
    val base = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("COUNTER_FAIL set DUCAT_DESK_STATE (a throwaway directory)"),
    )
    base.deleteRecursively()
    base.mkdirs()

    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("COUNTER ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val context = DeskContext(base)

    // --- the menu ---------------------------------------------------------

    check("an empty catalogue is empty, not a crash", Catalogue.all(context).isEmpty())

    val rates = RateStore(context)
    val coffee = Catalogue.draft(context, "Flat white", "3.20")

    // Before any rate exists, a price in pounds is not a price in monero, and
    // saying so is the whole point of Snag.NoRate.
    val noRate = Catalogue.price(context, coffee)
    check(
        "no rate means no price",
        noRate.isFailure &&
            (noRate.exceptionOrNull() as? Catalogue.SnagException)?.snag == Catalogue.Snag.NoRate,
        noRate.exceptionOrNull()?.message ?: "it priced it anyway",
    )

    // 150 of whatever this desk calls its currency, per XMR.
    rates.store(150.0, System.currentTimeMillis() / 1000, "tilltest")
    val priced = Catalogue.price(context, coffee)
    check("with a rate it prices", priced.isSuccess, priced.exceptionOrNull()?.message ?: "")
    // 3.20 / 150 = 0.0213333… XMR, truncated at twelve places.
    check(
        "and prices it correctly",
        priced.getOrNull()?.pxmr == 21_333_333_333L,
        "got ${priced.getOrNull()?.pxmr}",
    )
    check("a fresh rate is not stale", (priced.getOrNull()?.staleSecs ?: -1L) < 5)

    // The stamp is in seconds. Storing milliseconds here would make every
    // price look 55 years stale, which is the kind of thing that only shows up
    // in front of a customer.
    rates.store(150.0, System.currentTimeMillis() / 1000 - 7_200, "tilltest")
    check(
        "an old rate reports its age in seconds",
        (Catalogue.price(context, coffee).getOrNull()?.staleSecs ?: 0L) in 7_000..7_400,
        "got ${Catalogue.price(context, coffee).getOrNull()?.staleSecs}",
    )
    rates.store(150.0, System.currentTimeMillis() / 1000, "tilltest")

    check(
        "nonsense is unpriceable",
        (Catalogue.price(context, coffee.copy(price = "free")).exceptionOrNull()
            as? Catalogue.SnagException)?.snag == Catalogue.Snag.Unpriceable,
    )
    check(
        "a price in another currency is refused, not reinterpreted",
        (Catalogue.price(context, coffee.copy(currency = "ZWL")).exceptionOrNull()
            as? Catalogue.SnagException)?.snag == Catalogue.Snag.WrongCurrency,
    )
    // The overflow that used to become exactly one monero.
    check(
        "an absurd price does not wrap into a plausible one",
        Catalogue.price(context, coffee.copy(price = "18446744073709551617")).isFailure,
    )
    // A comma is a decimal point in most of the languages this ships in.
    check(
        "a comma decimal is understood",
        Catalogue.price(context, coffee.copy(price = "3,20")).getOrNull()?.pxmr == 21_333_333_333L,
    )

    // --- what people actually type ---------------------------------------
    //
    // Every money field used to filter on `isDigit() || '.' || ','`, which
    // keeps Arabic digits and deletes the Arabic decimal separator beside
    // them. Each of these is a price of 3.20 as its own keyboard writes it.
    val threeTwenty = 21_333_333_333L
    listOf(
        "ASCII" to "3.20",
        "comma decimal (de, fr, ru…)" to "3,20",
        "Arabic-Indic" to "٣٫٢٠",
        "Persian" to "۳٫۲۰",
        "Persian with a full stop" to "۳.۲۰",
        "Devanagari" to "३.२०",
        "Thai" to "๓.๒๐",
        "fullwidth" to "３．２０",
        "grouped, Arabic" to "٣٫٢٠",
        "padded" to "  3.20  ",
    ).forEach { (name, typed) ->
        val kept = typed.filter { org.ducatproject.ducat.Amounts.isNumberChar(it) }
        val got = Catalogue.price(context, coffee.copy(price = kept)).getOrNull()?.pxmr
        check(
            "a price typed in $name survives the field",
            got == threeTwenty,
            "typed '$typed' kept '$kept' got $got, wanted $threeTwenty",
        )
    }
    check(
        "and a thousands mark is not a decimal point",
        Catalogue.price(context, coffee.copy(price = "1٬234")).getOrNull()?.pxmr ==
            Catalogue.price(context, coffee.copy(price = "1234")).getOrNull()?.pxmr,
    )
    check(
        "letters are still not an amount",
        org.ducatproject.ducat.Amounts.parse("12abc") == null,
    )
    check("and neither is nothing", org.ducatproject.ducat.Amounts.parse("   ") == null)

    Catalogue.put(context, coffee)
    Catalogue.put(context, Catalogue.draft(context, "Croissant", "2.50"))
    check("two things on the menu", Catalogue.live(context).size == 2)
    check("saved as typed", Catalogue.all(context).first { it.id == coffee.id }.price == "3.20")

    Catalogue.put(context, coffee.copy(name = "Flat white (large)", price = "3.80"))
    check("editing replaces rather than duplicates", Catalogue.live(context).size == 2)
    check(
        "and the edit stuck",
        Catalogue.all(context).first { it.id == coffee.id }.price == "3.80",
    )

    Catalogue.put(context, coffee.copy(price = "3.80", archived = true))
    check("archived leaves the till", Catalogue.live(context).size == 1)
    check("but not the records", Catalogue.all(context).size == 2)

    // The menu must outlive the process, or a stall rebuilds it every morning.
    check(
        "the menu survives a fresh context",
        Catalogue.all(DeskContext(base)).size == 2,
    )

    // --- orders -----------------------------------------------------------

    // An order needs somewhere to be paid, which needs a wallet.
    // A valid ed25519 scalar (0x0a0a… little-endian is below the group
    // order), so addressFor really derives subaddresses rather than
    // silently falling back to the main address for every order.
    WalletStore(context).save("5TillTest", "0a".repeat(32), 2_190_000uL, true)

    check("no orders yet", Orders.all(context).isEmpty())

    val basket = listOf(BillItem("Flat white", 21_333_333_333L), BillItem("Croissant", 16_666_666_666L))
    val plain = basket.sumOf { it.amountPxmr }
    val first = Orders.place(context, basket)

    check("an order is placed", Orders.all(context).size == 1)
    check("it keeps its lines", first.lines.size == 2)
    check("it is waiting", first.state == Orders.State.Awaiting)
    check(
        "its total carries a tag above the bill, never below",
        first.totalPxmr >= plain && first.totalPxmr < plain + 1_000_000L,
        "bill $plain, asked ${first.totalPxmr}",
    )
    check("and a customer would not notice the tag", first.totalPxmr - plain < 1_000_000L)

    val uri = Orders.payUri(first)
    check("the pay code is a monero uri", uri.startsWith("monero:"), uri)
    check("addressed to this order", uri.contains(first.address), uri)
    // Not `contains("tx_amount=")`. That assertion passed while the amount
    // behind it was rounded to six places — which rounds off exactly the noise
    // the order recognises its own payment by, so every kiosk order would have
    // waited for ever with its customer standing there having paid. Read the
    // number back and compare it to the piconero.
    val asked = uri.substringAfter("tx_amount=").toBigDecimalOrNull()
    check(
        "for the exact amount, to the piconero",
        asked != null && asked.movePointRight(12).toBigIntegerExact().toLong() == first.totalPxmr,
        "asked $asked for ${first.totalPxmr} pXMR",
    )

    // The same code on a Persian phone. `%d` is one of the conversions Java
    // localizes, so a payment request built with the default locale came out
    // as ۰.۰۳۸۰۰۰۲۳۵۵۴۷ — digits no wallet can parse, on a build that ships
    // Persian and Arabic. Nothing about this is visible from an English desk.
    val wasDefault = java.util.Locale.getDefault()
    try {
        java.util.Locale.setDefault(java.util.Locale.forLanguageTag("fa-IR"))
        val farsi = Orders.payUri(first)
        check(
            "the pay code stays ASCII wherever the phone is",
            farsi.all { it.code < 128 },
            farsi,
        )
        check("and is still the same amount", farsi == uri, farsi)
    } finally {
        java.util.Locale.setDefault(wasDefault)
    }

    val second = Orders.place(context, basket)
    check("a second order of the same basket", Orders.all(context).size == 2)
    check(
        "is asked for a different amount, so its payment is its own",
        second.totalPxmr != first.totalPxmr,
        "both ${first.totalPxmr}",
    )
    check("and is called by a different number", second.number != first.number)
    check(
        "and shown a different address, so a queue does not share one",
        second.address != first.address,
    )


    // Numbers wrap rather than growing forever, and must not collide with a
    // number still on the board.
    check("numbers stay small enough to call across a room", second.number in 1..999)

    Orders.update(context, first.copy(state = Orders.State.Seen, seenTx = "ab".repeat(32)))
    val back = Orders.all(context).first { it.id == first.id }
    check("a sighting is remembered", back.state == Orders.State.Seen)
    check("with the transaction that made it", back.seenTx == "ab".repeat(32))
    check("and the other order is untouched",
        Orders.all(context).first { it.id == second.id }.state == Orders.State.Awaiting)

    check(
        "orders survive a fresh context",
        Orders.all(DeskContext(base)).size == 2,
    )

    // Nobody pays for ever. An order left awaiting keeps the poller reading
    // the mempool every pass, so giving up is what stops a Saturday's worth of
    // abandoned baskets from scanning until somebody force-stops the app.
    Orders.expire(context)
    check(
        "a fresh order is not given up on",
        Orders.all(context).first { it.id == second.id }.state == Orders.State.Awaiting,
    )
    Orders.update(
        context,
        Orders.all(context).first { it.id == second.id }
            .copy(placedAt = System.currentTimeMillis() / 1000 - 3_600),
    )
    Orders.expire(context)
    check(
        "an hour-old one is",
        Orders.all(context).first { it.id == second.id }.state == Orders.State.Abandoned,
    )
    check(
        "and giving up does not disturb a sighted order",
        Orders.all(context).first { it.id == first.id }.state == Orders.State.Seen,
    )

    // The addresses rotate rather than accumulating. Every one allocated takes
    // a permanent subaddress index that the wallet scanner checks against
    // every output it ever sees, so "a fresh one per order" is a bill that
    // arrives months later as a wallet that will not sync.
    val before = org.ducatproject.ducat.WalletStore(context).subaddressCount()
    repeat(80) { Orders.place(context, listOf(BillItem("x", 1_000_000_000L))) }
    val after = org.ducatproject.ducat.WalletStore(context).subaddressCount()
    check(
        "eighty more orders do not mean eighty more subaddresses",
        after - before <= 64,
        "grew by ${after - before}",
    )

    // --- an order that rides the protocol ---------------------------------

    // What the Order button reaches for now. No address and no noise: this
    // basket does not know whose it is yet, and guessing is what the bare
    // `monero:` code did.
    val pending = Orders.begin(context, basket)
    check("a begun order is unpaired", pending.unpaired)
    check("with nothing to pay to yet", pending.address.isEmpty())
    check("and no tab yet", pending.tabId == null)
    check(
        "its total is the bill, untagged",
        pending.totalPxmr == plain,
        "asked ${pending.totalPxmr} for a $plain bill",
    )

    // The anonymous machinery must not touch it. Amount-matching an order
    // with no noise in its total against a mempool full of round numbers is
    // exactly how somebody else's coffee gets marked paid.
    Orders.poolSight(context, "http://127.0.0.1:1")
    check(
        "the pool scan leaves an unpaired order alone",
        Orders.all(context).first { it.id == pending.id }.state == Orders.State.Awaiting,
    )

    // Once bound, the tab is the record. Bound by hand here — settling needs
    // a contact and a live node — but the mapping is what a kiosk screen
    // reads on every frame, so it is worth pinning.
    val tabs = org.ducatproject.ducat.TabStore(context)
    val tab = tabs.open("ab".repeat(16), Orders.ORIGIN)
    val bound = pending.copy(tabId = tab.id, personaHex = "ab".repeat(16))
    Orders.update(context, bound)

    check("a billed tab reads as awaiting", Orders.stateOf(context, bound) == Orders.State.Awaiting)
    tabs.update(tabs.get(tab.id)!!.copy(state = "settled", seenTx = "cd".repeat(32)))
    check("sighted in the pool reads as seen", Orders.stateOf(context, bound) == Orders.State.Seen)
    tabs.update(tabs.get(tab.id)!!.copy(state = "paid"))
    check("paid reads as confirmed", Orders.stateOf(context, bound) == Orders.State.Confirmed)
    tabs.update(tabs.get(tab.id)!!.copy(state = "paid_oob"))
    check(
        "settled outside DUCAT is still settled",
        Orders.stateOf(context, bound) == Orders.State.Confirmed,
    )
    tabs.update(tabs.get(tab.id)!!.copy(state = "cancelled"))
    check(
        "a withdrawn bill reads as walked away",
        Orders.stateOf(context, bound) == Orders.State.Abandoned,
    )

    // Giving up is the tab's call once there is a tab: the customer may be
    // paying right now, and the bill is already in their conversation.
    Orders.update(
        context,
        bound.copy(placedAt = System.currentTimeMillis() / 1000 - 7_200),
    )
    Orders.expire(context)
    check(
        "an old bound order is not abandoned behind the tab's back",
        Orders.all(context).first { it.id == bound.id }.state != Orders.State.Abandoned,
    )

    // --- two threads, one list --------------------------------------------
    //
    // The poller sights payments and abandons walked-away baskets while a
    // screen begins and binds orders. Every write rewrites the whole array, so
    // a lost update here is a paid order reverting to awaiting, or an order
    // vanishing while the tab that bills it survives.
    run {
        val before = Orders.all(context).size
        val n = 40
        val pool = java.util.concurrent.Executors.newFixedThreadPool(8)
        val gate = java.util.concurrent.CountDownLatch(1)
        val done = java.util.concurrent.CountDownLatch(n)
        repeat(n) { i ->
            pool.submit {
                gate.await()
                runCatching {
                    Orders.update(
                        context,
                        Orders.Order(
                            id = "race-$i", number = i % 999 + 1,
                            lines = listOf(BillItem("x", 1_000L)),
                            totalPxmr = 1_000L, address = "", state = Orders.State.Awaiting,
                            placedAt = System.currentTimeMillis() / 1000,
                        ),
                    )
                }
                done.countDown()
            }
        }
        gate.countDown()
        done.await(60, java.util.concurrent.TimeUnit.SECONDS)
        pool.shutdown()
        val after = Orders.all(context)
        val kept = after.count { it.id.startsWith("race-") }
        check(
            "forty concurrent writes all survive",
            kept == n,
            "kept $kept of $n (list went $before → ${after.size})",
        )
    }

    // --- the PIN ----------------------------------------------------------

    check("no PIN to begin with", !Pin.isSet(context))
    check(
        "and verifying against nothing says so",
        Pin.verify(context, "0000") == Pin.Verdict.Unset,
    )

    Pin.set(context, "1234")
    check("a PIN is set", Pin.isSet(context))
    check("the right one passes", Pin.verify(context, "1234") == Pin.Verdict.Ok)
    check("the wrong one does not", Pin.verify(context, "9999") is Pin.Verdict.Wrong)

    // A PIN is a number, not a glyph. The gate folds whatever the keypad
    // produced to ASCII before hashing, so somebody who set theirs on a
    // Persian keyboard and later typed it on an English one is not locked out
    // of their own wallet by a font.
    check(
        "a PIN is the same number in any script",
        org.ducatproject.ducat.Amounts.typedNumber("۱۲۳۴").filter { it in '0'..'9' } == "1234",
    )
    check(
        "and so are Arabic-Indic digits",
        org.ducatproject.ducat.Amounts.typedNumber("١٢٣٤").filter { it in '0'..'9' } == "1234",
    )

    // The verifier is a hash over a salt: the digits must not be on disk.
    val onDisk = base.walkTopDown().filter { it.isFile }
        .joinToString("\n") { runCatching { it.readText() }.getOrDefault("") }
    check("and the digits are nowhere on disk", !onDisk.contains("\"1234\""))

    // Passing resets the count, so a day of ordinary use never creeps toward
    // a lockout.
    repeat(3) { Pin.verify(context, "9999") }
    check("the right PIN clears the failures", Pin.verify(context, "1234") == Pin.Verdict.Ok)
    check("really clears them", Pin.verify(context, "9999") == Pin.Verdict.Wrong(3))

    // Four free tries, then waiting.
    repeat(3) { Pin.verify(context, "9999") }
    val locked = Pin.verify(context, "9999")
    check("guessing eventually locks", locked is Pin.Verdict.Locked, "got $locked")
    check("and the lock has a duration", (locked as? Pin.Verdict.Locked)?.secondsLeft?.let { it > 0 } == true)
    check(
        "a lock refuses even the right PIN",
        Pin.verify(context, "1234") is Pin.Verdict.Locked,
        "otherwise the lockout is decorative",
    )
    check("the lock is readable without guessing", Pin.lockedFor(context) > 0)

    // A lockout a force-stop clears is not a lockout.
    check(
        "and it survives a fresh context",
        Pin.verify(DeskContext(base), "1234") is Pin.Verdict.Locked,
    )

    println(if (failures == 0) "COUNTERTEST OK" else "COUNTERTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
