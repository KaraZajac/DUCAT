package org.ducatproject.ducat

import android.content.Context
import org.json.JSONObject

/**
 * What a conversation is about, when it began at a board (§16.18).
 *
 * A rental card is minted per posting and claimed once, so the card that
 * opened a thread names exactly one listing — and both ends can know which.
 * The seeker knows because they tapped it; the owner knows because the card
 * the stranger answered was cut for that listing and no other.
 *
 * Without this the thread is two people and no subject: the owner types the
 * rent and both deposits back in by hand, from a listing they wrote and the
 * app already has, while the seeker stares at a chat with a name on it and
 * has to remember which of the cars they asked about.
 *
 * Deliberately a copy, not a pointer. A listing expires off its board after a
 * day and can be edited or deleted at any time; the conversation about it
 * outlives all of that, and "the Corolla, 0.040000 XMR a day" is what was
 * being discussed whatever the listing says next week.
 */
object Enquiries {
    private fun prefs(context: Context) = securePrefs(context, "ducat_enquiries")

    /** The listing a thread is about, as it stood when the thread opened. */
    data class About(
        val title: String,
        val pricePxmr: Long,
        val depositPxmr: Long,
        /** [Listings.KIND_VEHICLE] or [Listings.KIND_PLACE]. */
        val kind: Int,
        /**
         * Which listing, on the side that owns it — so the address and the
         * key handover, which live on the listing and never on a board, can
         * be offered once there is a booking to give them to. Empty on the
         * seeker's side, which knows the notice but not the record behind it.
         */
        val listingId: String = "",
    )

    /**
     * Note what this person and I are talking about. First write wins: a
     * second claim cannot repaint an older conversation's subject.
     */
    private val lock = Any()

    fun remember(context: Context, personaHex: String, about: About) = synchronized(lock) {
        if (personaHex.isBlank()) return
        val p = prefs(context)
        // Under the lock, or "first write wins" is only true when nothing
        // races: two claims arriving together both pass this check and the
        // later one repaints the subject the earlier one set.
        if (p.contains(key(personaHex))) return
        p.edit().putString(
            key(personaHex),
            JSONObject()
                .put("title", about.title)
                .put("price", about.pricePxmr)
                .put("deposit", about.depositPxmr)
                .put("kind", about.kind)
                .put("listing", about.listingId)
                .toString(),
        ).apply()
    }

    fun about(context: Context, personaHex: String): About? {
        val raw = prefs(context).getString(key(personaHex), null) ?: return null
        return runCatching {
            val o = JSONObject(raw)
            About(
                title = o.optString("title"),
                pricePxmr = o.optLong("price"),
                depositPxmr = o.optLong("deposit"),
                kind = o.optInt("kind"),
                listingId = o.optString("listing"),
            )
        }.getOrNull()?.takeIf { it.title.isNotBlank() }
    }

    /** Forgetting a contact forgets what they asked about. */
    fun forget(context: Context, personaHex: String) =
        prefs(context).edit().remove(key(personaHex)).apply()

    private fun key(personaHex: String) = "about_$personaHex"
}
