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

    enum class CompressFormat { JPEG, PNG, WEBP }

    /**
     * Re-encode. The phone steps quality down until the bytes fit a record;
     * ImageIO's JPEG writer takes the same 0..100 scale, so the loop above
     * behaves the same on both clients.
     */
    fun compress(format: CompressFormat, quality: Int, out: java.io.OutputStream): Boolean {
        val img = image ?: return false
        if (format != CompressFormat.JPEG) {
            return javax.imageio.ImageIO.write(img, format.name.lowercase(), out)
        }
        val writer = javax.imageio.ImageIO.getImageWritersByFormatName("jpeg").next()
        val params = writer.defaultWriteParam.apply {
            compressionMode = javax.imageio.ImageWriteParam.MODE_EXPLICIT
            compressionQuality = quality.coerceIn(0, 100) / 100f
        }
        javax.imageio.ImageIO.createImageOutputStream(out).use { ios ->
            writer.output = ios
            // JPEG has no alpha; flatten onto white as the phone's encoder does.
            val rgb = java.awt.image.BufferedImage(
                img.width, img.height, java.awt.image.BufferedImage.TYPE_INT_RGB,
            )
            rgb.createGraphics().apply {
                color = java.awt.Color.WHITE
                fillRect(0, 0, img.width, img.height)
                drawImage(img, 0, 0, null)
                dispose()
            }
            writer.write(null, javax.imageio.IIOImage(rgb, null, null), params)
        }
        writer.dispose()
        return true
    }

    companion object {
        @JvmStatic
        fun createBitmap(w: Int, h: Int, config: Config): Bitmap = Bitmap(w, h)

        /** The crop the avatar editor takes before scaling: a square middle. */
        @JvmStatic
        fun createBitmap(src: Bitmap, x: Int, y: Int, w: Int, h: Int): Bitmap {
            val img = src.image ?: return Bitmap(w, h)
            return Bitmap(w, h, img.getSubimage(x, y, w, h))
        }

        @JvmStatic
        fun createScaledBitmap(src: Bitmap, w: Int, h: Int, filter: Boolean): Bitmap {
            val img = src.image ?: return Bitmap(w, h)
            val out = java.awt.image.BufferedImage(
                w, h, java.awt.image.BufferedImage.TYPE_INT_ARGB,
            )
            out.createGraphics().apply {
                if (filter) {
                    setRenderingHint(
                        java.awt.RenderingHints.KEY_INTERPOLATION,
                        java.awt.RenderingHints.VALUE_INTERPOLATION_BILINEAR,
                    )
                }
                drawImage(img, 0, 0, w, h, null)
                dispose()
            }
            return Bitmap(w, h, out)
        }
    }
}

/**
 * Contact pictures arrive as sealed bytes (§16.15) and are decoded on both
 * clients from the same call site; ImageIO reads what the phone's decoder
 * reads. A picture that will not decode returns null exactly as Android's
 * does, and the caller already draws initials in that case.
 */
object BitmapFactory {
    /** The phone's bounded-decode knobs.
     *
     *  `inJustDecodeBounds` is honoured: ImageIO can read a header without
     *  building the raster, which is the whole point of the two-pass decode
     *  SafeImage does. `inSampleSize` is recorded and ignored — the desk has
     *  memory the phone does not, and nothing here draws a chat bubble. */
    class Options {
        @JvmField var inSampleSize: Int = 1
        @JvmField var inJustDecodeBounds: Boolean = false
        @JvmField var outWidth: Int = 0
        @JvmField var outHeight: Int = 0
    }

    /** Read the size without building the raster, and say so in [opts]. */
    private fun bounds(stream: () -> java.io.InputStream?, opts: Options): Boolean =
        runCatching {
            javax.imageio.ImageIO.createImageInputStream(stream() ?: return false)
                .use { iis ->
                    val r = javax.imageio.ImageIO.getImageReaders(iis)
                    if (!r.hasNext()) return false
                    val reader = r.next()
                    reader.setInput(iis, true, true)
                    opts.outWidth = reader.getWidth(0)
                    opts.outHeight = reader.getHeight(0)
                    reader.dispose()
                }
            true
        }.getOrDefault(false)

    private fun decode(stream: () -> java.io.InputStream?, opts: Options?): Bitmap? {
        if (opts?.inJustDecodeBounds == true) {
            if (!bounds(stream, opts)) { opts.outWidth = -1; opts.outHeight = -1 }
            return null
        }
        val img = javax.imageio.ImageIO.read(stream() ?: return null) ?: return null
        return Bitmap(img.width, img.height, img)
    }

    @JvmStatic
    fun decodeFile(path: String, opts: Options? = null): Bitmap? =
        decode({ runCatching { java.io.FileInputStream(path) }.getOrNull() }, opts)

    @JvmStatic
    fun decodeStream(input: java.io.InputStream?): Bitmap? = decode({ input }, null)

    @JvmStatic
    fun decodeStream(
        input: java.io.InputStream?,
        outPadding: Any?,
        opts: Options?,
    ): Bitmap? = decode({ input }, opts)

    @JvmStatic
    fun decodeByteArray(data: ByteArray?, offset: Int, length: Int): Bitmap? =
        decodeByteArray(data, offset, length, null)

    @JvmStatic
    fun decodeByteArray(
        data: ByteArray?,
        offset: Int,
        length: Int,
        opts: Options?,
    ): Bitmap? {
        if (data == null || length <= 0) return null
        return decode({ java.io.ByteArrayInputStream(data, offset, length) }, opts)
    }
}
