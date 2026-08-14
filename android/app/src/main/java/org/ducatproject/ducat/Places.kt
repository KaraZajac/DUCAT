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
     * The defaults are positioned, not plucked: ~15% under a rideshare's
     * rider-side rates. The arithmetic that makes both sides win at once is
     * the platform's absent ~30% take — Uber's rider price and its driver
     * payout differ by that margin, and pricing inside the gap means the
     * rider pays less than Uber *and* the driver (who keeps 100% here, the
     * network fee being ~a cent) earns more than Uber would have paid them.
     * A taxi is not the benchmark; it loses to both on every axis.
     */
    fun base(context: Context) = pref(context).getFloat("fare_base", 2.00f).toDouble()
    fun perKm(context: Context) = pref(context).getFloat("fare_per_km", 0.65f).toDouble()
    fun perMin(context: Context) = pref(context).getFloat("fare_per_min", 0.25f).toDouble()
    fun minFare(context: Context) = pref(context).getFloat("fare_min", 6.00f).toDouble()

    /** What the same ride costs elsewhere, for the line under the estimate:
     *  (rideshare rider price, what its driver would have seen, taxi). */
    fun competitors(meters: Long, seconds: Long): Triple<Double, Double, Double> {
        val km = meters / 1000.0
        val mins = seconds / 60.0
        val uber = (2.00 + 2.75 + 0.75 * km + 0.30 * mins).coerceAtLeast(7.50)
        val uberDriver = uber * 0.71
        val taxi = 3.50 + 1.70 * km + 0.10 * mins
        return Triple(uber, uberDriver, taxi)
    }

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

    /** The same, from a real route's distance and duration (OSRM) — no
     *  circuity guess needed when the road itself has answered. */
    fun estimateExact(context: Context, routeMeters: Long, routeSeconds: Long): Pair<Double, Long>? {
        val rate = RateStore(context).cached()?.first ?: return null
        val km = routeMeters / 1000.0
        val mins = routeSeconds / 60.0
        val fiat = (base(context) + perKm(context) * km + perMin(context) * mins)
            .coerceAtLeast(minFare(context))
        val pxmr = ((fiat / rate) * 1e12).toLong()
        return fiat to pxmr
    }

    private fun pref(context: Context) =
        context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)
}
