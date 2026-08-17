package org.ducatproject.ducat

import android.content.Context
import java.util.Locale

/**
 * Distance shown the way this user reads distance.
 *
 * Kilometres for most of the world, miles for the handful of places that still
 * measure roads in them. Like currency, the default is taken from the device
 * rather than assumed — someone who thinks in miles should not have to convert
 * a pickup distance in their head — and like currency it is overridable in
 * Settings, because the device's guess is only a guess.
 */
class UnitsStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_units", Context.MODE_PRIVATE)

    /** "system" (follow the locale), "metric", or "imperial". */
    fun system(): String = prefs.getString("distance", SYSTEM) ?: SYSTEM

    fun setSystem(v: String) = prefs.edit().putString("distance", v).apply()

    companion object {
        const val SYSTEM = "system"
        const val METRIC = "metric"
        const val IMPERIAL = "imperial"
    }
}

object Units {
    private const val METERS_PER_MILE = 1609.344

    /** The countries that measure road distance in miles. */
    private val MILE_COUNTRIES = setOf("US", "GB", "LR", "MM")

    private fun localeUsesMiles(): Boolean =
        Locale.getDefault().country.uppercase() in MILE_COUNTRIES

    /** Whether to render distances in miles for this device and preference. */
    fun useMiles(context: Context): Boolean = when (UnitsStore(context).system()) {
        UnitsStore.IMPERIAL -> true
        UnitsStore.METRIC -> false
        else -> localeUsesMiles()
    }

    /**
     * A distance, in the user's unit, with the number formatted for their
     * locale (so a comma-decimal locale reads "3,5 km"). The unit symbol is a
     * resource string so it, too, can be localised.
     */
    fun distance(context: Context, meters: Double): String {
        val miles = useMiles(context)
        val value = if (miles) meters / METERS_PER_MILE else meters / 1000.0
        val num = String.format(Locale.getDefault(), "%.1f", value)
        val unit = context.getString(
            if (miles) R.string.unit_mi else R.string.unit_km
        )
        return context.getString(R.string.distance_value, num, unit)
    }

    fun distance(context: Context, meters: Long): String = distance(context, meters.toDouble())
}
