package org.ducatproject.ducat

import android.content.Context
import android.content.res.Configuration
import java.util.Locale

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
