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
