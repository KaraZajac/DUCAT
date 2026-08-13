package org.ducatproject.ducat.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

/**
 * A QR of a contact card.
 *
 * Always black on white regardless of theme. A Catppuccin-tinted QR is a QR
 * some scanners fail to read, and a code that looks right and does not scan is
 * worse than one that looks plain.
 *
 * Error correction is **L**. §16.9 measured the card near 1 KB, and a higher
 * level pushes past what fits a phone screen at scannable module size — the
 * trade is deliberate, not a default left alone.
 */
@Composable
fun QrBlock(text: String) {
    val matrix = remember(text) {
        runCatching {
            QRCodeWriter().encode(
                text,
                BarcodeFormat.QR_CODE,
                512,
                512,
                mapOf(
                    EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.L,
                    EncodeHintType.MARGIN to 1,
                ),
            )
        }.getOrNull()
    }

    if (matrix == null) {
        Box(
            Modifier.fillMaxWidth().height(120.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                "Too large for a QR — use the link instead.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
        return
    }

    Box(
        Modifier
            .fillMaxWidth()
            .background(Color.White)
            .padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.size(260.dp)) {
            val cell = size.width / matrix.width
            for (y in 0 until matrix.height) {
                for (x in 0 until matrix.width) {
                    if (matrix.get(x, y)) {
                        drawRect(
                            color = Color.Black,
                            topLeft = Offset(x * cell, y * cell),
                            size = Size(cell, cell),
                        )
                    }
                }
            }
        }
    }
}
