package android.text.format

/**
 * The phone's own 12/24-hour setting.
 *
 * On Android this reads Settings.System.TIME_12_24, which is why the app asks
 * it rather than pattern-matching `HH` — the answer belongs to the person
 * holding the phone, not to their country. A desk has no such setting, so it
 * takes the JVM's locale default, which is the nearest true thing available.
 */
/**
 * Byte counts the way Android words them ("25 MB"), for the Library screen.
 * Android rounds to short, human units; String.format on the JVM does the
 * same arithmetic with the reader's locale separators.
 */
object Formatter {
    @JvmStatic
    fun formatShortFileSize(context: android.content.Context?, bytes: Long): String {
        var v = bytes.toDouble()
        var unit = "B"
        for (u in listOf("kB", "MB", "GB", "TB")) {
            if (v < 999.5) break
            v /= 1000.0
            unit = u
        }
        val shown = if (v < 9.95 && unit != "B") {
            String.format(java.util.Locale.getDefault(), "%.1f", v)
        } else {
            String.format(java.util.Locale.getDefault(), "%.0f", v)
        }
        return "$shown $unit"
    }
}

object DateFormat {
    @JvmStatic
    fun getTimeFormat(context: android.content.Context?): java.text.DateFormat =
        java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT)

    /**
     * Android asks ICU for the locale's arrangement of a field skeleton; the
     * JVM has no skeleton API, so the desk answers from a table of the one
     * skeleton the app uses. The languages that put the month first are the
     * distinction that matters; everyone else reads day-first as today.
     */
    @JvmStatic
    fun getBestDateTimePattern(locale: java.util.Locale, skeleton: String): String =
        when (skeleton) {
            "dMMM" -> when (locale.language) {
                "ja", "zh" -> "M月d日"
                "ko" -> "M월 d일"
                else -> "d MMM"
            }
            else -> skeleton
        }
}
