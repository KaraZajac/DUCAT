package org.ducatproject.ducat

import android.content.Context
import android.content.res.Configuration
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

/**
 * Applies the stored language to a context, for `attachBaseContext`.
 *
 * Also sets the JVM default locale, so number, currency and date formatting
 * done through `String.format` / `java.text` follows the same choice the
 * resources do — otherwise a screen could read in Spanish while its amounts
 * used the device's separators.
 */
object LocaleWrapper {
    fun wrap(base: Context): Context {
        val locale = LocaleStore(base).locale() ?: return base
        Locale.setDefault(locale)
        val config = Configuration(base.resources.configuration)
        config.setLocale(locale)
        // Right-to-left languages (Arabic, Persian) carry their layout
        // direction with the locale; supportsRtl in the manifest lets it apply.
        config.setLayoutDirection(locale)
        return base.createConfigurationContext(config)
    }
}

/** The Activity behind a (possibly wrapped) Context, for recreate() on a
 *  language change. */
fun Context.findActivity(): android.app.Activity? {
    var c: Context? = this
    while (c is android.content.ContextWrapper) {
        if (c is android.app.Activity) return c
        c = c.baseContext
    }
    return null
}
