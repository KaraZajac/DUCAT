package org.ducatproject.ducat

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder

/**
 * Geocoding and routing, over OpenStreetMap's public services.
 *
 * **This file is a privacy trade, and it says so.** Everything else in DUCAT
 * keeps location on the device or coarsened to a §15.12 cell; an address
 * search necessarily sends the address to whoever answers it, and a route
 * request sends both endpoints. The services here are OSM's (Nominatim for
 * addresses, OSRM's public router for routes) — community-run rather than
 * ad-funded, but still a server seeing a query. The UI states it where the
 * feature is used. The v2 answer is offline data; this is the honest v1.
 *
 * Usage policy compliance: a real User-Agent naming the project, and no
 * query storms — searches are debounced by the caller.
 */
object Geo {
    private const val UA = "DUCAT/0.8 (github.com/KaraZajac/DUCAT)"

    data class Hit(val label: String, val latE7: Long, val lonE7: Long)
    data class Route(
        val meters: Long,
        val seconds: Long,
        /** Decoded route geometry, (latE7, lonE7) pairs, for the map. */
        val points: List<Pair<Long, Long>>,
        /** Per-leg (meters, seconds) when routed through waypoints — the
         *  driver's "to the pickup" and "the ride itself", separately. */
        val legs: List<Pair<Long, Long>> = emptyList(),
    )

    /**
     * Address or business → candidates. Blocking; call from IO.
     *
     * `near` biases results toward the user: "Starbucks" means nothing
     * globally and everything locally, so a viewbox (~50 km) around the fix
     * is sent, unbounded — prefer nearby, never refuse the airport across
     * town. Labels lead with the POI's name when it has one, because a
     * business is looked up by what the sign says, not by its street number.
     */
    fun search(query: String, near: Pair<Long, Long>? = null): List<Hit> {
        val q = URLEncoder.encode(query, "UTF-8")
        val bias = near?.let { (la, lo) ->
            // Snapped to a coarse grid first. The box is what biases the
            // search, but a box is symmetric — average its corners and you
            // have its centre, and the centre used to be the fix itself to
            // four decimal places, which is about eleven metres. Nominatim,
            // and Nominatim's logs, learned where somebody was standing every
            // time they typed a letter into a search field.
            //
            // Snapping costs the search nothing. The box is ninety kilometres
            // across; moving its centre by up to five changes which results
            // rank higher not at all, and what leaves the phone is a cell
            // coarser than the ~5 km one §16.17 allows on a public board.
            val lat = coarse(la / 1e7); val lon = coarse(lo / 1e7)
            // Locale.US, non-negotiably: a comma-decimal locale would format
            // 48.85 as "48,85" and the URL's own commas stop meaning anything.
            "&viewbox=%.4f,%.4f,%.4f,%.4f&bounded=0".format(
                java.util.Locale.US, lon - 0.45, lat + 0.45, lon + 0.45, lat - 0.45,
            )
        } ?: ""
        val body = get(
            "https://nominatim.openstreetmap.org/search?q=$q&format=jsonv2&limit=6$bias"
        ) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(body)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                val name = o.optString("name")
                val display = o.optString("display_name")
                // "Name — street, town" beats a nine-part administrative
                // genealogy. Three parts of context is what a person scans.
                val context = display.split(", ")
                    .filterNot { it == name }.take(3).joinToString(", ")
                Hit(
                    label = when {
                        name.isNotBlank() && context.isNotBlank() -> "$name — $context"
                        name.isNotBlank() -> name
                        else -> display
                    }.take(90),
                    latE7 = (o.getString("lat").toDouble() * 1e7).toLong(),
                    lonE7 = (o.getString("lon").toDouble() * 1e7).toLong(),
                )
            }
        }.getOrElse { emptyList() }
    }

    /**
     * Straight-line metres between two points.
     *
     * **As the crow flies, and only ever offered as that.** A search can turn
     * up a dozen branches of the same chain and the one thing a person needs
     * in order to choose is which is nearest — but routing a dozen candidates
     * is a dozen calls to somebody else's router for answers eleven of which
     * get thrown away. The driving figure arrives on the next screen, from
     * [route], where it is one call and where it is what the fare is quoted
     * from; this is the cheap comparison that happens before that.
     *
     * Haversine on a spherical earth. Good to about half a percent, which is
     * a rounding error against a number shown to one decimal place, and it
     * needs no projection and no network.
     */
    fun metersBetween(aLatE7: Long, aLonE7: Long, bLatE7: Long, bLonE7: Long): Double {
        val r = 6_371_000.0
        val la1 = Math.toRadians(aLatE7 / 1e7)
        val la2 = Math.toRadians(bLatE7 / 1e7)
        val dLa = la2 - la1
        val dLo = Math.toRadians((bLonE7 - aLonE7) / 1e7)
        val h = kotlin.math.sin(dLa / 2).let { it * it } +
            kotlin.math.cos(la1) * kotlin.math.cos(la2) *
            kotlin.math.sin(dLo / 2).let { it * it }
        return 2 * r * kotlin.math.asin(kotlin.math.sqrt(h).coerceAtMost(1.0))
    }

    /** Driving route between two points. Blocking; call from IO. */
    fun route(fromLatE7: Long, fromLonE7: Long, toLatE7: Long, toLonE7: Long): Route? =
        routeVia(listOf(fromLatE7 to fromLonE7, toLatE7 to toLonE7))

    /** A route through every waypoint in order — the driver's whole job is
     *  me → pickup → destination, and the legs come back separately. */
    fun routeVia(waypoints: List<Pair<Long, Long>>): Route? {
        if (waypoints.size < 2) return null
        val coords = waypoints.joinToString(";") { (la, lo) ->
            "%f,%f".format(java.util.Locale.US, lo / 1e7, la / 1e7)
        }
        val body = get(
            "https://router.project-osrm.org/route/v1/driving/$coords" +
                "?overview=full&geometries=geojson"
        ) ?: return null
        return runCatching {
            val r = JSONObject(body).getJSONArray("routes").getJSONObject(0)
            val line = r.getJSONObject("geometry").getJSONArray("coordinates")
            val legsArr = r.getJSONArray("legs")
            Route(
                meters = r.getDouble("distance").toLong(),
                seconds = r.getDouble("duration").toLong(),
                points = (0 until line.length()).map { i ->
                    val pt = line.getJSONArray(i)
                    (pt.getDouble(1) * 1e7).toLong() to (pt.getDouble(0) * 1e7).toLong()
                },
                legs = (0 until legsArr.length()).map { i ->
                    val l = legsArr.getJSONObject(i)
                    l.getDouble("distance").toLong() to l.getDouble("duration").toLong()
                },
            )
        }.getOrNull()
    }

    /**
     * A degree tenth — about eleven kilometres, and less near the poles.
     *
     * Rounding, not truncation: truncating always moves toward the equator and
     * toward the prime meridian, which is a bias somebody could unpick. And
     * the result is a grid point, so repeated searches from one place report
     * one cell rather than a track through it.
     */
    internal fun coarse(deg: Double): Double = Math.round(deg * 10.0) / 10.0

    private fun get(url: String): String? = runCatching {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.setRequestProperty("User-Agent", UA)
        conn.connectTimeout = 10_000
        conn.readTimeout = 15_000
        conn.inputStream.bufferedReader().use { it.readText() }
    }.onFailure { DucatLog.w("Geo", "fetch: ${it.message}") }.getOrNull()
}
