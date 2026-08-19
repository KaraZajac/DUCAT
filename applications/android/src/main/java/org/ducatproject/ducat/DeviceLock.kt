package org.ducatproject.ducat

import android.content.Context

/**
 * The lock the phone already has, offered instead of typing the PIN again.
 *
 * Somebody who unlocks their phone forty times a day should not have to learn
 * a second secret to buy a coffee. So where the device has a credential
 * enrolled — a fingerprint, a face, or its own PIN or pattern — the gate can
 * ask *it*, and the answer is as good: proving you are the person holding an
 * unlocked phone is exactly what [Pin] exists to establish.
 *
 * **This does not replace the PIN, and must not.** The device credential is an
 * offer, never the only door:
 *
 *  - not every device has one enrolled, and enrolment can be removed later,
 *    which would otherwise strand somebody outside their own wallet;
 *  - the desk has no such thing at all, and compiles these same sources;
 *  - and a PIN this app owns is the one that survives a phone being handed to
 *    somebody with the screen already unlocked, which is the case §15.5 is
 *    actually about.
 *
 * So the PIN stays set, stays required at onboarding, and stays visible behind
 * every prompt this raises.
 *
 * **Why a hook rather than a call.** The implementation is `BiometricPrompt`,
 * which is Android and fragments and an Activity; none of that exists on the
 * desk, which compiles this file. The app installs [backend] at startup and
 * everything above the hook is platform-free — the same arrangement
 * `Notify.sink` uses for the tray.
 */
object DeviceLock {

    /** What the platform can actually do. Null wherever there is no platform. */
    interface Backend {
        /** True when this device has something to ask. */
        fun enrolled(context: Context): Boolean

        /**
         * Raise the system's own prompt. [onResult] is called with true only
         * on a real success — a cancel, a lockout and an error are all false,
         * and all mean "fall back to the PIN", never "let them through".
         */
        fun prompt(
            context: Context,
            title: String,
            subtitle: String,
            onResult: (Boolean) -> Unit,
        )
    }

    @Volatile
    var backend: Backend? = null

    /** Is there a device credential to offer at all, right now? */
    fun available(context: Context): Boolean =
        runCatching { backend?.enrolled(context) == true }.getOrDefault(false)

    private fun prefs(context: Context) = securePrefs(context, "ducat_pin")

    /**
     * Whether to raise the system prompt without being asked.
     *
     * Set the first time somebody uses it and never presented as a setting.
     * A preference screen is a thing to go and find; using the button once is
     * the same statement and costs nobody a trip through settings. Cleared if
     * the enrolment goes away, so a removed fingerprint does not leave the gate
     * raising a prompt that can no longer succeed.
     */
    fun preferred(context: Context): Boolean =
        available(context) && prefs(context).getBoolean("prefer_device_lock", false)

    fun remember(context: Context, used: Boolean) =
        prefs(context).edit().putBoolean("prefer_device_lock", used).apply()
}
