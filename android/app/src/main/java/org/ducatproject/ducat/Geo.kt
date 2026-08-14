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
    )

    /** Address → candidates. Blocking; call from IO. */
    fun search(query: String): List<Hit> {
        val q = URLEncoder.encode(query, "UTF-8")
        val body = get("https://nominatim.openstreetmap.org/search?q=$q&format=jsonv2&limit=5")
            ?: return emptyList()
        return runCatching {
            val arr = JSONArray(body)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                Hit(
                    label = o.optString("display_name").take(80),
                    latE7 = (o.getString("lat").toDouble() * 1e7).toLong(),
                    lonE7 = (o.getString("lon").toDouble() * 1e7).toLong(),
                )
            }
        }.getOrElse { emptyList() }
    }

    /** Driving route between two points. Blocking; call from IO. */
    fun route(fromLatE7: Long, fromLonE7: Long, toLatE7: Long, toLonE7: Long): Route? {
        val coords = "%f,%f;%f,%f".format(
            fromLonE7 / 1e7, fromLatE7 / 1e7, toLonE7 / 1e7, toLatE7 / 1e7,
        )
        val body = get(
            "https://router.project-osrm.org/route/v1/driving/$coords" +
                "?overview=full&geometries=geojson"
        ) ?: return null
        return runCatching {
            val r = JSONObject(body).getJSONArray("routes").getJSONObject(0)
            val line = r.getJSONObject("geometry").getJSONArray("coordinates")
            Route(
                meters = r.getDouble("distance").toLong(),
                seconds = r.getDouble("duration").toLong(),
                points = (0 until line.length()).map { i ->
                    val pt = line.getJSONArray(i)
                    (pt.getDouble(1) * 1e7).toLong() to (pt.getDouble(0) * 1e7).toLong()
                },
            )
        }.getOrNull()
    }

    private fun get(url: String): String? = runCatching {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.setRequestProperty("User-Agent", UA)
        conn.connectTimeout = 10_000
        conn.readTimeout = 15_000
        conn.inputStream.bufferedReader().use { it.readText() }
    }.onFailure { DucatLog.w("Geo", "fetch: ${it.message}") }.getOrNull()
}
