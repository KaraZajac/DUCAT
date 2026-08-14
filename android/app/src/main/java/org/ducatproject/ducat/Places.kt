package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Saved places, because a destination needs coordinates and this app calls no
 * geocoder: an address search would hand every destination the user ever
 * types to whichever server answers it. A place is saved by standing in it
 * once (or pasting a geocell), named by its owner, kept locally.
 */
class PlaceStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    data class Place(val name: String, val latE7: Long, val lonE7: Long)

    fun all(): List<Place> {
        val raw = prefs.getString("places_v1", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrElse { return emptyList() }
        return (0 until arr.length()).mapNotNull { i ->
            runCatching {
                val o = arr.getJSONObject(i)
                Place(o.getString("n"), o.getLong("la"), o.getLong("lo"))
            }.getOrNull()
        }
    }

    fun add(p: Place) {
        val list = all().filterNot { it.name == p.name } + p
        val arr = JSONArray()
        list.forEach {
            arr.put(JSONObject().put("n", it.name).put("la", it.latE7).put("lo", it.lonE7))
        }
        prefs.edit().putString("places_v1", arr.toString()).apply()
        ContactStore.bump()
    }

    fun remove(name: String) {
        val arr = JSONArray()
        all().filterNot { it.name == name }.forEach {
            arr.put(JSONObject().put("n", it.name).put("la", it.latE7).put("lo", it.lonE7))
        }
        prefs.edit().putString("places_v1", arr.toString()).apply()
        ContactStore.bump()
    }
}

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

    fun base(context: Context) = pref(context).getFloat("fare_base", 2.50f).toDouble()
    fun perKm(context: Context) = pref(context).getFloat("fare_per_km", 1.50f).toDouble()
    fun perMin(context: Context) = pref(context).getFloat("fare_per_min", 0.30f).toDouble()

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
        val fiat = base(context) + perKm(context) * km + perMin(context) * mins
        val pxmr = ((fiat / rate) * 1e12).toLong()
        return fiat to pxmr
    }

    private fun pref(context: Context) =
        context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)
}
