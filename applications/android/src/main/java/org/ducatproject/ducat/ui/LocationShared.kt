package org.ducatproject.ducat.ui

// Turning a fix into something a person can send is the same everywhere;
// *getting* the fix is not. The phone reads its GPS (Location.kt); a desk
// does not move, so it is told where it is once (the desk's LocationDesk.kt).

/** Whether this device may be asked where it is. */
fun locationAllowed(context: android.content.Context): Boolean =
    context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) ==
        android.content.pm.PackageManager.PERMISSION_GRANTED

/**
 * Ask for location again, by whichever route is still open.
 *
 * Android shows its dialog once, allows one more ask, and then stops showing
 * anything at all — a second refusal is permanent, and `launch` after that is
 * a button that visibly does nothing. `shouldShowRequestPermissionRationale`
 * is false both before the first ask and after the last one, so `asked` is
 * what tells those two apart; past the end of the road, the only way back is
 * the app's own page in Settings.
 */
fun askForLocation(
    context: android.content.Context,
    asked: Boolean,
    launch: (String) -> Unit,
) {
    val perm = android.Manifest.permission.ACCESS_FINE_LOCATION
    // Compose hands screens whatever context wraps the activity, which is
    // often not the activity itself; an unwrapped cast quietly comes back
    // null and this button would never find its way to Settings.
    var c: android.content.Context? = context
    while (c != null && c !is android.app.Activity) {
        c = (c as? android.content.ContextWrapper)?.baseContext
    }
    val activity = c as? android.app.Activity
    if (asked && activity?.shouldShowRequestPermissionRationale(perm) == false) {
        context.startActivity(
            android.content.Intent(
                android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                android.net.Uri.fromParts("package", context.packageName, null),
            ),
        )
    } else {
        launch(perm)
    }
}

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
