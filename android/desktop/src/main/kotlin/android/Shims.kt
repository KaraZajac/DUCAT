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
    }

    abstract val filesDir: File
    abstract fun getSharedPreferences(name: String, mode: Int): SharedPreferences
    open val packageName: String get() = "org.ducatproject.desk"
    open val packageManager: android.content.pm.PackageManager
        get() = android.content.pm.PackageManager()
}

interface SharedPreferences {
    fun getString(key: String, def: String?): String?
    fun getInt(key: String, def: Int): Int
    fun getLong(key: String, def: Long): Long
    fun getBoolean(key: String, def: Boolean): Boolean
    fun getFloat(key: String, def: Float): Float
    fun contains(key: String): Boolean
    val all: Map<String, *>
    fun edit(): Editor

    interface Editor {
        fun putString(key: String, v: String?): Editor
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
