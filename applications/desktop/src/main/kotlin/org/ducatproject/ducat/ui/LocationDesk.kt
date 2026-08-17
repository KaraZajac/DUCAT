package org.ducatproject.ducat.ui

import android.content.Context

/**
 * Where this desk is.
 *
 * A phone answers by reading its GPS. A desk has none — and does not need
 * one, because a desk does not move: it is told its position once and that
 * answer stays true. Everything downstream (§15.12's ~1.2 km geocells, a
 * location sent in a message) then works here exactly as it does on a phone,
 * with the honest difference that this fix is a claim its operator typed
 * rather than a satellite's.
 *
 * Unset is unset: `grabFix` reports null rather than guessing a city, and
 * every caller already handles "no fix" because a phone indoors has none.
 */
object DeskLocation {
    private fun prefs(context: Context) =
        context.getSharedPreferences("ducat_desk_place", Context.MODE_PRIVATE)

    /** Latitude and longitude in 1e-7 degrees, or null when never set. */
    fun get(context: Context): Pair<Long, Long>? {
        val p = prefs(context)
        if (!p.contains("lat")) return null
        return p.getLong("lat", 0L) to p.getLong("lon", 0L)
    }

    fun set(context: Context, latE7: Long, lonE7: Long) {
        prefs(context).edit().putLong("lat", latE7).putLong("lon", lonE7).apply()
    }

    fun clear(context: Context) {
        prefs(context).edit().remove("lat").remove("lon").apply()
    }

    /** "52.5200, 13.4050" — what the settings field shows and accepts. */
    fun format(fix: Pair<Long, Long>): String =
        "%.4f, %.4f".format(java.util.Locale.US, fix.first / 1e7, fix.second / 1e7)

    fun parse(text: String): Pair<Long, Long>? {
        val parts = text.split(',', ' ').filter { it.isNotBlank() }
        if (parts.size != 2) return null
        val lat = parts[0].trim().toDoubleOrNull() ?: return null
        val lon = parts[1].trim().toDoubleOrNull() ?: return null
        if (lat !in -90.0..90.0 || lon !in -180.0..180.0) return null
        return (lat * 1e7).toLong() to (lon * 1e7).toLong()
    }
}

/** The desk's half of the phone's grabFix: its stored position, or nothing. */
fun grabFix(context: Context, done: (Pair<Long, Long>?) -> Unit) {
    done(DeskLocation.get(context))
}
