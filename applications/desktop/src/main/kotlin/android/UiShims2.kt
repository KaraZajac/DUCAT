// A toast, and the back gesture, desk editions.

package android.widget

/**
 * The phone's brief message at the bottom of the screen. A desktop has no
 * such affordance by convention, so the desk collects them: the window
 * shows the most recent one for a few seconds (see Main.kt), which keeps
 * "Copied", "Sent" and the small confirmations from vanishing into a log.
 */
class Toast private constructor(private val text: String) {
    fun show() {
        latest = text to System.currentTimeMillis()
        org.ducatproject.ducat.DucatLog.i("Desk", text)
    }

    companion object {
        @JvmField val LENGTH_SHORT: Int = 0
        @JvmField val LENGTH_LONG: Int = 1

        /** The most recent message and when it was posted. */
        @Volatile
        var latest: Pair<String, Long>? = null

        @JvmStatic
        fun makeText(context: android.content.Context?, text: CharSequence?, duration: Int): Toast =
            Toast(text?.toString().orEmpty())

        @JvmStatic
        fun makeText(context: android.content.Context?, resId: Int, duration: Int): Toast =
            Toast(android.res.DeskRes.string(resId))
    }
}
