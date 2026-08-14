package org.ducatproject.ducat.ui

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
        "📍 Where I am: https://www.openstreetmap.org/?mlat=%.5f&mlon=%.5f#map=17/%.5f/%.5f"
            .format(la, lo, la, lo)
    })
}

/** One raw fix in 1e-7 degrees — what a geocell (§15.12) is computed from. */
fun grabFix(
    context: android.content.Context,
    done: (Pair<Long, Long>?) -> Unit,
) {
    val lm = context.getSystemService(android.location.LocationManager::class.java)
        ?: return done(null)
    val send = { loc: android.location.Location? ->
        done(loc?.let {
            (it.latitude * 1e7).toLong() to (it.longitude * 1e7).toLong()
        })
    }
    try {
        val provider = when {
            lm.isProviderEnabled(android.location.LocationManager.GPS_PROVIDER) ->
                android.location.LocationManager.GPS_PROVIDER
            lm.isProviderEnabled(android.location.LocationManager.NETWORK_PROVIDER) ->
                android.location.LocationManager.NETWORK_PROVIDER
            else -> return done(null)
        }
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            lm.getCurrentLocation(provider, null, context.mainExecutor) { loc ->
                // A fresh fix indoors can take a while or fail outright; the
                // last known position is minutes old at worst and beats an
                // empty pickup field every time.
                send(loc
                    ?: lm.getLastKnownLocation(android.location.LocationManager.GPS_PROVIDER)
                    ?: runCatching {
                        lm.getLastKnownLocation(android.location.LocationManager.NETWORK_PROVIDER)
                    }.getOrNull())
            }
        } else {
            send(lm.getLastKnownLocation(provider))
        }
    } catch (e: SecurityException) {
        done(null)
    }
}
