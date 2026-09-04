// Picking a file, desk edition.
//
// A phone screen asks the system for content and gets a Uri back; a desktop
// opens a file dialog and gets a path. Same gesture, same callback, so the
// screens that attach a picture, choose an avatar or restore a backup work
// here without a desk-specific edit. What a desk genuinely lacks — a camera
// — says so instead of pretending: TakePicture never reports a photo that
// was not taken.

package androidx.activity.result.contract

import android.net.Uri

sealed class ActivityResultContract<I, O> {
    /** Runs on the caller's thread; a file dialog is modal either way. */
    abstract fun run(input: I): O
}

object ActivityResultContracts {
    /** "Give me a file of this MIME type." */
    class GetContent : ActivityResultContract<String, Uri?>() {
        override fun run(input: String): Uri? = pickFile(input)
    }

    /** "Give me several." The desk's dialog picks one at a time, so this
     *  returns a list of nought or one — enough for shared code to compile
     *  and behave, and honest about what this shim can actually do. */
    class GetMultipleContents : ActivityResultContract<String, List<Uri>>() {
        override fun run(input: String): List<Uri> = listOfNotNull(pickFile(input))
    }

    class OpenDocument : ActivityResultContract<Array<String>, Uri?>() {
        override fun run(input: Array<String>): Uri? = pickFile(input.firstOrNull() ?: "*/*")
    }

    class CreateDocument(private val mime: String = "*/*") :
        ActivityResultContract<String, Uri?>() {
        override fun run(input: String): Uri? = saveFile(input)
    }

    /** No camera here. The callback is told the picture was not taken. */
    class TakePicture : ActivityResultContract<Uri, Boolean>() {
        override fun run(input: Uri): Boolean {
            android.widget.Toast
                .makeText(null, "This desk has no camera", android.widget.Toast.LENGTH_SHORT)
                .show()
            return false
        }
    }

    /** Permissions are an Android concept; a desk already has its answer. */
    class RequestPermission : ActivityResultContract<String, Boolean>() {
        override fun run(input: String): Boolean = true
    }

    private fun pickFile(mime: String): Uri? {
        val d = java.awt.FileDialog(null as java.awt.Frame?, "Choose a file", java.awt.FileDialog.LOAD)
        if (mime.startsWith("image/")) {
            d.setFilenameFilter { _, name ->
                name.lowercase().matches(Regex(".*\\.(png|jpg|jpeg|gif|webp|bmp)$"))
            }
        }
        d.isVisible = true
        val dir = d.directory ?: return null
        val file = d.file ?: return null
        return Uri.fromFile(java.io.File(dir, file))
    }

    private fun saveFile(suggested: String): Uri? {
        val d = java.awt.FileDialog(null as java.awt.Frame?, "Save as", java.awt.FileDialog.SAVE)
        d.file = suggested
        d.isVisible = true
        val dir = d.directory ?: return null
        val file = d.file ?: return null
        return Uri.fromFile(java.io.File(dir, file))
    }
}
