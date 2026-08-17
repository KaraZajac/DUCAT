package org.ducatproject.ducat.ui

import androidx.compose.ui.window.DialogProperties

/**
 * The desk's half of the phone's PlatformWindow.kt: same function, same
 * meaning — a dialog that is allowed to be as large as its content wants.
 * There are no system bars to draw behind here, so the phone's third flag
 * has nothing to say.
 */
fun fullScreenDialogProperties(): DialogProperties = DialogProperties(
    usePlatformDefaultWidth = false,
    dismissOnBackPress = true,
)
