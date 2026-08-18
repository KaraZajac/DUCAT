package org.ducatproject.ducat

import android.content.Context
import android.content.FilePreferences
import android.content.SharedPreferences
import java.io.File

/**
 * The desk's `securePrefs`, matching the phone's signature so the shared
 * stores compile against both — and now actually keeping a secret.
 *
 * The phone puts its spend key, persona secret and prekeys behind
 * EncryptedSharedPreferences, whose master key lives in the Android Keystore
 * and never touches disk. A desktop has no such box. This file used to answer
 * that by storing them in plaintext JSON and saying so, which was defensible
 * while the desk was a chat client and stopped being defensible when the desk
 * grew a wallet of its own: anything that could read the home directory could
 * read the money.
 *
 * So the key comes from a passphrase instead. Argon2id (§4.3's reviewed
 * parameters, domain-separated from the backup's key by `vault_key`), and
 * XChaCha20-Poly1305 per file through the same bridge the attachments use.
 * What that buys and what it does not:
 *
 *  - **Buys**: a stolen disk, a synced home directory, a backup of the laptop,
 *    a second user on the machine — none of those yield the keys.
 *  - **Does not buy**: protection from something running *as the operator
 *    while the desk is unlocked*. The key is in memory then, as it must be.
 *
 * Unlocking is deliberately not automatic. A desk that unlocks itself is a
 * desk whose key is on the same disk as its data, which is the arrangement
 * this replaces. Headless tools pass `DUCAT_DESK_PASSPHRASE`; the window asks.
 */
object DeskVault {
    private const val VAULT = "vault.json"
    private const val CHECK = "DUCAT-DESK-VAULT-CHECK-v1"

    @Volatile
    private var key: ByteArray? = null

    @Volatile
    private var root: File? = null

    val unlocked: Boolean get() = key != null

    /** True when this desk has a vault at all — i.e. a passphrase was set. */
    fun exists(dir: File): Boolean = File(dir, VAULT).isFile

    /**
     * Set the passphrase for a desk that has none, and encrypt whatever is
     * already on disk.
     *
     * Ordered the way the phone's own migration is: write the encrypted copy,
     * verify it reads back, and only then delete the plaintext. A crash in
     * the middle costs a re-run, never the data.
     */
    fun create(dir: File, passphrase: String): Result<Unit> = runCatching {
        require(passphrase.length >= 8) { "a passphrase under eight characters is not one" }
        val salt = uniffi.ducat_mobile.randomBytes(16u)
        val k = uniffi.ducat_mobile.vaultKey(passphrase, salt)
        val nonce = uniffi.ducat_mobile.randomBytes(24u)
        val check = uniffi.ducat_mobile.attachmentSeal(k, nonce, CHECK.toByteArray())
        File(dir, VAULT).writeText(
            org.json.JSONObject()
                .put("v", 1)
                .put("salt", b64(salt))
                .put("nonce", b64(nonce))
                .put("check", b64(check))
                .toString(),
        )
        key = k
        root = dir
        migratePlaintext(dir, k)
    }

    /** Open an existing vault. Wrong passphrase fails; it does not corrupt. */
    fun unlock(dir: File, passphrase: String): Result<Unit> = runCatching {
        val o = org.json.JSONObject(File(dir, VAULT).readText())
        val k = uniffi.ducat_mobile.vaultKey(passphrase, unb64(o.getString("salt")))
        val plain = uniffi.ducat_mobile.attachmentOpen(
            k, unb64(o.getString("nonce")), unb64(o.getString("check")),
        )
        check(plain.decodeToString() == CHECK) { "that passphrase does not open this desk" }
        key = k
        root = dir
        // A vault made before some store existed still has plaintext beside it.
        migratePlaintext(dir, k)
    }

    fun lock() {
        key?.fill(0)
        key = null
        clearPrefsCache()
    }

