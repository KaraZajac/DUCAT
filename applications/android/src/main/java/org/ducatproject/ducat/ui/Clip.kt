package org.ducatproject.ducat.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.widget.Toast

/**
 * Copy, and say so.
 *
 * Below Android 13 the system draws nothing when a clip lands, so a Copy
 * button that only copies is a tap with no evidence either way — half the
 * screens said "copied" out loud and half went silent, depending on which
 * clipboard API they happened to reach for. From 13 the platform shows its
 * own overlay and a toast on top says it twice, so the toast keeps to
 * where it is the only voice. The desk never draws a system overlay (its
 * shim reports SDK 0) and its Toast surfaces in the window, so both
 * clients end up telling the truth exactly once.
 *
 * [said] is a complete localized sentence ("Link copied"), never a noun
 * for a format string to absorb — nineteen languages of gender agreement
 * on the participle is not a formatting problem worth having.
 */
fun copyText(context: Context, value: String, said: String) {
    (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager)
        .setPrimaryClip(ClipData.newPlainText(said, value))
    if (Build.VERSION.SDK_INT < 33) {
        Toast.makeText(context, said, Toast.LENGTH_SHORT).show()
    }
}
