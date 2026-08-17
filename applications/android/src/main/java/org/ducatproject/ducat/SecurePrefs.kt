package org.ducatproject.ducat

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

/**
 * The store, encrypted at rest.
 *
 * A DUCAT phone's `ducat_contacts` prefs hold the spend key, the persona
 * secret, every contact, and the whole message and receipt history; the
 * ceremony prefs hold escrow key shares. In plaintext SharedPreferences a
 * lost or seized phone is a full transcript and a live wallet. This wraps
 * those files in AES-GCM with a key that lives in the Android Keystore and
 * never leaves it — so the bytes on disk are ciphertext, and the key is not
 * on disk at all.
 *
 * Losing the Keystore key (uninstall, factory reset) makes the ciphertext
 * unrecoverable — which is the same guarantee, stated from the other side,
 * that §4.3's forced backup exists to answer: the money comes back from the
 * passphrase-protected export, not from anything on the device. Nobody can
 * recover it *for* you is the feature, not a regression.
 *
 * A single cached instance per file: EncryptedSharedPreferences is not meant
 * to be recreated per call, and the migration must run exactly once.
 *
 * NOTE: only the sensitive files go through here. Settings — locale, units,
 * theme, the ride draft, the map cache — stay plain: no secret is in them and
 * every extra migrating file is extra risk for no privacy gained.
 */
object SecurePrefs {
    /** Encrypted files get a distinct name: the library manages the file, and
     *  pointing it at an existing plaintext file would read ciphertext from
     *  cleartext bytes. The plaintext original is migrated in, then deleted. */
    private const val SUFFIX = "_enc"

    private val cache = HashMap<String, SharedPreferences>()

    @Synchronized
    fun get(context: Context, name: String): SharedPreferences {
        cache[name]?.let { return it }
        val app = context.applicationContext
        val masterKey = MasterKey.Builder(app)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val enc = EncryptedSharedPreferences.create(
            app,
            name + SUFFIX,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
        migrateOnce(app, name, enc)
        cache[name] = enc
        return enc
    }

    /**
     * Copy a pre-encryption plaintext file into the encrypted store, once,
     * then delete the plaintext.
     *
     * Idempotent and crash-safe by ordering: the data is committed to the
     * encrypted store (and its `_migrated` flag) *before* the plaintext is
     * deleted, so a crash between the two just re-copies harmlessly on the
     * next launch. A fresh install has an empty plaintext file and simply
     * records the flag.
     */
    private fun migrateOnce(context: Context, name: String, enc: SharedPreferences) {
        if (enc.getBoolean(MIGRATED, false)) return
        val plain = context.getSharedPreferences(name, Context.MODE_PRIVATE)
        val all = plain.all
        val e = enc.edit()
        for ((k, v) in all) {
            when (v) {
                is String -> e.putString(k, v)
                is Boolean -> e.putBoolean(k, v)
                is Int -> e.putInt(k, v)
                is Long -> e.putLong(k, v)
                // `kotlin.Float` spelled out: this package has its own `Float`
                // (§17.2's spendable balance), and the bare name resolves to it.
                is kotlin.Float -> e.putFloat(k, v)
                is Set<*> -> e.putStringSet(k, v.filterIsInstance<String>().toSet())
            }
        }
        e.putBoolean(MIGRATED, true)
        // commit(), not apply(): the plaintext delete below must not race the
        // write, or a crash could leave the secrets only in the deleted file.
        val ok = e.commit()
        if (ok && all.isNotEmpty()) {
            DucatLog.i(TAG, "migrated ${all.size} key(s) of $name to the encrypted store")
        }
        if (ok) context.deleteSharedPreferences(name)
    }

    private const val MIGRATED = "_secureprefs_migrated"
    private const val TAG = "SecurePrefs"
}

/**
 * The one call the stores make. On the phone it returns the encrypted,
 * migrated store; the desktop build provides its own plaintext version of
 * this same function (a laptop's threat model and storage are different, and
 * it has no Android Keystore).
 */
fun securePrefs(context: Context, name: String): SharedPreferences =
    SecurePrefs.get(context, name)
