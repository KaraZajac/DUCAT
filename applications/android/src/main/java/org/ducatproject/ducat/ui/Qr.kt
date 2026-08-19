package org.ducatproject.ducat.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import org.ducatproject.ducat.R
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * A QR code, drawn as **one bitmap**.
 *
 * The first version drew the matrix cell by cell in a `Canvas`. That reads
 * naturally and is catastrophic, for a reason worth writing down: ZXing's
 * `encode(text, format, width, height)` returns a matrix at the **requested
 * pixel size**, not at the QR's module count. Asking for 512×512 therefore
 * produced a 512×512 matrix, and the loop issued up to **262,144 `drawRect`
 * calls per frame**, on the main thread, on every recomposition. It lagged, and
 * then it took the app down — which also killed the Veilid node and left the
 * routes in every card that had been handed out pointing nowhere.
 *
 * Now: encoded once off the main thread, converted to a single bitmap, drawn
 * with one call. `FilterQuality.None` because a QR must scale by nearest
 * neighbour — smoothing blurs the module edges and scanners start failing.
 *
 * Always black on white regardless of theme. A Catppuccin-tinted QR is a QR
 * some scanners refuse, and a code that looks right and does not scan is worse
 * than one that looks plain.
 */
@Composable
fun QrBlock(text: String) {
    // The producer does assign — that is the line below. Compose's lint fails
    // to see it through `by` plus a suspend right-hand side, and reports the
    // producer as never assigning either way round, with or without a local
    // in between. Suppressed rather than contorted, because one standing
    // error is how a lint run stops being read.
    @android.annotation.SuppressLint("ProduceStateDoesNotAssignValue")
    val bitmap by produceState<Result<ImageBitmap>?>(null, text) {
        value = withContext(Dispatchers.Default) { encodeQr(text) }
    }

    Box(
        Modifier.fillMaxWidth().background(Color.White).padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        val b = bitmap
        when {
            b == null -> Box(Modifier.size(260.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
            b.isFailure -> Text(
                stringResource(R.string.qr_too_much_data),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
            else -> Image(
                bitmap = b.getOrThrow(),
                contentDescription = stringResource(R.string.qr_code),
                modifier = Modifier.size(260.dp),
                contentScale = ContentScale.Fit,
                filterQuality = FilterQuality.None,
            )
        }
    }
}

private fun encodeQr(text: String): Result<ImageBitmap> = runCatching {
    // Error correction L: §16.9's card is already near a version-31 symbol, and
    // a higher level pushes it past what scans from a phone screen. Chosen, not
    // left at a default.
    val matrix = QRCodeWriter().encode(
        text,
        BarcodeFormat.QR_CODE,
        0, // zero asks ZXing for the natural module count rather than a
        0, // pixel size, which is what makes the bitmap small and cheap.
        mapOf(
            EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.L,
            EncodeHintType.MARGIN to 1,
        ),
    )
    val w = matrix.width
    val h = matrix.height
    val pixels = IntArray(w * h)
    for (y in 0 until h) {
        val row = y * w
        for (x in 0 until w) {
            pixels[row + x] = if (matrix.get(x, y)) 0xFF000000.toInt() else 0xFFFFFFFF.toInt()
        }
    }
    Bitmap.createBitmap(w, h, Bitmap.Config.RGB_565)
        .apply { setPixels(pixels, 0, w, 0, 0, w, h) }
        .asImageBitmap()
}
