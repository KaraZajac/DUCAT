package org.ducatproject.ducat

import android.content.Context
import android.util.Base64
import uniffi.ducat_mobile.Profile

/**
 * What this person publishes about themselves (§16.9).
 *
 * **None of it rides the card.** The card is a QR code someone scans across a
 * counter; a picture does not fit in one, and everything here would make it
 * unscannable. It travels on the contact record instead, which is also why it
 * can change afterwards without reissuing anything — the initial card only has
 * to get the two of you connected.
 *
 * Validation lives in `core`, not here. A screen that checks and a wire format
 * that does not is a wire format that accepts whatever a modified client sends,
 * and these fields render as identity on somebody else's phone. What this file
 * does is *mirror* those rules so a person is told at the keyboard rather than
 * refused after publishing.
 */
class MyProfile(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

    fun name(): String? = prefs.getString("my_name", null)
    fun setName(v: String?) = put("my_name", v?.trim()?.ifBlank { null })

    fun email(): String? = prefs.getString("my_email", null)
    fun setEmail(v: String?) = put("my_email", v?.trim()?.ifBlank { null })

    fun phone(): String? = prefs.getString("my_phone", null)
    fun setPhone(v: String?) = put("my_phone", v?.trim()?.ifBlank { null })

    fun signal(): String? = prefs.getString("my_signal", null)
    fun setSignal(v: String?) = put("my_signal", v?.trim()?.ifBlank { null })

    // The car (§15.12): what a rider looks for at the curb. Claims, like the
    // rest of this file — the rider's check is the bumper.
    fun carModel(): String? = prefs.getString("my_car_model", null)
    fun setCarModel(v: String?) = put("my_car_model", v?.trim()?.ifBlank { null })
    fun carColor(): String? = prefs.getString("my_car_color", null)
    fun setCarColor(v: String?) = put("my_car_color", v?.trim()?.ifBlank { null })
    fun plate(): String? = prefs.getString("my_plate", null)
    fun setPlate(v: String?) = put("my_plate", v?.trim()?.ifBlank { null })

    /** 1..6, matching `pronounOptions()`. Null means not set, which is not a
     *  failure state — someone with none renders like anyone else. */
    fun pronouns(): Int? = prefs.getInt("my_pronouns", 0).takeIf { it in 1..6 }
    fun setPronouns(v: Int?) {
        prefs.edit().putInt("my_pronouns", v ?: 0).apply(); ContactStore.bump()
    }

    fun avatar(): ByteArray? = prefs.getString("my_avatar", null)
        ?.let { Base64.decode(it, Base64.NO_WRAP) }

    fun setAvatar(v: ByteArray?) {
        prefs.edit()
            .putString("my_avatar", v?.let { Base64.encodeToString(it, Base64.NO_WRAP) })
            .apply()
        ContactStore.bump()
    }

    /**
     * Whether the optional fields go out with a new contact.
     *
     * On by default, which is a deliberate reversal of how `publish_address`
     * started and is worth being honest about: a default that shares is a
     * default that shares for people who never opened this screen. It is here
     * because a wallet whose contacts are anonymous hex is one nobody uses, and
     * because these fields — a name, a face, pronouns — are what a person hands
     * over in the same gesture anyway. The address is the one with a lasting
     * cost, and it has its own switch.
     */
    fun shareProfile(): Boolean = prefs.getBoolean("share_profile", true)
    fun setShareProfile(v: Boolean) {
        prefs.edit().putBoolean("share_profile", v).apply(); ContactStore.bump()
    }

    /** What actually goes on the record, after the share switch.
     *
     *  Scoped to the handshake (§16.9), the same way the plate is. Email, phone
     *  and signal are real-world identifiers — ways to reach a person off
     *  DUCAT — and they ride only a deliberate contact exchange ([purpose] ==
     *  "profile"), never a till, a tab, a ride or a hail. A plate has no
     *  business on a card handed across a bar; neither does a phone number.
     *
     *  The car rides only when [driving] — the one moment a rider scans a curb
     *  for a stranger's vehicle (§15.12). A face, a name and pronouns are the
     *  low-cost gesture of introduction and ride wherever the share switch
     *  allows: recognising who is at the counter is worth something and locates
     *  no one off the app.
     *
     *  [purpose] null (an older peer's card, or one that did not say) is read
     *  as *not* a contact exchange — the private default. */
    fun toWire(purpose: String? = "profile", driving: Boolean = false): Profile {
        if (!shareProfile()) return Profile(null, null, null, null, null, null, null, null)
        val relational = purpose == "profile"
        return Profile(
            avatar = avatar(),
            email = if (relational) email() else null,
            phone = if (relational) phone() else null,
            signal = if (relational) signal() else null,
            pronouns = pronouns()?.toUInt(),
            carModel = if (driving) carModel() else null,
            carColor = if (driving) carColor() else null,
            plate = if (driving) plate() else null,
        )
    }

    private fun put(key: String, v: String?) {
        prefs.edit().putString(key, v).apply()
        ContactStore.bump()
    }

    companion object {
        /**
         * The same rules `core` enforces, so a person is corrected while typing
         * rather than after publishing.
         *
         * Returns null when the value is acceptable, or the reason it is not.
         * Kept beside each other on purpose: two copies of a rule drift, and
         * these are checked against the vectors on the other side of the bridge.
         */
        fun emailProblem(v: String): String? {
            if (v.isBlank()) return null
            val ok = Regex("^[A-Za-z0-9._%+\\-']+@[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)*\\.[A-Za-z]{2,}$")
            return when {
                v.length > 254 -> "That is longer than an email address can be."
                !ok.matches(v) -> "That is not the shape of an email address."
                v.contains("..") -> "That is not the shape of an email address."
                else -> null
            }
        }

        fun phoneProblem(v: String): String? {
            if (v.isBlank()) return null
            return when {
                !v.all { it.isDigit() } ->
                    "Digits only — no +, spaces or brackets. The country code is digits too."
                v.length > 15 -> "A phone number is at most 15 digits."
                else -> null
            }
        }

        fun signalProblem(v: String): String? {
            if (v.isBlank()) return null
            val ok = Regex("^[A-Za-z_][A-Za-z0-9_]{2,}\\.[0-9]{2,}$")
            return when {
                v.length > 48 -> "That is longer than a Signal username can be."
                !ok.matches(v) -> "A Signal username looks like name.12 — a name, a dot, then digits."
                else -> null
            }
        }
    }
}
