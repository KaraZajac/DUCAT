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
enum class Mode { None, Pos, BarTab, Taxi, Donate }

class ModeStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    fun current(): Mode =
        runCatching { Mode.valueOf(prefs.getString("mode_current", null) ?: "None") }
            .getOrDefault(Mode.None)

    fun set(m: Mode) {
        prefs.edit().putString("mode_current", m.name).apply()
        ContactStore.bump()
    }
}
