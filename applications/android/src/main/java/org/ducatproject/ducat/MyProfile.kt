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
class MyProfile(context: Context, personaHex: String? = null) {
    private val prefs = securePrefs(context, "ducat_contacts")

    // One profile per persona (the profiles design): the presentation
    // follows the hat. The primary wears the unsuffixed keys the app has
    // always used — no migration, and the backup bundle's shape is
    // untouched — while every other persona keys its fields by its hex.
    // Null means the worn persona, which since the doorway rule (a mode
    // with a binding wears its persona on entry) is the acting identity
    // everywhere a profile is shown or sent.
    private val hex: String
    private val legacyKeys: Boolean
    init {
        val personas = PersonaStore(context)
        hex = personaHex ?: personas.worn()
        legacyKeys = hex == personas.personaHex()
    }
    private fun k(base: String) = if (legacyKeys) base else "$base|$hex"

    fun name(): String? = prefs.getString(k("my_name"), null)
    fun setName(v: String?) = put(k("my_name"), v?.trim()?.ifBlank { null })

    fun email(): String? = prefs.getString(k("my_email"), null)
    fun setEmail(v: String?) = put(k("my_email"), v?.trim()?.ifBlank { null })

    fun phone(): String? = prefs.getString(k("my_phone"), null)
    fun setPhone(v: String?) = put(k("my_phone"), v?.trim()?.ifBlank { null })

    fun signal(): String? = prefs.getString(k("my_signal"), null)
    fun setSignal(v: String?) = put(k("my_signal"), v?.trim()?.ifBlank { null })

    // The car (§15.12): what a rider looks for at the curb. Claims, like the
    // rest of this file — the rider's check is the bumper.
    fun carModel(): String? = prefs.getString(k("my_car_model"), null)
    fun setCarModel(v: String?) = put(k("my_car_model"), v?.trim()?.ifBlank { null })
    fun carColor(): String? = prefs.getString(k("my_car_color"), null)
    fun setCarColor(v: String?) = put(k("my_car_color"), v?.trim()?.ifBlank { null })
    fun plate(): String? = prefs.getString(k("my_plate"), null)
    fun setPlate(v: String?) = put(k("my_plate"), v?.trim()?.ifBlank { null })

    /** 1..6, matching `pronounOptions()`. Null means not set, which is not a
     *  failure state — someone with none renders like anyone else. */
    fun pronouns(): Int? = prefs.getInt(k("my_pronouns"), 0).takeIf { it in 1..6 }
    fun setPronouns(v: Int?) {
        prefs.edit().putInt(k("my_pronouns"), v ?: 0).apply(); ContactStore.bump()
    }

    fun avatar(): ByteArray? = prefs.getString(k("my_avatar"), null)
        ?.let { Base64.decode(it, Base64.NO_WRAP) }

    fun setAvatar(v: ByteArray?) {
        prefs.edit()
            .putString(k("my_avatar"), v?.let { Base64.encodeToString(it, Base64.NO_WRAP) })
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
    fun shareProfile(): Boolean = prefs.getBoolean(k("share_profile"), true)
    fun setShareProfile(v: Boolean) {
        prefs.edit().putBoolean(k("share_profile"), v).apply(); ContactStore.bump()
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
         * Returns null when the value is acceptable, or the string resource
         * naming the reason it is not — an id rather than a sentence, because
         * these were the last English sentences hardcoded on a translated
         * screen. Kept beside each other on purpose: two copies of a rule
         * drift, and these are checked against the vectors on the other side
         * of the bridge.
         */
        fun emailProblem(v: String): Int? {
            if (v.isBlank()) return null
            val ok = Regex("^[A-Za-z0-9._%+\\-']+@[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)*\\.[A-Za-z]{2,}$")
            return when {
                v.length > 254 -> R.string.myprofile_email_too_long
                !ok.matches(v) -> R.string.myprofile_email_shape
                v.contains("..") -> R.string.myprofile_email_shape
                else -> null
            }
        }

        fun phoneProblem(v: String): Int? {
            if (v.isBlank()) return null
            return when {
                !v.all { it.isDigit() } -> R.string.myprofile_phone_digits
                v.length > 15 -> R.string.myprofile_phone_too_long
                else -> null
            }
        }

        fun signalProblem(v: String): Int? {
            if (v.isBlank()) return null
            val ok = Regex("^[A-Za-z_][A-Za-z0-9_]{2,}\\.[0-9]{2,}$")
            return when {
                v.length > 48 -> R.string.myprofile_signal_too_long
                !ok.matches(v) -> R.string.myprofile_signal_shape
                else -> null
            }
        }
    }
}
