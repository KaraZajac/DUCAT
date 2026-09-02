// Sharing, desk edition.
//
// The phone hands text to Android's share sheet — every app that accepts
// text is a destination. A desktop has no such registry, so the desk keeps
// the promise the button makes ("get this out of the app") the way a desktop
// keeps it: the text goes to the clipboard, and the screen says so.

package android.content

class Intent(val action: String? = null, val data: android.net.Uri? = null) {
    var type: String? = null
    private val extras = mutableMapOf<String, Any>()
    private var stream: android.net.Uri? = null

    fun putExtra(name: String, value: String): Intent = apply { extras[name] = value }
    fun putExtra(name: String, value: android.net.Uri): Intent = apply {
        extras[name] = value
        if (name == EXTRA_STREAM) stream = value
    }

    /**
     * The clip a share intent carries.
     *
     * Android reads the URI grant from the extra *or* this, and builds the
     * share sheet's preview from this alone — so the phone sets both, and
     * the shared code that does it has to compile here too. The desk hands
     * a file to a window rather than to a chooser, so nothing reads it back;
     * it exists so the two clients can keep one implementation.
     */
    var clipData: ClipData? = null

    /** The file a share intent carries, if it carries one. */
    val streamUri: android.net.Uri? get() = stream ?: data

    fun setDataAndType(uri: android.net.Uri?, mime: String?): Intent = apply {
        type = mime
        stream = uri
    }

    fun addFlags(flags: Int): Intent = this
    fun getStringExtra(name: String): String? = extras[name] as? String
    fun setType(t: String): Intent = apply { type = t }
    val dataString: String? get() = null

    companion object {
        @JvmField val ACTION_SEND: String = "android.intent.action.SEND"
        @JvmField val ACTION_VIEW: String = "android.intent.action.VIEW"
        @JvmField val EXTRA_TEXT: String = "android.intent.extra.TEXT"
        @JvmField val EXTRA_SUBJECT: String = "android.intent.extra.SUBJECT"
        @JvmField val EXTRA_STREAM: String = "android.intent.extra.STREAM"
        @JvmField val FLAG_GRANT_READ_URI_PERMISSION: Int = 1
        @JvmField val FLAG_ACTIVITY_NEW_TASK: Int = 268435456

        @JvmStatic
        fun createChooser(target: Intent, title: CharSequence?): Intent = target
    }
}
