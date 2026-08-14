package org.ducatproject.ducat.ui

import android.app.Activity
import android.nfc.NfcAdapter
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.nfc.Tap

/**
 * The reading half of the tap, as an effect any scan surface can carry.
 *
 * While the composable is on screen, this phone is a reader: another phone's
 * card, or an NDEF sticker, lands in [onUri] through the same door a QR scan
 * uses — one code path for "something arrived", however it travelled, which is
 * §15.3.2's ladder made literal.
 *
 * Reader mode suspends this phone's own card emulation while active, which is
 * the right priority for a screen whose stated purpose is reading. Screens
 * that *offer* — the code screen's My-code tab, a till — must not compose
 * this. No NFC hardware means the effect is silently nothing: QR remains the
 * floor that always works.
 */
@Composable
fun NfcTapReader(onUri: (String) -> Unit) {
    val context = LocalContext.current
    val handler = rememberUpdatedState(onUri)

    DisposableEffect(Unit) {
        val activity = context as? Activity ?: return@DisposableEffect onDispose {}
        val adapter = NfcAdapter.getDefaultAdapter(activity)
            ?: run {
                DucatLog.i("Tap", "no NFC hardware — QR only")
                return@DisposableEffect onDispose {}
            }
        DucatLog.i("Tap", "reader mode on")

        val main = Handler(Looper.getMainLooper())
        adapter.enableReaderMode(
            activity,
            { tag ->
                // Binder thread. The transceive walk is blocking and belongs
                // here; the result crosses to the main thread before touching
                // compose state.
                val uri = runCatching { Tap.read(tag) }.getOrNull()
                if (uri != null) {
                    DucatLog.i("Tap", "read ${uri.length} chars over NFC")
                    main.post { handler.value(uri) }
                }
            },
            // A and B cover phones (ISO-DEP rides both) and most stickers.
            // SKIP_NDEF_CHECK is deliberately absent: we want the platform to
            // parse NDEF for us on plain tags.
            NfcAdapter.FLAG_READER_NFC_A or NfcAdapter.FLAG_READER_NFC_B,
            null,
        )
        onDispose {
            DucatLog.i("Tap", "reader mode off")
            runCatching { adapter.disableReaderMode(activity) }
        }
    }
}
