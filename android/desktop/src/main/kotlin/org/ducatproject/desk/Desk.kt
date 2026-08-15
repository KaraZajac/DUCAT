package org.ducatproject.desk

import android.content.Context
import android.content.FilePreferences
import android.content.SharedPreferences
import java.io.File

/**
 * The desk's Context: the same stores the phone trusts, on files.
 *
 * One JSON file per preferences name under `$XDG_DATA_HOME/ducat-desk/prefs`,
 * which makes the desk's whole protocol state a directory you can look at —
 * and, per §4.3, one you must treat as the complete spending credential it is.
 */
class DeskContext(private val root: File) : Context() {
    private val prefsDir = File(root, "prefs").apply { mkdirs() }
    private val cache = java.util.concurrent.ConcurrentHashMap<String, SharedPreferences>()

    override val filesDir: File = File(root, "files").apply { mkdirs() }

    override fun getSharedPreferences(name: String, mode: Int): SharedPreferences =
        cache.getOrPut(name) { FilePreferences(File(prefsDir, "$name.json")) }
}
