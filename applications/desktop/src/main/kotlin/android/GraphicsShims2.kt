// `bitmap.asImageBitmap()` — Android-only in Compose, and the last step of
// every QR the phone draws and every avatar it shows.

package androidx.compose.ui.graphics

fun android.graphics.Bitmap.asImageBitmap(): ImageBitmap {
    image?.let { return it.toComposeImageBitmap() }
    val img = java.awt.image.BufferedImage(
        width, height, java.awt.image.BufferedImage.TYPE_INT_RGB,
    )
    img.setRGB(0, 0, width, height, pixels, 0, width)
    return img.toComposeImageBitmap()
}
