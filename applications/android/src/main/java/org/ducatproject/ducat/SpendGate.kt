package org.ducatproject.ducat

import android.content.Context

/**
 * Whether the person holding this phone may spend this much, right now
 * (§15.5.1).
 *
 * WYSIWYS says a payer sees exactly what they sign. It says nothing about
 * *whether the person holding the device should be signing at all* — a stolen
 * unlocked phone is a bearer instrument, and that is how people lose money
 * rather than how the protocol fails.
 *
 * The shape is the one a phone's own payment app already has, because that is
 * the one people already know:
 *
 *  - **Unlock the phone, open the app, tap.** A device unlocked within the
 *    last two minutes carries any payment below the threshold. There is no
 *    tap-and-go tier below that, because what leaves is money in a wallet
 *    nobody can reverse, not a transit fare.
 *  - **Above the threshold, the PIN, every time.** One number, set by the
 *    user, defaulting to a hundred of whatever they price in. "Every time"
 *    and not "once": a PIN read over a shoulder is then worth one large
 *    payment to whoever takes the phone, not as many as they can tap inside
 *    a validity window.
 *  - **A rolling hour also counts.** A per-payment limit alone does not stop
 *    twenty payments just under it, which is how a lifted phone is actually
 *    drained.
 *
 * The arithmetic is core's ([uniffi.ducat_mobile.checkVerification]); this
 * file is the three things core cannot know — what the user set, what the
 * phone can attest to, and what has already been spent this hour.
 *
 * **None of this goes on the wire.** A payee never learns which tier was
 * satisfied and cannot ask for one; if a counterparty could influence
 * verification it would ask for the weakest, which is the downgrade attack
 * EMV spent years patching.
 */
object SpendGate {
    private const val TAG = "DucatSpendGate"

    /** What the caller must do before the money may leave. */
    sealed interface Decision {
        /** Nothing more is needed. */
        data object Allow : Decision
        /** Raise the app's PIN gate, and send only if it passes. */
        data class AskPin(val why: Int) : Decision
    }

    private fun prefs(context: Context) = securePrefs(context, "ducat_contacts")

    /**
     * The one threshold a user sets, in the **minor units of the currency
     * they price in** — never piconero, which would silently turn a "$100
     * limit" into a $70 one the next time the price moved.
     *
     * Null means "the default", which is read from core rather than repeated
     * here so there is one answer to what it is.
     */
    fun pinAbove(context: Context): Long {
        val set = prefs(context).getLong("spend_pin_above", -1L)
        return if (set >= 0) set else uniffi.ducat_mobile.defaultVerificationPolicy().appSecretAt.toLong()
    }

    fun setPinAbove(context: Context, minorUnits: Long) {
        prefs(context).edit().putLong("spend_pin_above", minorUnits.coerceAtLeast(0L)).apply()
        ContactStore.bump()
    }

    /** The policy as this phone is set up: core's defaults, the user's number. */
    fun policy(context: Context): uniffi.ducat_mobile.VerificationPolicy {
        val d = uniffi.ducat_mobile.defaultVerificationPolicy()
        return uniffi.ducat_mobile.VerificationPolicy(
            deviceUnlockAt = d.deviceUnlockAt,
            deviceUnlockValidityS = d.deviceUnlockValidityS,
            appSecretAt = pinAbove(context).toULong(),
            appSecretValidityS = d.appSecretValidityS,
            appSecretEveryTime = d.appSecretEveryTime,
            cumulativeAt = d.cumulativeAt,
            cumulativeWindowS = d.cumulativeWindowS,
        )
    }

    // --- the rolling hour ---------------------------------------------------
    //
    // Kept as a short list of (when, how much) rather than a running total,
    // because a total has no way to forget: the window has to slide, and a
    // counter that only ever grows locks a phone out an hour after a busy
    // afternoon and never lets it back in.

