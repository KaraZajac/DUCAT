// Sharing, desk edition.
//
// The phone hands text to Android's share sheet — every app that accepts
// text is a destination. A desktop has no such registry, so the desk keeps
// the promise the button makes ("get this out of the app") the way a desktop
// keeps it: the text goes to the clipboard, and the screen says so.

package android.content

class Intent(val action: String? = null) {
    var type: String? = null
    private val extras = mutableMapOf<String, String>()

    fun putExtra(name: String, value: String): Intent = apply { extras[name] = value }
    fun getStringExtra(name: String): String? = extras[name]
    fun setType(t: String): Intent = apply { type = t }
    val dataString: String? get() = null

    companion object {
        @JvmField val ACTION_SEND: String = "android.intent.action.SEND"
        @JvmField val ACTION_VIEW: String = "android.intent.action.VIEW"
        @JvmField val EXTRA_TEXT: String = "android.intent.extra.TEXT"
        @JvmField val EXTRA_SUBJECT: String = "android.intent.extra.SUBJECT"
        @JvmField val EXTRA_STREAM: String = "android.intent.extra.STREAM"

        @JvmStatic
        fun createChooser(target: Intent, title: CharSequence?): Intent = target
    }
}
