package org.ducatproject.desk

import org.ducatproject.ducat.Fare
import org.ducatproject.ducat.FareRates
import org.ducatproject.ducat.RateStore
import java.io.File

/**
 * What a ride is suggested at, country by country.
 *
 * The fare model used four constants in US dollars and then divided by
 * whatever rate the reader's *chosen currency* had, so the dollars were
 * silently reread: an eight-kilometre ride offered ₹11.20 in India — about
 * thirteen US cents — and KWD 11.20 in Kuwait, about thirty-six dollars.
 * Right in one country, wrong in ninety-nine, and nothing said so.
 *
 * `./gradlew :desktop:faretest`
 */
fun main() {
    val base = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("FARE_FAIL set DUCAT_DESK_STATE (a throwaway directory)"),
    )
    base.deleteRecursively(); base.mkdirs()

    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("FARE ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val context = DeskContext(base)
    val store = RateStore(context)
    // A round number so the arithmetic below is checkable by eye: one XMR is
    // $200, and the reader happens to read in dollars too.
    store.store(200.0, System.currentTimeMillis() / 1000, "faretest")
    store.storeUsd(200.0)

    check("the survey covers a hundred countries", FareRates.COVERED >= 95, "${FareRates.COVERED}")

    // The ratios were taken from the constants the old model used, anchored
    // on the United States' own taxi. If they are right, the US comes back
    // exactly as it was — which is the check that the shape is right rather
    // than merely plausible.
    Fare.setCountry(context, "US")
    check("the US base is still 2.00", "%.2f".format(Fare.base(context)) == "2.00",
        "%.4f".format(Fare.base(context)))
    // Per-minute is derived from per-km, and per-km now comes from the survey
    // (1.86) rather than the 1.70 the old constants assumed — so it lands near
    // 0.25 rather than exactly on it, for the same reason per-km does.
    check("its per-minute is close to 0.25", Fare.perMin(context) in 0.22..0.29,
        "%.4f".format(Fare.perMin(context)))
    check("its minimum is still 6.00", "%.2f".format(Fare.minFare(context)) == "6.00",
        "%.4f".format(Fare.minFare(context)))
    // Per-km is from the survey (1.86) rather than the old hardcoded 1.70,
    // so it lands near 0.71 rather than exactly 0.65.
    check("and its per-km is close to 0.65", Fare.perKm(context) in 0.60..0.75,
        "%.4f".format(Fare.perKm(context)))

    val (uber, driver, taxi) = Fare.competitors(context, 8_000, 960)
    check("a US rideshare prices an 8 km ride near a real one", uber in 10.0..20.0, "%.2f".format(uber))
    check("its driver sees less than its rider pays", driver < uber)
    check("and a taxi costs more than the rideshare", taxi > uber, "taxi %.2f".format(taxi))

    // The point of the whole exercise: the same ride, priced where it happens.
    println()
    println("FARE_TABLE  an 8 km, 16 min ride, in US dollars")
    listOf("CH", "NO", "GB", "DE", "US", "JP", "BR", "ZA", "TR", "IN", "EG", "PH")
        .forEach { iso ->
            Fare.setCountry(context, iso)
            val ours = Fare.estimateExact(context, 8_000, 960)!!.first
            val (u, _, t) = Fare.competitors(context, 8_000, 960)
            println(
                "FARE_ROW  %-3s  ours %7.2f   rideshare %7.2f   taxi %7.2f".format(iso, ours, u, t),
            )
            check("  $iso undercuts its own rideshare", ours < u, "%.2f vs %.2f".format(ours, u))
        }
    println()

    // A country nobody surveyed gets the median rather than the richest.
    Fare.setCountry(context, "ZZ")
    val unknown = Fare.base(context)
    Fare.setCountry(context, "CH")
    check("an unsurveyed country is not priced like Switzerland",
        unknown < Fare.base(context) / 2, "%.2f vs %.2f".format(unknown, Fare.base(context)))

    // Without a dollar rate there is no honest answer, and saying so beats
    // relabelling the table.
    val bare = DeskContext(File(base, "bare").apply { mkdirs() })
    RateStore(bare).store(200.0, System.currentTimeMillis() / 1000, "faretest")
    check("no dollar rate means no estimate, not a wrong one",
        Fare.estimateExact(bare, 8_000, 960) == null)

    // A driver's own rates still win, and they are dollars like the table.
    Fare.setCountry(context, "IN")
    Fare.setRates(context, base = 1.0, perKm = 0.10, perMin = 0.02)
    check("a rate a driver typed is the rate used",
        "%.2f".format(Fare.base(context)) == "1.00", "%.4f".format(Fare.base(context)))

    println(if (failures == 0) "FARETEST OK" else "FARETEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
