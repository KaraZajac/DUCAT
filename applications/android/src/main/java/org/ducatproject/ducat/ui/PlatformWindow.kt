package org.ducatproject.ducat.ui

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
fun fullScreenDialogProperties(): DialogProperties = DialogProperties(
    usePlatformDefaultWidth = false,
    dismissOnBackPress = true,
    decorFitsSystemWindows = false,
)
