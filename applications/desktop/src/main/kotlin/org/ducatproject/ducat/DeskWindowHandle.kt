package org.ducatproject.ducat

import android.content.Context

/**
 * What `context.findActivity()?.recreate()` means on a desk.
 *
 * The phone applies a language change by recreating its Activity, because
 * `attachBaseContext` is the only hook that runs before a resource is read.
 * A desk has no Activity and no such hook — it points its resource table at
 * the new tag and redraws. Same call site (Settings), same outcome (the
 * whole window in the chosen language), so the Drawer needs no desk edit.
 */
class DeskWindowHandle(private val context: Context) {
    fun recreate() {
        android.res.DeskRes.setLocale(LocaleStore(context).tag())
        LocaleStore(context).locale()?.let { java.util.Locale.setDefault(it) }
        uiEpoch.value = uiEpoch.value + 1
    }
}

/** Bumped whenever the window must redraw from scratch (a language change). */
val uiEpoch = kotlinx.coroutines.flow.MutableStateFlow(0)

fun Context.findActivity(): DeskWindowHandle? = DeskWindowHandle(this)
