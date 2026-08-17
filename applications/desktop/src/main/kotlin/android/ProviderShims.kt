// Handing a file to another program.
//
// Android needs a FileProvider because an app cannot pass a raw path across
// a process boundary. A desktop has no such boundary: a path *is* the
// handle, so this returns one and the share path (Context.startActivity)
// does the desk-appropriate thing with it.

package androidx.core.content

object FileProvider {
    @JvmStatic
    fun getUriForFile(
        context: android.content.Context,
        authority: String,
        file: java.io.File,
    ): android.net.Uri = android.net.Uri.fromFile(file)
}
