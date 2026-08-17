// android.graphics, in the handful of calls the screens make of it: build a
// pixel buffer for a QR, decode a contact's picture, hand either to Compose.

package android.graphics

class Bitmap internal constructor(
    val width: Int,
    val height: Int,
    internal val image: java.awt.image.BufferedImage? = null,
) {
    /** Only used by the QR path, which writes its own pixels. */
    val pixels: IntArray by lazy { IntArray(width * height) }

    enum class Config { RGB_565, ARGB_8888 }

    fun setPixels(
        src: IntArray,
        offset: Int,
        stride: Int,
        x: Int,
        y: Int,
        w: Int,
        h: Int,
    ) {
        for (row in 0 until h) {
            System.arraycopy(src, offset + row * stride, pixels, (y + row) * width + x, w)
        }
    }

    companion object {
        @JvmStatic
        fun createBitmap(w: Int, h: Int, config: Config): Bitmap = Bitmap(w, h)
    }
}

/**
 * Contact pictures arrive as sealed bytes (§16.15) and are decoded on both
 * clients from the same call site; ImageIO reads what the phone's decoder
 * reads. A picture that will not decode returns null exactly as Android's
 * does, and the caller already draws initials in that case.
 */
object BitmapFactory {
    @JvmStatic
    fun decodeByteArray(data: ByteArray?, offset: Int, length: Int): Bitmap? = runCatching {
        if (data == null || length <= 0) return null
        val img = javax.imageio.ImageIO.read(
            java.io.ByteArrayInputStream(data, offset, length),
        ) ?: return null
        Bitmap(img.width, img.height, img)
    }.getOrNull()
}
