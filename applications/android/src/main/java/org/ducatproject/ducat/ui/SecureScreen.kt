package org.ducatproject.ducat.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.Window
import android.view.WindowManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.window.DialogWindowProvider

/**
 * Keep whatever this sits in off screenshots, screen recordings, casting
 * and the recents thumbnail, for as long as it is on screen.
 *
 * Not the whole activity. A QR card exists to be screenshotted and passed
 * on, and the gallery tooling reads the screen; what needs the flag is the
 * handful of surfaces where a secret is typed or shown — the PIN, a backup
 * passphrase, a restored wallet's address — and only while they are up. So
 * the flag is set on the window the composable actually lives in: a dialog
 * has a window of its own, and securing the activity underneath it would
 * leave the dialog's surface in every recording.
 *
 * Reference-counted per window, because two of these can overlap — the PIN
 * gate raised over the backup card — and the first to leave must not strip
 * the flag from the one still showing.
 *
 * The desk compiles a twin of this that does nothing: a laptop window has no
 * such flag, and the desk's screens carry the call unchanged.
 */
@Composable
fun SecureWhileShown() {
    val view = LocalView.current
    DisposableEffect(view) {
        // Inside a Dialog the view's parent is the dialog's own root, which
        // hands out that window; anywhere else, the activity's.
        val window = (view.parent as? DialogWindowProvider)?.window
            ?: view.context.activity()?.window
            ?: return@DisposableEffect onDispose {}
        SecureWindows.hold(window)
        onDispose { SecureWindows.release(window) }
    }
}

private object SecureWindows {
    // Identity, not equality: two Window objects are two windows.
    private val held = java.util.IdentityHashMap<Window, Int>()

    fun hold(w: Window) = synchronized(held) {
        val n = (held[w] ?: 0) + 1
        held[w] = n
        if (n == 1) w.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }

    fun release(w: Window) = synchronized(held) {
        val n = (held[w] ?: 0) - 1
        if (n <= 0) {
            held.remove(w)
            w.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            held[w] = n
        }
    }
}

/** The activity behind a view's context, through however many wrappers. */
private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}
