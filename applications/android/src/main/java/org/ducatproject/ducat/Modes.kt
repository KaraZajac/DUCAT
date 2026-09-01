package org.ducatproject.ducat

import android.content.Context

/**
 * Operating modes (§15): what this device is *for* right now.
 *
 * A mode is not a screen, it is a stance. A till is a till all day — the person
 * holding it rings up sale after sale, and making them navigate to a feature
 * before every customer is making them do it forty times a shift. So a mode
 * takes over the Home tab rather than living behind the drawer: switch it on
 * and the app leads with it, switch it off and the wallet comes back.
 *
 * One at a time. A device that is simultaneously a till and a taxi meter has
 * two ideas about what an arriving payment means.
 */
// None is Personal — the default, the wallet-and-chat app. Hail is gone from
// this list on purpose: hailing is a rider's moment, not a job, and it lives
// as a card on the personal Home screen. Drive folded into Taxi, because a
// taxi finds fares and runs a meter with the same hands. Renting is the
// owner's side of §16.18: personal mode is where someone looks for a car or
// a place, this is where someone has one to let.
// Marketplace is deliberately its own mode rather than a tab of Renting: a
// person selling a bicycle and a person letting a room are doing different
// jobs, and the board they share is an implementation detail of where the
// notice lands. Appended rather than inserted — `current()` reads the name
// out of preferences, so the order here is free but the spelling is not.
enum class Mode { None, Pos, BarTab, Taxi, Donate, Renting, Kiosk, Marketplace, HireHelp, Press }

class ModeStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")
    private val app = context.applicationContext

    fun current(): Mode =
        runCatching { Mode.valueOf(prefs.getString("mode_current", null) ?: "None") }
            .getOrDefault(Mode.None)

    /**
     * Switch the device's job.
     *
     * [browsing] says the mode was entered by tapping a tile on the wallet's
     * own home screen — somebody going to look at what is for sale nearby,
     * not somebody starting a shift. The two want opposite things from the
     * chrome: a shift wants the wallet out of the way until it is switched
     * off deliberately, and a look wants a way back.
     *
     * Without the distinction there was only the shift. Tapping "Rent gear"
     * to browse turned the phone into a renting terminal, permanently; the
     * only way back was a drawer item two levels down, and Back from the
     * shell's first tab did not return to the wallet — it left the app, and
     * relaunching came back into renting.
     */
    fun set(m: Mode, browsing: Boolean = false) {
        prefs.edit()
            .putString("mode_current", m.name)
            // Only a mode entered this way can be left this way. Choosing
            // Taxi from Operating modes means it, and should not sprout a
            // door out of a decision somebody already made.
            .putBoolean("mode_browsing", browsing && m != Mode.None)
            .apply()
        // A mode with a bound persona puts that hat on as the shift starts
        // — the shop answers as the shop, whatever was worn on the walk in.
        // Entry-time only, and only for a real shift: browsing changes
        // nothing, and the switcher can still change hats mid-shift (the
        // binding is a default, not a cage). setWorn ignores a persona
        // that no longer exists, so a stale binding degrades to "as worn".
        if (!browsing) {
            boundPersona(m)?.let { PersonaStore(app).setWorn(it) }
        }
        ContactStore.bump()
    }

    /** Whether the current mode was opened to look, rather than to work. */
    fun browsing(): Boolean = prefs.getBoolean("mode_browsing", false)

    /** The persona this mode answers as, or null for "whatever is worn". */
    fun boundPersona(m: Mode): String? =
        prefs.getString("mode_persona_${m.name}", null)

    fun bindPersona(m: Mode, hex: String?) {
        prefs.edit().apply {
            if (hex == null) remove("mode_persona_${m.name}")
            else putString("mode_persona_${m.name}", hex)
        }.apply()
        ContactStore.bump()
    }
}
