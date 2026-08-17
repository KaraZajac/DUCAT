package org.ducatproject.ducat.ui

// Turning a fix into something a person can send is the same everywhere;
// *getting* the fix is not. The phone reads its GPS (Location.kt); a desk
// does not move, so it is told where it is once (the desk's LocationDesk.kt).

/**
 * One location fix, sent as a link anyone's map can open.
 *
 * Explicitly one-shot: a single fix the user chose to send, never a stream —
 * continuous location is a different feature with a different threat model,
 * and this deliberately is not it.
 */
fun grabLocation(
    context: android.content.Context,
    done: (String?) -> Unit,
) = grabFix(context) { fix ->
    done(fix?.let { (lat, lon) ->
        val la = lat / 1e7; val lo = lon / 1e7
        // Locale.US or a comma-decimal locale mints mlat=52,52000 — a URL
        // no map can open. Same rule as every coordinate URL in Geo.kt.
        val url = "https://www.openstreetmap.org/?mlat=%.5f&mlon=%.5f#map=17/%.5f/%.5f"
            .format(java.util.Locale.US, la, lo, la, lo)
        context.getString(org.ducatproject.ducat.R.string.location_where_i_am, url)
    })
}
