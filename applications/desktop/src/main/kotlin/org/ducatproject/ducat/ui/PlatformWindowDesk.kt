package org.ducatproject.ducat.ui

import androidx.compose.ui.window.DialogProperties

/**
 * The desk's half of the phone's PlatformWindow.kt: same function, same
 * meaning — a dialog that is allowed to be as large as its content wants.
 * There are no system bars to draw behind here, so the phone's third flag
 * has nothing to say.
 */
fun fullScreenDialogProperties(dismissOnBackPress: Boolean = true): DialogProperties =
    DialogProperties(
        usePlatformDefaultWidth = false,
        dismissOnBackPress = dismissOnBackPress,
    )

/**
 * Where a voice memo is recorded here, and in what format: WAV, because the
 * JVM has no AAC encoder and a mislabelled m4a is worse than a larger file
 * that every decoder — including Android's — actually reads.
 */
fun voiceMemoFile(context: android.content.Context): java.io.File =
    java.io.File(context.cacheDir, "voice-memo.wav")

/**
 * The desk's half of SystemBarIcons: nothing to point. A desktop window has
 * no status bar of the operating system's to tint.
 */
@androidx.compose.runtime.Composable
fun SystemBarIcons(dark: Boolean) = Unit
