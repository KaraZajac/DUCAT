// The desk's Android, all of it.
//
// The phone's protocol brain — Mailbox, ContactStore, the stores — touches
// exactly four Android classes: Context, SharedPreferences, Base64, Log
// (plus Build and PackageManager in the log's banner line). Re-creating
// those names lets the desk compile the *same source files* the phone
// ships, which is the whole point: one implementation of the patience
// windows, the prekey partitions and the chain rules, not two drifting
// copies. This is a shim, not a port; the day this grows past a few
// screens of code is the day the logic moves to a real shared module
// instead.

package android.content

import org.json.JSONObject
import java.io.File

abstract class Context {
    companion object {
        @JvmField val MODE_PRIVATE: Int = 0
        @JvmField val CLIPBOARD_SERVICE: String = "clipboard"
    }

    abstract val filesDir: File
    open val cacheDir: File get() = File(filesDir.parentFile, "cache").apply { mkdirs() }
    abstract fun getSharedPreferences(name: String, mode: Int): SharedPreferences
    open val packageName: String get() = "org.ducatproject.desk"
    open val packageManager: android.content.pm.PackageManager
        get() = android.content.pm.PackageManager()

    // Screens read their words through the Context too, not only through
    // Compose's stringResource — 257 call sites' worth. See Resources.kt.
    open fun getString(id: Int): String = android.res.DeskRes.string(id)
    open fun getString(id: Int, vararg args: Any?): String =
        android.res.DeskRes.string(id, *args)
    open val resources: Resources get() = Resources
    open val contentResolver: ContentResolver get() = ContentResolver()

    /**
     * A phone would hand this to the share sheet. The desk's honest
     * equivalent is the clipboard — the text leaves the app, which is what
     * the button promised — and Toast says which.
     */
    /**
     * Runtime permissions are Android's model; a desk's answer is its own
     * settings, and the file dialog *is* the consent. Granting here is not a
     * pretence — nothing is bypassed, because nothing gates it.
     */
    open fun checkSelfPermission(permission: String): Int = android.content.pm.PackageManager.PERMISSION_GRANTED

    open fun startActivity(intent: Intent) {
        // A file: hand it to the desktop, which knows what opens it.
        intent.streamUri?.toFile()?.let { f ->
            runCatching {
                if (java.awt.Desktop.isDesktopSupported()) {
                    java.awt.Desktop.getDesktop().open(f)
                    return
                }
            }
            ClipboardManager().setPrimaryClip(ClipData.newPlainText(null, f.absolutePath))
            android.widget.Toast
                .makeText(this, "Saved — path copied: ${f.name}", android.widget.Toast.LENGTH_LONG)
                .show()
            return
        }
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        if (text != null) {
            ClipboardManager().setPrimaryClip(ClipData.newPlainText(null, text))
            android.widget.Toast
                .makeText(this, "Copied to the clipboard", android.widget.Toast.LENGTH_SHORT)
                .show()
        }
    }

    /** Only the services a screen asks for by name. */
    open fun getSystemService(name: String): Any? = when (name) {
        CLIPBOARD_SERVICE -> ClipboardManager()
        else -> null
    }

    /** The typed form: `getSystemService(ClipboardManager::class.java)`. */
    @Suppress("UNCHECKED_CAST")
    open fun <T> getSystemService(type: Class<T>): T? = when (type) {
        ClipboardManager::class.java -> ClipboardManager() as T
        else -> null
    }
}

/** Only the corner of android.content.res.Resources the screens touch. */
object Resources {
    fun getString(id: Int): String = android.res.DeskRes.string(id)
    fun getQuantityString(id: Int, count: Int): String =
        android.res.DeskRes.plural(id, count)
    fun getQuantityString(id: Int, count: Int, vararg args: Any?): String =
        android.res.DeskRes.plural(id, count, *args)
}

interface SharedPreferences {
    fun getString(key: String, def: String?): String?
    fun getStringSet(key: String, def: Set<String>?): Set<String>?
    fun getInt(key: String, def: Int): Int
    fun getLong(key: String, def: Long): Long
    fun getBoolean(key: String, def: Boolean): Boolean
    fun getFloat(key: String, def: Float): Float
    fun contains(key: String): Boolean
    val all: Map<String, *>
    fun edit(): Editor

    interface Editor {
        fun putString(key: String, v: String?): Editor
        fun putStringSet(key: String, v: Set<String>?): Editor
        fun putInt(key: String, v: Int): Editor
        fun putLong(key: String, v: Long): Editor
        fun putBoolean(key: String, v: Boolean): Editor
        fun putFloat(key: String, v: Float): Editor
        fun remove(key: String): Editor
        fun clear(): Editor
        fun apply()
        fun commit(): Boolean
    }
}

