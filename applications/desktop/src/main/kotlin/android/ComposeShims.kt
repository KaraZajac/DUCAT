// The three Compose symbols a phone screen reaches for, desk editions.
//
// These live in Compose's own packages on purpose: the phone's sources say
// `import androidx.compose.ui.res.stringResource`, and the whole point is
// that those sources compile here unedited. Compose Desktop declares neither
// `stringResource` nor `LocalContext` (both are Android-only), so nothing is
// being shadowed — this fills genuine holes rather than overriding anything.

package androidx.compose.ui.res

import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.toComposeImageBitmap

@Composable
fun stringResource(id: Int): String = android.res.DeskRes.string(id)

@Composable
fun stringResource(id: Int, vararg formatArgs: Any): String =
    android.res.DeskRes.string(id, *formatArgs)

@Composable
fun pluralStringResource(id: Int, count: Int): String =
    android.res.DeskRes.plural(id, count)

@Composable
fun pluralStringResource(id: Int, count: Int, vararg formatArgs: Any): String =
    android.res.DeskRes.plural(id, count, *formatArgs)

/**
 * A drawable by id. Only the handful of raster drawables the phone's screens
 * name are shipped (generateDeskRes copies them); a vector XML has no meaning
 * off Android, so an id with no bitmap draws nothing rather than crashing a
 * screen over an ornament.
 */
@Composable
fun painterResource(id: Int): Painter = androidx.compose.runtime.remember(id) {
    val bytes = object {}.javaClass.getResourceAsStream("/deskres/drawable/$id.png")
        ?.use { it.readBytes() }
    if (bytes == null) {
        BitmapPainter(
            androidx.compose.ui.graphics.ImageBitmap(1, 1),
        )
    } else {
        BitmapPainter(
            javax.imageio.ImageIO.read(java.io.ByteArrayInputStream(bytes))
                .toComposeImageBitmap(),
        )
    }
}
