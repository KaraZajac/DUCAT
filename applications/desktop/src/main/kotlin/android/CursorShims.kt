// Asking a Uri its display name. On Android that is a ContentProvider query;
// here every Uri is a file, so the answer is the file's own name — the same
// answer, arrived at without the ceremony.

package android.provider

object OpenableColumns {
    const val DISPLAY_NAME = "_display_name"
    const val SIZE = "_size"
}

