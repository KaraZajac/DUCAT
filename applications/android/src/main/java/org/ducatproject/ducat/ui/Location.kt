package org.ducatproject.ducat.ui

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
    // A recent fix answers *now*. Everything this feeds is ~1.2 km coarse
    // (§15.12 cells) or a route preview, and a ten-minute-old position beats
    // thirty silent seconds of GPS cold-start every time a person is
    // standing indoors wondering if the button worked.
    val recent = listOf(
        android.location.LocationManager.GPS_PROVIDER,
        android.location.LocationManager.NETWORK_PROVIDER,
    ).mapNotNull { p ->
        runCatching { lm.getLastKnownLocation(p) }.getOrNull()
    }.maxByOrNull { it.time }
    if (recent != null && System.currentTimeMillis() - recent.time < 10 * 60 * 1000) {
        return send(recent)
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
                // This callback runs after the try below has exited — an
                // unguarded call here crashes if permission was revoked
                // between the request and the fix arriving.
                send(loc
                    ?: runCatching {
                        lm.getLastKnownLocation(android.location.LocationManager.GPS_PROVIDER)
                    }.getOrNull()
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