    /** Remember a payment that actually left, for the velocity rule. */
    fun record(context: Context, minorUnits: Long) {
        if (minorUnits <= 0) return
        val p = policy(context)
        val now = System.currentTimeMillis() / 1000
        val kept = spends(context, now, p.cumulativeWindowS.toLong()) + (now to minorUnits)
        val arr = org.json.JSONArray()
        kept.takeLast(200).forEach { (at, amt) ->
            arr.put(org.json.JSONObject().put("at", at).put("m", amt))
        }
        prefs(context).edit().putString("spend_window", arr.toString()).apply()
    }

    /** What has been spent inside the window, in minor units. */
    fun spentInWindow(context: Context): Long {
        val p = policy(context)
        val now = System.currentTimeMillis() / 1000
        return spends(context, now, p.cumulativeWindowS.toLong()).sumOf { it.second }
    }

    private fun spends(context: Context, now: Long, windowS: Long): List<Pair<Long, Long>> {
        val raw = prefs(context).getString("spend_window", null) ?: return emptyList()
        val arr = runCatching { org.json.JSONArray(raw) }.getOrNull() ?: return emptyList()
        val out = ArrayList<Pair<Long, Long>>(arr.length())
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            val at = o.optLong("at")
            // A stamp from the future is a phone whose clock moved, and
            // counting it would hold the window open indefinitely. Dropped,
            // the same way a stale rate stamp is disbelieved.
            if (at > now + 60 || now - at > windowS) continue
            out.add(at to o.optLong("m"))
        }
        return out
    }

    /**
     * Piconero as the user's own money, in minor units, or zero when there is
     * no rate this phone can vouch for.
     *
     * Zero rather than a guess: the callers are the velocity counter and the
     * threshold test, and both are better off counting nothing than counting
     * an invented conversion. [decide] handles the no-rate case separately,
     * by escalating.
     */
    fun minorUnits(context: Context, pxmr: Long): Long {
        val store = RateStore(context)
        if (!store.enabled() || store.isStale()) return 0L
        val rate = store.cached()?.first ?: return 0L
        return (pxmr / 1_000_000_000_000.0 * rate * 100.0).toLong()
    }

    // --- the decision -------------------------------------------------------

    /**
     * May this payment be signed?
     *
     * `amountPxmr` is converted to the user's currency here, because the
     * thresholds are denominated in real money. **A rate this phone cannot
     * vouch for escalates to the PIN rather than relaxing anything**: without
     * a trustworthy rate the client cannot tell which rung it is on, and
     * failing the other way would let anyone able to stall a rate feed lower
     * the requirement — a liveness problem turned into a security one.
     */
    fun decide(context: Context, amountPxmr: Long): Decision {
        val p = policy(context)
        val store = RateStore(context)
        val rate = if (store.enabled() && !store.isStale()) store.cached()?.first else null
        val minor = minorUnits(context, amountPxmr)
        // The window the policy names, asked of the platform as a window:
        // Android answers "was there an authentication inside N seconds", not
        // "how long ago", so the number has to come from here. Null is a
        // phone with no secure lock screen — nothing established, and the
        // app's own secret is what is left.
        val unlocked = DeviceLock.authenticatedWithin(context, p.deviceUnlockValidityS.toInt()) ?: false
        val out = uniffi.ducat_mobile.checkVerification(
            p,
            unlocked,
            Pin.secretAgeSecs(context)?.toULong(),
            minor.toULong(),
            spentInWindow(context).toULong(),
            rate != null,
        )
        if (out.permitted) return Decision.Allow
        DucatLog.i(TAG, "spend gate: required ${out.required} satisfied ${out.satisfied}")
        return Decision.AskPin(
            when {
                rate == null -> R.string.spend_gate_no_rate
                out.required == uniffi.ducat_mobile.Verification.APP_SECRET &&
                    minor >= p.appSecretAt.toLong() -> R.string.spend_gate_over_threshold
                out.required == uniffi.ducat_mobile.Verification.APP_SECRET -> R.string.spend_gate_hour
                else -> R.string.spend_gate_locked
            },
        )
    }
}
