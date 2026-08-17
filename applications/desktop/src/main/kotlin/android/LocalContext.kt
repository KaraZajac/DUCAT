// `LocalContext.current` — Android-only in Compose, and named by two dozen
// phone screens. The desk provides its own Context at the root of the window
// (see Main.kt), so the screens find one exactly where they expect it.

package androidx.compose.ui.platform

import androidx.compose.runtime.staticCompositionLocalOf

val LocalContext = staticCompositionLocalOf<android.content.Context> {
    error(
        "No Context in this composition. The desk provides one at the window " +
            "root: CompositionLocalProvider(LocalContext provides context) { … }",
    )
}
