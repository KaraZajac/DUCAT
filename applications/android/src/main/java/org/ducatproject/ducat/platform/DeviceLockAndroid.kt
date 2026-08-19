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
