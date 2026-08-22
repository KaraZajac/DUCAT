package org.ducatproject.ducat

import android.content.Context

/**
 * The PIN that stands between somebody holding this phone and spending from
 * it (§15.5's confirm rule, with teeth).
 *
 * **What this is and is not.** The keys are already encrypted at rest — that
 * is [securePrefs] and the platform keystore's job, and it is the thing that
 * protects the wallet from a stolen phone's *storage*. This protects it from
 * a stolen phone that is unlocked and in somebody's hand: the moment between
 * picking it up and money leaving. So it gates actions, not bytes. Saying
 * otherwise would be the kind of security claim that gets people robbed.
 *
 * The PIN itself is never stored. What is stored is a PBKDF2 verifier over a
 * random salt, and a count of how many times somebody has guessed wrong —
 * because four digits is a small space and the only thing that makes it a
 * lock rather than a speed bump is that guessing has to be slow.
 */
object Pin {
    private const val TAG = "DucatPin"
    private const val ITERATIONS = 200_000
    private const val LENGTH_BITS = 256

    /** Wrong guesses allowed before the waiting starts. */
    private const val FREE_TRIES = 4

    /** First cooldown, doubling per failure after that. */
    private const val FIRST_COOLDOWN_MS = 30_000L
    private const val MAX_COOLDOWN_MS = 15L * 60 * 1000

    /** Short enough to type at a counter, long enough to be worth typing. */
    const val MIN_DIGITS = 4
    const val MAX_DIGITS = 12

    private fun prefs(context: Context) = securePrefs(context, "ducat_pin")

    fun isSet(context: Context): Boolean =
        !prefs(context).getString("verifier", null).isNullOrBlank()

    /**
     * Set or replace the PIN. Callers that are *replacing* one must have
     * checked the old one first — this cannot tell the difference between an
     * owner and somebody who picked the phone up.
     */
    fun set(context: Context, pin: String) {
        val salt = ByteArray(16).also { java.security.SecureRandom().nextBytes(it) }
        prefs(context).edit()
            .putString("salt", salt.hex())
            .putString("verifier", derive(pin, salt).hex())
            .putInt("failures", 0)
            .putLong("locked_until", 0L)
            .apply()
        DucatLog.i(TAG, "a PIN is set on this device")
    }

    /** How the last guess went. */
    sealed interface Verdict {
        data object Ok : Verdict

        /** Wrong, and this many tries before the waiting starts. */
        data class Wrong(val triesLeft: Int) : Verdict

        /** Too many wrong guesses; nothing is accepted until this passes. */
        data class Locked(val secondsLeft: Long) : Verdict

        /** No PIN has been set on this device yet. */
        data object Unset : Verdict
    }

    /**
     * Seconds still to wait, or zero.
     *
     * Measured on two clocks, and the longer answer wins.
     *
     * `currentTimeMillis` is the wall clock, which is a setting. Somebody
     * holding this phone can open the date picker, push it a year forward, and
     * every cooldown this object ever wrote is in the past — which turns a
     * fifteen-minute wait per guess back into as fast as PBKDF2 will run, and
     * a four-digit PIN falls in well under an hour.
     *
     * `elapsedRealtime` counts since boot and nothing can set it, so it is the
     * honest one. What it cannot do is survive a reboot, and it comes back
     * near zero — so a deadline written when the phone had been up three days
     * would read as three days of lockout. Hence the cap: neither clock can
     * assert more than one full cooldown, which is also the most any honest
     * lockout is worth. A reboot is therefore not an escape, and not a brick
     * either. It costs at most the wait that was already owed.
     */
    fun lockedFor(context: Context): Long {
        val p = prefs(context)
        val wall = p.getLong("locked_until", 0L) - System.currentTimeMillis()
        val mono = p.getLong("locked_until_elapsed", 0L) -
            android.os.SystemClock.elapsedRealtime()
        return (remaining(wall, mono) / 1000).coerceAtLeast(0)
    }

    /** The arithmetic of the two clocks, with no clock of its own to read. */
    internal fun remaining(wallMs: Long, monoMs: Long): Long =
        maxOf(
            wallMs.coerceAtMost(MAX_COOLDOWN_MS),
            monoMs.coerceAtMost(MAX_COOLDOWN_MS),
        ).coerceAtLeast(0)

    fun verify(context: Context, pin: String): Verdict {
        val p = prefs(context)
        val saltHex = p.getString("salt", null)
        val want = p.getString("verifier", null)
        if (saltHex.isNullOrBlank() || want.isNullOrBlank()) return Verdict.Unset

        val waiting = lockedFor(context)
        if (waiting > 0) return Verdict.Locked(waiting)

        val got = derive(pin, saltHex.unhex()).hex()
        // Constant time: a comparison that returns early on the first wrong
        // digit tells an attacker which digit was wrong.
        if (java.security.MessageDigest.isEqual(got.toByteArray(), want.toByteArray())) {
            p.edit().putInt("failures", 0)
                .putLong("locked_until", 0L)
                .putLong("locked_until_elapsed", 0L)
                .apply()
            return Verdict.Ok
        }
        val failures = p.getInt("failures", 0) + 1
        val e = p.edit().putInt("failures", failures)
        val over = failures - FREE_TRIES
        if (over > 0) {
            // Doubling, and remembered across restarts — a lockout that a
            // force-stop clears is not a lockout.
            val wait = (FIRST_COOLDOWN_MS shl (over - 1).coerceAtMost(20))
                .coerceAtMost(MAX_COOLDOWN_MS)
            e.putLong("locked_until", System.currentTimeMillis() + wait)
            e.putLong(
                "locked_until_elapsed",
                android.os.SystemClock.elapsedRealtime() + wait,
            )
            e.apply()
            DucatLog.w(TAG, "wrong PIN ×$failures — waiting ${wait / 1000}s")
            return Verdict.Locked(wait / 1000)
        }
        e.apply()
        return Verdict.Wrong(FREE_TRIES - failures)
    }

    private fun derive(pin: String, salt: ByteArray): ByteArray {
        val spec = javax.crypto.spec.PBEKeySpec(
            pin.toCharArray(), salt, ITERATIONS, LENGTH_BITS,
        )
        return javax.crypto.SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256")
            .generateSecret(spec).encoded
    }

    private fun ByteArray.hex(): String =
        joinToString("") { "%02x".format(it) }

    private fun String.unhex(): ByteArray =
        chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}
