// The last few Android names a *screen* touches: the clipboard, a toast, and
// the back gesture. Each is a real platform behaviour on a phone and a
// different real behaviour here, which is exactly why they are shimmed
// rather than stubbed away — copying should copy, and a screen that says
// "copied" should be telling the truth on both clients.

package android.content

class ClipData private constructor(
    val text: String,
    /** A clip can carry a file instead of words — see [Intent.clipData],
     *  which the phone sets so a share sheet can preview what it is about
     *  to hand over. Nothing here reads it; the desk shares to a window. */
    val uri: android.net.Uri? = null,
) {
    companion object {
        @JvmStatic
        fun newPlainText(label: CharSequence?, text: CharSequence?): ClipData =
            ClipData(text?.toString().orEmpty())

        @JvmStatic
        fun newUri(resolver: Any?, label: CharSequence?, uri: android.net.Uri?): ClipData =
            ClipData(label?.toString().orEmpty(), uri)
    }
}

class ClipboardManager {
    fun setPrimaryClip(clip: ClipData) {
        runCatching {
            java.awt.Toolkit.getDefaultToolkit().systemClipboard.setContents(
                java.awt.datatransfer.StringSelection(clip.text), null,
            )
        }
    }
}
