package android.text.format

/**
 * The phone's own 12/24-hour setting.
 *
 * On Android this reads Settings.System.TIME_12_24, which is why the app asks
 * it rather than pattern-matching `HH` — the answer belongs to the person
 * holding the phone, not to their country. A desk has no such setting, so it
 * takes the JVM's locale default, which is the nearest true thing available.
 */
object DateFormat {
    @JvmStatic
    fun getTimeFormat(context: android.content.Context?): java.text.DateFormat =
        java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT)
}