/**
 * SharedPreferences over one JSON file per name. Same durability contract
 * the stores already assume: apply() lands before the process exits in any
 * orderly path, and the file is the truth on the next launch.
 */
class FilePreferences(private val file: File) : SharedPreferences {
    private val lock = Any()
    private val map: MutableMap<String, Any> = run {
        if (!file.exists()) mutableMapOf()
        else {
            val o = JSONObject(file.readText())
            o.keys().asSequence().associateWith { o.get(it) }.toMutableMap()
        }
    }

    private fun save() {
        val o = JSONObject()
        map.forEach { (k, v) -> o.put(k, v) }
        file.parentFile?.mkdirs()
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeText(o.toString())
        tmp.renameTo(file)
    }

    override fun getString(key: String, def: String?) =
        synchronized(lock) { map[key] as? String ?: def }
    override fun getStringSet(key: String, def: Set<String>?): Set<String>? =
        synchronized(lock) {
            // Stored as a JSONArray (see putStringSet); read it back as a Set.
            (map[key] as? org.json.JSONArray)?.let { a ->
                (0 until a.length()).map { a.getString(it) }.toSet()
            } ?: def
        }
    override fun getInt(key: String, def: Int) =
        synchronized(lock) { (map[key] as? Number)?.toInt() ?: def }
    override fun getLong(key: String, def: Long) =
        synchronized(lock) { (map[key] as? Number)?.toLong() ?: def }
    override fun getBoolean(key: String, def: Boolean) =
        synchronized(lock) { map[key] as? Boolean ?: def }
    override fun getFloat(key: String, def: Float) =
        synchronized(lock) { (map[key] as? Number)?.toFloat() ?: def }
    override fun contains(key: String) = synchronized(lock) { map.containsKey(key) }
    override val all: Map<String, *> get() = synchronized(lock) { map.toMap() }

    override fun edit(): SharedPreferences.Editor = object : SharedPreferences.Editor {
        private val puts = mutableMapOf<String, Any>()
        private val removes = mutableSetOf<String>()
        private var wipe = false

        override fun putString(key: String, v: String?) =
            apply2 { if (v == null) removes += key else puts[key] = v }
        override fun putStringSet(key: String, v: Set<String>?) =
            // A JSONArray so save()'s o.put serializes it correctly and
            // getStringSet reads it back; org.json would not wrap a raw Set.
            apply2 { if (v == null) removes += key else puts[key] = org.json.JSONArray(v.toList()) }
        override fun putInt(key: String, v: Int) = apply2 { puts[key] = v }
        override fun putLong(key: String, v: Long) = apply2 { puts[key] = v }
        override fun putBoolean(key: String, v: Boolean) = apply2 { puts[key] = v }
        override fun putFloat(key: String, v: Float) = apply2 { puts[key] = v.toDouble() }
        override fun remove(key: String) = apply2 { removes += key }
        override fun clear() = apply2 { wipe = true }

        private inline fun apply2(f: () -> Unit): SharedPreferences.Editor {
            f(); return this
        }

        override fun apply() { commit() }
        override fun commit(): Boolean {
            synchronized(lock) {
                if (wipe) map.clear()
                removes.forEach { map.remove(it) }
                map.putAll(puts)
                save()
            }
            return true
        }
    }
}

/**
 * Reading what a picker returned. On a phone the Uri is an opaque handle a
 * ContentProvider resolves; here every Uri is a file, so this is the file.
 */
class ContentResolver {
    fun openInputStream(uri: android.net.Uri): java.io.InputStream? =
        uri.toFile()?.takeIf { it.isFile }?.inputStream()

    fun openOutputStream(uri: android.net.Uri): java.io.OutputStream? =
        uri.toFile()?.outputStream()

    fun query(
        uri: android.net.Uri,
        projection: Array<String>?,
        selection: String?,
        args: Array<String>?,
        sort: String?,
    ): Cursor? = uri.toFile()?.takeIf { it.isFile }?.let { Cursor(it) }

    fun getType(uri: android.net.Uri): String? =
        uri.toFile()?.let { runCatching { java.nio.file.Files.probeContentType(it.toPath()) }.getOrNull() }
}

/** The one column a screen ever reads off a picked file. */
class Cursor internal constructor(private val file: java.io.File) {
    fun getColumnIndex(name: String): Int =
        if (name == android.provider.OpenableColumns.DISPLAY_NAME) 0 else -1
    fun moveToFirst(): Boolean = true
    fun getString(index: Int): String = file.name
    fun close() {}
    inline fun <R> use(block: (Cursor) -> R): R = block(this).also { close() }
}