    /** Every plaintext store becomes an encrypted one, then stops existing. */
    private fun migratePlaintext(dir: File, k: ByteArray) {
        val prefs = File(dir, "prefs")
        prefs.listFiles { f: File -> f.name.endsWith(".json") }?.forEach { plain ->
            val name = plain.name.removeSuffix(".json")
            val enc = File(prefs, "$name.enc")
            runCatching {
                val body = plain.readText()
                writeEncrypted(enc, k, body)
                // Read it back before letting go of the only other copy.
                check(readEncrypted(enc, k) == body) { "re-read did not match" }
                plain.delete()
                DucatLog.i("DeskVault", "encrypted $name")
            }.onFailure { DucatLog.w("DeskVault", "migrating $name: ${it.message}") }
        }
    }

    internal fun writeEncrypted(file: File, k: ByteArray, body: String) {
        val nonce = uniffi.ducat_mobile.randomBytes(24u)
        val ct = uniffi.ducat_mobile.attachmentSeal(k, nonce, body.toByteArray())
        file.parentFile?.mkdirs()
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeBytes(nonce + ct)
        tmp.renameTo(file)
    }

    internal fun readEncrypted(file: File, k: ByteArray): String {
        val all = file.readBytes()
        require(all.size > 24) { "truncated vault file" }
        return uniffi.ducat_mobile
            .attachmentOpen(k, all.copyOfRange(0, 24), all.copyOfRange(24, all.size))
            .decodeToString()
    }

    internal fun keyOrNull(): ByteArray? = key

    private fun b64(b: ByteArray): String =
        java.util.Base64.getEncoder().encodeToString(b)

    private fun unb64(s: String): ByteArray = java.util.Base64.getDecoder().decode(s)
}

/**
 * Preferences whose file is a sealed blob rather than readable JSON.
 *
 * Deliberately the same shape as [FilePreferences] — one file per name, whole
 * document rewritten on commit — because the stores above it already assume
 * that, and a clever incremental format would be a second place for the chain
 * counters to tear.
 */
class VaultPreferences(private val file: File, private val key: ByteArray) : SharedPreferences {
    // FilePreferences reads and writes plaintext JSON; give it a private
    // scratch path and keep the sealed file as the durable one.
    private val scratch: File = File.createTempFile("ducat-vault", ".json").apply {
        deleteOnExit()
        if (file.isFile) runCatching { writeText(DeskVault.readEncrypted(file, key)) }
    }
    private val inner = FilePreferences(scratch)

    private fun seal() {
        DeskVault.writeEncrypted(file, key, if (scratch.isFile) scratch.readText() else "{}")
    }

    override fun getString(k: String, d: String?) = inner.getString(k, d)
    override fun getStringSet(k: String, d: Set<String>?) = inner.getStringSet(k, d)
    override fun getInt(k: String, d: Int) = inner.getInt(k, d)
    override fun getLong(k: String, d: Long) = inner.getLong(k, d)
    override fun getBoolean(k: String, d: Boolean) = inner.getBoolean(k, d)
    override fun getFloat(k: String, d: kotlin.Float) = inner.getFloat(k, d)
    override fun contains(k: String) = inner.contains(k)
    override val all: Map<String, *> get() = inner.all

    override fun edit(): SharedPreferences.Editor {
        val e = inner.edit()
        return object : SharedPreferences.Editor by e {
            override fun apply() { e.apply(); seal() }
            override fun commit(): Boolean = e.commit().also { seal() }
        }
    }
}

private val cache = java.util.concurrent.ConcurrentHashMap<String, SharedPreferences>()

/**
 * The store the phone's code asks for. Sealed when this desk has a vault and
 * it is open; plaintext only on a desk whose operator never set a passphrase,
 * which the window says out loud rather than implying otherwise.
 */
fun securePrefs(context: Context, name: String): SharedPreferences {
    val root = context.filesDir.parentFile
    val k = DeskVault.keyOrNull()
    if (k == null) {
        // A locked desk must not quietly answer "empty". Every store here
        // reads as absent when it cannot be decrypted, and absent has
        // meanings that cost money: no wallet means *mint a new one*, and a
        // till would then take payments into a wallet nobody can restore
        // while the real one sits sealed beside it. Refuse instead.
        check(root == null || !DeskVault.exists(root)) {
            "this desk is locked; unlock it before reading $name"
        }
        return context.getSharedPreferences(name, 0)
    }
    return cache.getOrPut(name) { VaultPreferences(File(root, "prefs/$name.enc"), k) }
}

internal fun clearPrefsCache() = cache.clear()
