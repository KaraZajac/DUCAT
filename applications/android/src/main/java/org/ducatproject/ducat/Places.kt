package org.ducatproject.ducat

import android.content.Context
/**
 * The fare estimate (§15.12): every taxi on Earth prices the same way.
 *
 *     estimate = base + per_km × distance + per_min × time
 *
 * Distance is great-circle × 1.3 — the circuity factor road networks
 * actually measure at — and time assumes ~30 km/h urban. Rates are fiat
 * (that is how people think about cab fare) with editable defaults, and the
 * conversion to piconero snapshots at post time, §15.11's meter rule reused.
 * The number seeds an *offer*; there is no surge because there is nobody to
 * decree one — a driver counter-quotes on a busy night instead.
 */
object Fare {
    const val CIRCUITY = 1.3
    const val AVG_KMH = 30.0

    /**
     * Where the fare rates come from.
     *
     * The device's own region unless somebody has said otherwise. A driver
     * working across a border, or one whose phone is set to another country,
     * can set it; everyone else never sees this.
     */
    fun country(context: Context): String =
        pref(context).getString("fare_country", null)
            ?.takeIf { it.isNotBlank() }
            ?: java.util.Locale.getDefault().country.ifBlank { "US" }

    fun setCountry(context: Context, iso: String) =
        pref(context).edit().putString("fare_country", iso.uppercase()).apply()

    /** The local taxi this country's suggestions are built from. */
    fun local(context: Context): FareRates.Local = FareRates.of(country(context))

    /**
     * The suggested rates, in **US dollars**.
     *
     * Positioned rather than plucked: about fifteen percent under a local
     * rideshare's rider price. The arithmetic that lets both sides win at
     * once is the platform's absent ~30% take — a rideshare's rider price and
     * its driver payout differ by that margin, and pricing inside the gap
     * means the rider pays less than the rideshare *and* the driver, who
     * keeps all of it here, earns more than the rideshare would have paid
     * them. A taxi is not the benchmark; it loses to both on every axis.
     *
     * Derived from the local taxi rather than fixed, because $0.65/km is a
     * bargain in Zurich and unaffordable in Manila. A driver can still type
     * over any of it — see [setRates], which stores in dollars too.
     */
    fun base(context: Context) = stored(context, "fare_base")
        ?: local(context).start * FareRates.OURS_BASE

    fun perKm(context: Context) = stored(context, "fare_per_km")
        ?: local(context).perKm * FareRates.OURS_PER_KM

    fun perMin(context: Context) = stored(context, "fare_per_min")
        ?: local(context).perKm * FareRates.MIN_FROM_KM * FareRates.OURS_PER_MIN

    fun minFare(context: Context) = stored(context, "fare_min")
        ?: local(context).start * FareRates.OURS_MIN

    /** A rate a driver typed, or null to take the country's suggestion. */
    private fun stored(context: Context, key: String): Double? =
        pref(context).getFloat(key, 0f).toDouble().takeIf { it > 0 }

    /**
     * What the same ride costs elsewhere, for the line under the estimate:
     * (rideshare rider price, what its driver would have seen, taxi) — all in
     * US dollars, and all from this country's own taxi.
     */
    fun competitors(context: Context, meters: Long, seconds: Long): Triple<Double, Double, Double> {
        val l = local(context)
        val km = meters / 1000.0
        val mins = seconds / 60.0
        val taxiPerMin = l.perKm * FareRates.MIN_FROM_KM
        val rideshare = (
            l.start * FareRates.RIDESHARE_FIXED +
                l.perKm * FareRates.RIDESHARE_PER_KM * km +
                taxiPerMin * FareRates.RIDESHARE_PER_MIN * mins
            ).coerceAtLeast(l.start * FareRates.RIDESHARE_MIN)
        val taxi = l.start + l.perKm * km + taxiPerMin * mins
        return Triple(rideshare, rideshare * FareRates.DRIVER_SHARE, taxi)
    }

    /** Rates a driver typed, in US dollars like the table. */
    fun setRates(context: Context, base: Double, perKm: Double, perMin: Double) {
        pref(context).edit()
            .putFloat("fare_base", base.toFloat())
            .putFloat("fare_per_km", perKm.toFloat())
            .putFloat("fare_per_min", perMin.toFloat())
            .apply()
    }

    /** (fiat estimate, pxmr estimate), or null with no exchange rate cached. */
    fun estimate(context: Context, straightMeters: Long): Pair<Double, Long>? {
        val km = straightMeters / 1000.0 * CIRCUITY
        return estimateExact(context, (km * 1000).toLong(), (km / AVG_KMH * 3600).toLong())
    }

    /**
     * The same, from a real route's distance and duration (OSRM) — no
     * circuity guess needed when the road itself has answered.
     *
     * Returns the figure in the **reader's** currency and the piconero it
     * comes to. The dollars of the table become piconero through the dollar's
     * own rate, and piconero becomes the reader's currency through theirs;
     * before, the dollars were simply relabelled and an eight-kilometre ride
     * offered thirteen cents in Delhi.
     */
    fun estimateExact(context: Context, routeMeters: Long, routeSeconds: Long): Pair<Double, Long>? {
        val store = RateStore(context)
        val theirs = store.cached()?.first ?: return null
        val usd = store.usdPerXmr() ?: return null
        val km = routeMeters / 1000.0
        val mins = routeSeconds / 60.0
        val dollars = (base(context) + perKm(context) * km + perMin(context) * mins)
            .coerceAtLeast(minFare(context))
        val pxmr = ((dollars / usd) * 1e12).toLong()
        return (dollars / usd) * theirs to pxmr
    }

    /** A dollar figure as piconero, for the competitor line. */
    fun usdToPxmr(context: Context, dollars: Double): Long? =
        RateStore(context).usdPerXmr()?.let { ((dollars / it) * 1e12).toLong() }

    /**
     * A dollar figure in the reader's own currency.
     *
     * The comparison line printed the table's dollars beside the reader's
     * currency code, so "what a rideshare would charge" was a US number
     * wearing a rupee sign. Through piconero, like everything else.
     */
    fun usdToReader(context: Context, dollars: Double): Double? {
        val store = RateStore(context)
        val usd = store.usdPerXmr() ?: return null
        val theirs = store.cached()?.first ?: return null
        return dollars / usd * theirs
    }

    private fun pref(context: Context) =
        securePrefs(context, "ducat_contacts")
}
