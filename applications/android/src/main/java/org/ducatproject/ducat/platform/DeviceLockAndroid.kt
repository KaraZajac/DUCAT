package org.ducatproject.ducat.platform

import android.content.Context
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.fragment.app.FragmentActivity
import org.ducatproject.ducat.DeviceLock
import org.ducatproject.ducat.DucatLog

/**
 * [DeviceLock] on a phone: `BiometricPrompt`, asking for whatever the owner
 * already set up.
 *
 * Deliberately **not** in the shared sources — this is the half that knows
 * about fragments and activities, and the desk compiles the other half.
 *
 * `BIOMETRIC_WEAK or DEVICE_CREDENTIAL` rather than `BIOMETRIC_STRONG`: the
 * question here is "is this the owner", not "release a key from the Keystore",
 * and insisting on a class-3 sensor would refuse a perfectly good device PIN
 * on a phone whose face unlock is class 2. The credential is the fallback the
 * system itself offers, which is what makes this usable by somebody who has
 * enrolled no biometric at all — the case that matters most, because it is the
 * one where the alternative is typing a second PIN.
 */
object DeviceLockAndroid : DeviceLock.Backend {

    private const val TAG = "DucatDeviceLock"

    private const val ALLOWED =
        BiometricManager.Authenticators.BIOMETRIC_WEAK or
            BiometricManager.Authenticators.DEVICE_CREDENTIAL

    override fun enrolled(context: Context): Boolean =
        BiometricManager.from(context).canAuthenticate(ALLOWED) ==
            BiometricManager.BIOMETRIC_SUCCESS

    /**
     * The one question Android answers about *when* the owner last proved
     * themselves: a Keystore key bound to user authentication for a window.
     * Initialising a cipher with it succeeds while the window holds and
     * throws `UserNotAuthenticatedException` once it lapses, so the key is
     * the clock and this process keeps no timestamp of its own to be wrong
     * about.
     *
     * The window is fixed when the key is made, so a changed setting remakes
     * it. Null when there is no secure lock screen to bind to — the caller
     * then asks for the app's own secret, which is the honest answer for a
     * phone with nothing guarding it.
     */
    override fun authenticatedWithin(context: Context, withinSecs: Int): Boolean? {
        val alias = "ducat_spend_window_$withinSecs"
        return runCatching {
            val store = java.security.KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            if (!store.containsAlias(alias)) {
                val spec = android.security.keystore.KeyGenParameterSpec.Builder(
                    alias,
                    android.security.keystore.KeyProperties.PURPOSE_ENCRYPT,
                )
                    .setBlockModes(android.security.keystore.KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(android.security.keystore.KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setUserAuthenticationRequired(true)
                    .also { b ->
                        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
                            b.setUserAuthenticationParameters(
                                withinSecs,
                                android.security.keystore.KeyProperties.AUTH_DEVICE_CREDENTIAL or
                                    android.security.keystore.KeyProperties.AUTH_BIOMETRIC_STRONG,
                            )
                        } else {
                            @Suppress("DEPRECATION")
                            b.setUserAuthenticationValidityDurationSeconds(withinSecs)
                        }
                    }
                    .build()
                javax.crypto.KeyGenerator.getInstance(
                    android.security.keystore.KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore",
                ).apply { init(spec) }.generateKey()
            }
            val key = store.getKey(alias, null) as javax.crypto.SecretKey
            javax.crypto.Cipher.getInstance("AES/GCM/NoPadding").init(javax.crypto.Cipher.ENCRYPT_MODE, key)
            true
        }.getOrElse { e ->
            when (e) {
                // The window lapsed. A real answer, not a failure.
                is android.security.keystore.UserNotAuthenticatedException -> false
                // The lock screen went away after the key was made; the key
                // is gone with it. Same answer as never having had one.
                is android.security.keystore.KeyPermanentlyInvalidatedException -> {
                    runCatching {
                        java.security.KeyStore.getInstance("AndroidKeyStore")
                            .apply { load(null) }.deleteEntry(alias)
                    }
                    null
                }
                else -> {
                    DucatLog.i(TAG, "no authentication window available: ${e.message}")
                    null
                }
            }
        }
    }

    override fun prompt(
        context: Context,
        title: String,
        subtitle: String,
        onResult: (Boolean) -> Unit,
    ) {
        // The prompt needs the Activity, not whatever Context a composable
        // happened to be holding — and a screen wrapped for its locale hands
        // out a ContextWrapper, so unwrap rather than cast.
        val activity = generateSequence(context) {
            (it as? android.content.ContextWrapper)?.baseContext
        }.filterIsInstance<FragmentActivity>().firstOrNull()
        if (activity == null) {
            DucatLog.w(TAG, "no activity to host the prompt")
            return onResult(false)
        }
        val prompt = BiometricPrompt(
            activity,
            androidx.core.content.ContextCompat.getMainExecutor(activity),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(
                    r: BiometricPrompt.AuthenticationResult,
                ) = onResult(true)

                // Everything else is a no. A cancel, a lockout and an error
                // are indistinguishable from here and must be: the only
                // outcome that opens the gate is a success.
                override fun onAuthenticationError(code: Int, msg: CharSequence) {
                    DucatLog.i(TAG, "device unlock declined ($code)")
                    onResult(false)
                }

                // Not a failure of the attempt — one bad finger, more coming.
                // The system keeps its own prompt up; say nothing.
                override fun onAuthenticationFailed() {}
            },
        )
        runCatching {
            prompt.authenticate(
                BiometricPrompt.PromptInfo.Builder()
                    .setTitle(title)
                    .setSubtitle(subtitle)
                    .setAllowedAuthenticators(ALLOWED)
                    .build(),
            )
        }.onFailure {
            DucatLog.w(TAG, "prompt: ${it.message}")
            onResult(false)
        }
    }
}
