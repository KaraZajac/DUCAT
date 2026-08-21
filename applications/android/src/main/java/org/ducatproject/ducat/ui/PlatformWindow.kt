package org.ducatproject.ducat.ui

import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.window.DialogProperties

/**
 * A full-screen dialog's properties, phone edition.
 *
 * `decorFitsSystemWindows = false` is what lets these screens draw behind the
 * status and navigation bars — an Android-only concern, and the one line in
 * an otherwise portable screen that a desktop Compose build cannot compile.
 * Naming it here keeps the screens themselves shareable with the desk, which
 * supplies its own version of this file: the content crosses, the window
 * chrome stays where its platform is.
 */
fun fullScreenDialogProperties(dismissOnBackPress: Boolean = true): DialogProperties =
    DialogProperties(
        usePlatformDefaultWidth = false,
        dismissOnBackPress = dismissOnBackPress,
        decorFitsSystemWindows = false,
    )

/**
 * Where a voice memo is recorded, and — by its extension — in what format.
 *
 * A phone records AAC in MP4: universally decodable, and a minute of speech
 * lands near 250 KB. The desk's half of this file answers wav instead,
 * because a JVM ships no AAC encoder; the sender labels the attachment from
 * this name, so neither client has to assume what the other produced.
 */
fun voiceMemoFile(context: android.content.Context): java.io.File =
    java.io.File(context.cacheDir, "voice-memo.m4a")

/**
 * Point the system bar icons at the theme they are sitting on.
 *
 * The app draws behind the status bar, so whatever the system decides to
 * paint the clock and the battery with lands on the app's own background.
 * Left alone it decided white, on a background that is very nearly white, and
 * the clock had been all but invisible on every screen for as long as anyone
 * had been looking at it — masked only on the full-screen dialogs, where the
 * window dim happened to darken the strip enough to read.
 *
 * Takes the theme's own answer rather than the system's dark-mode flag,
 * because the theme can be set to Latte or Mocha explicitly and the bars have
 * to follow the app, not the phone.
 */
@androidx.compose.runtime.Composable
fun SystemBarIcons(dark: Boolean) {
    val view = androidx.compose.ui.platform.LocalView.current
    if (view.isInEditMode) return
    // The colour the bars are painted, as well as the icons drawn on them.
    //
    // This app does not draw edge to edge, so the strip behind the clock is
    // the *window's* background — which comes from Theme.DeviceDefault and is
    // light whatever DUCAT is doing. On Latte that happened to match and hid
    // the problem; on Mocha the app went to (30, 30, 46) and left a bright
    // white band across the top of every screen, with white icons on it.
    val bar = androidx.compose.material3.MaterialTheme.colorScheme.background.toArgb()
    androidx.compose.runtime.SideEffect {
        // The dialog's window when there is one, the activity's otherwise: a
        // full-screen dialog owns the bars while it is up, and unwrapping the
        // context is how a Compose view finds the activity it belongs to —
        // `view.context` is a ContextWrapper, not the Activity itself.
        val window = (view.parent as? androidx.compose.ui.window.DialogWindowProvider)?.window
            ?: generateSequence(view.context) {
                (it as? android.content.ContextWrapper)?.baseContext
            }.filterIsInstance<android.app.Activity>().firstOrNull()?.window
            ?: return@SideEffect
        @Suppress("DEPRECATION")
        run {
            window.statusBarColor = bar
            window.navigationBarColor = bar
        }
        androidx.core.view.WindowInsetsControllerCompat(window, window.decorView).apply {
            isAppearanceLightStatusBars = !dark
            isAppearanceLightNavigationBars = !dark
        }
    }
}
