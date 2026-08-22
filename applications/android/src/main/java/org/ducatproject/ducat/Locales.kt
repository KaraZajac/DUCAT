package org.ducatproject.ducat

// The language *choice* — a stored tag and the menu of endonyms — is
// the same on every client, so it lives here and the desk compiles it
// verbatim. Applying that choice is per-platform: Android wraps a
// Context in attachBaseContext (Localization.kt), the desk points its
// resource table at the tag (android/Resources.kt).

import android.content.Context
import java.util.Locale

/**
 * The app's language, chosen by the user or following the device.
 *
 * DUCAT is meant to be global — like the transport and the money under it —
 * so nothing a user reads is pinned to English. The stored value is a BCP-47
 * tag ("es", "pt-BR", "zh") or empty for "follow the system".
 *
 * It is applied by wrapping the base context in `attachBaseContext` (see
 * [LocaleWrapper]), which is the one hook that runs before any resource is
 * read — so a screen never briefly renders in the wrong language and then
 * corrects itself. Changing it recreates the activity, which re-runs that hook.
 */
class LocaleStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_locale", Context.MODE_PRIVATE)

    /** The chosen BCP-47 tag, or "" to follow the device. */
    fun tag(): String = prefs.getString("lang", "") ?: ""

    fun setTag(tag: String) = prefs.edit().putString("lang", tag).apply()

    /** The chosen locale, or null when following the device. */
    fun locale(): Locale? = tag().takeIf { it.isNotBlank() }?.let { Locale.forLanguageTag(it) }
}

/**
 * The languages offered in Settings. Each is named in **its own language**,
 * because a language menu written in a language you cannot read is not a menu
 * you can use — the whole point of the row is to be recognised by someone who
 * does not yet have the app in a language they understand.
 *
 * The tag is what Android resolves resources against; a `values-<tag>` folder
 * that does not exist yet falls back to the default (English) cleanly, so a
 * language can appear here before it is fully translated without breaking.
 */
object Languages {
    data class Lang(val tag: String, val endonym: String)

    /** "" is the device default, shown as its own row at the top. */
    val SUPPORTED = listOf(
        // English is a choice, not just what you get by default.
        //
        // It was reachable only through "follow the system", which is a
        // different question — it means "whatever this device is set to", and
        // the two coincide only when the device is already English. Somebody
        // reading English on a borrowed, secondhand or work phone set to
        // Arabic had nineteen languages to pick from and not their own. That
        // is the failure this menu's own rule describes: a language menu you
        // cannot use is not a menu.
        //
        // There is no `values-en`; English is the default `values/`, and
        // Android resolves the tag to it.
        Lang("en", "English"),
        Lang("es", "Español"),
        Lang("fr", "Français"),
        Lang("de", "Deutsch"),
        Lang("pt", "Português"),
        Lang("it", "Italiano"),
        Lang("nl", "Nederlands"),
        Lang("ru", "Русский"),
        Lang("uk", "Українська"),
        Lang("pl", "Polski"),
        Lang("tr", "Türkçe"),
        Lang("zh", "中文"),
        Lang("ja", "日本語"),
        Lang("ko", "한국어"),
        Lang("ar", "العربية"),
        Lang("fa", "فارسی"),
        Lang("hi", "हिन्दी"),
        Lang("id", "Bahasa Indonesia"),
        Lang("vi", "Tiếng Việt"),
        Lang("th", "ไทย"),
    )

    /** The endonym for a stored tag, for showing the current choice. */
    fun endonymFor(tag: String): String? = SUPPORTED.firstOrNull { it.tag == tag }?.endonym
}
