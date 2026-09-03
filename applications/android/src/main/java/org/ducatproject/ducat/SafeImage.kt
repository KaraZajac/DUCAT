package org.ducatproject.ducat

import android.graphics.Bitmap
import android.graphics.BitmapFactory

/**
 * Decoding a picture somebody else chose.
 *
 * The protocol bounds what arrives — 12 KiB for an avatar, just under a
 * mebibyte for an attachment — and those are bounds on the *compressed* bytes,
 * which is the wrong quantity for the thing that runs out. PNG compresses flat
 * colour to almost nothing, so 416 KiB of legitimate PNG decodes to a
 * 20000×20000 bitmap: 400 megapixels, 1.6 GB at four bytes each, on a phone
 * that has none of it. Even the avatar cap buys about 31 MB, and the chat list
 * decodes every contact's at once.
 *
 * The failure is worse than a crash, because the decode sits behind
 * `remember(...)`: the picture is re-decoded every time that conversation is
 * opened, so one message from one contact ends the conversation permanently and
 * takes the app with it each time somebody tries.
 *
 * So the pixels get a ceiling of their own. `inJustDecodeBounds` reads the
 * header without allocating anything, and `inSampleSize` makes the decoder
 * throw away rows and columns on the way out — the bitmap is never built at
 * full size, which is the point. A picture too big to show is shown smaller,
 * not refused: the honest version of a large image is somebody's camera.
 */
object SafeImage {

    /** Room for an avatar. Drawn at 40dp, so this is already generous. */
    const val AVATAR_PIXELS = 1 shl 20

    /** Room for a picture in a chat bubble, drawn at most 280dp wide. */
    const val MESSAGE_PIXELS = 4 shl 20

    /** Room for a picture the user is about to send, before it is scaled. */
    const val COMPOSE_PIXELS = 16 shl 20

    fun fromBytes(bytes: ByteArray, maxPixels: Int): Bitmap? = runCatching {
        val probe = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, probe)
        val opts = BitmapFactory.Options().apply {
            inSampleSize = sampleFor(probe.outWidth, probe.outHeight, maxPixels)
        }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
    }.getOrNull()

    fun fromFile(path: String, maxPixels: Int): Bitmap? = runCatching {
        val probe = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(path, probe)
        val opts = BitmapFactory.Options().apply {
            inSampleSize = sampleFor(probe.outWidth, probe.outHeight, maxPixels)
        }
        BitmapFactory.decodeFile(path, opts)
    }.getOrNull()

    fun fromStream(open: () -> java.io.InputStream?, maxPixels: Int): Bitmap? = runCatching {
        // Twice, because a sampled decode needs the size first and a stream
        // only goes forwards. Callers hand over a way to open it, not an open
        // one, so the second pass starts from the beginning.
        val probe = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        open().use { BitmapFactory.decodeStream(it, null, probe) }
        val opts = BitmapFactory.Options().apply {
            inSampleSize = sampleFor(probe.outWidth, probe.outHeight, maxPixels)
        }
        open().use { BitmapFactory.decodeStream(it, null, opts) }
    }.getOrNull()

    /**
     * The picture, and nothing else that was in the file.
     *
     * A phone writes GPS into every photo by default, accurate to a few
     * metres, and almost nothing on the way out strips it: police forces
     * have issued warnings about exactly this on classified-ad sites,
     * where a photograph of a sofa hands over the address of the sofa.
     * For this app it is worse than for most, because §16.18 spends its
     * whole design on *not* knowing where anybody is — a listing is placed
     * on a board at precision 5, about five kilometres — and one un-stripped
     * photo replaces that with a doorstep. The board never carries a
     * picture (§16.18 forbids it), but a thread does, and a thread is where
     * a stranger who answered an advertisement is standing.
     *
     * Stripping is a side effect of honesty rather than a filter: the
     * bytes are decoded to pixels and encoded again, so nothing but pixels
     * can survive. No allow-list of tags to keep in step with, and no way
     * for a maker's private extension to ride through one.
     *
     * **The orientation is read first and baked into the pixels**, which
     * is the part that is easy to get wrong: a phone camera writes
     * upright pixels plus a "turn this" tag, so dropping the tag without
     * turning the pixels is how stripping metadata famously lands
     * everybody's holiday photos on their side.
     *
     * Only formats we can re-encode faithfully. Anything else — a PDF, an
     * unknown type — returns null, and the caller sends what it was given
     * rather than a silently mangled copy.
     */
    fun stripped(
        open: () -> java.io.InputStream?,
        mime: String?,
        maxPixels: Int = COMPOSE_PIXELS,
    ): ByteArray? {
        val png = mime?.lowercase() == "image/png"
        if (!png && mime?.lowercase() != "image/jpeg") return null
        return runCatching {
            val fixed = upright(open, maxPixels) ?: return null
            val out = java.io.ByteArrayOutputStream()
            // PNG is lossless, so re-encoding costs nothing but time. JPEG
            // is not, and 92 is where a second generation stops being
            // visible — this is one re-encode, not a chain of them.
            val ok = if (png) {
                fixed.compress(Bitmap.CompressFormat.PNG, 100, out)
            } else {
                fixed.compress(Bitmap.CompressFormat.JPEG, 92, out)
            }
            if (ok) out.toByteArray() else null
        }.getOrNull()
    }

    /**
     * Decode the way the photographer held the camera.
     *
     * [fromStream] returns what the file stores, which for a phone photo
     * is the sensor's idea of up plus a tag saying how far round it
     * actually was. Anything taking a picture *in* — an avatar, an
     * attachment — wants this one, because the tag is about to be dropped
     * and after that the pixels are the only record of which way is up.
     * Display of bytes already stored can keep using fromStream: those
     * went through here on the way in.
     */
    fun upright(open: () -> java.io.InputStream?, maxPixels: Int): Bitmap? {
        val turn = orientationOf(open)
        val src = fromStream(open, maxPixels) ?: return null
        return turned(src, turn)
    }

    /** The EXIF turn, or 1 (upright) when there is none to read. */
    private fun orientationOf(open: () -> java.io.InputStream?): Int = runCatching {
        open().use { s ->
            s ?: return 1
            @Suppress("DEPRECATION")
            android.media.ExifInterface(s).getAttributeInt(
                android.media.ExifInterface.TAG_ORIENTATION,
                android.media.ExifInterface.ORIENTATION_NORMAL,
            )
        }
    }.getOrDefault(1)

    /**
     * The eight EXIF orientations, applied to the pixels.
     *
     * Four rotations and their mirrored twins. The mirrored ones are rare
     * — a front camera that recorded the flip rather than applying it —
     * but they are in the enumeration, and a picture that comes back
     * mirrored is worse than one that comes back sideways because nobody
     * notices until it is somebody's face.
     */
    private fun turned(src: Bitmap, orientation: Int): Bitmap {
        val m = android.graphics.Matrix()
        when (orientation) {
            android.media.ExifInterface.ORIENTATION_ROTATE_90 -> m.postRotate(90f)
            android.media.ExifInterface.ORIENTATION_ROTATE_180 -> m.postRotate(180f)
            android.media.ExifInterface.ORIENTATION_ROTATE_270 -> m.postRotate(270f)
            android.media.ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> m.postScale(-1f, 1f)
            android.media.ExifInterface.ORIENTATION_FLIP_VERTICAL -> m.postScale(1f, -1f)
            android.media.ExifInterface.ORIENTATION_TRANSPOSE -> {
                m.postRotate(90f); m.postScale(-1f, 1f)
            }
            android.media.ExifInterface.ORIENTATION_TRANSVERSE -> {
                m.postRotate(270f); m.postScale(-1f, 1f)
            }
            else -> return src
        }
        return runCatching {
            Bitmap.createBitmap(src, 0, 0, src.width, src.height, m, true)
        }.getOrDefault(src)
    }

    /**
     * How much of the picture to throw away on the way out.
     *
     * The decoder rounds a sample size down to a power of two, so this only
     * ever returns one — anything else would silently decode larger than asked.
     *
     * A header that could not be read leaves the dimensions at -1. Sampling
     * cannot help there and neither can refusing, since the decode is about to
     * fail anyway: return 1 and let it fail honestly rather than inventing a
     * ratio from nonsense.
     */
    internal fun sampleFor(width: Int, height: Int, maxPixels: Int): Int {
        if (width <= 0 || height <= 0 || maxPixels <= 0) return 1
        var sample = 1
        // Long, because the multiplication is exactly where the numbers are
        // large: 20000 × 20000 overflows a signed Int and comes out negative,
        // which compares as comfortably under any budget.
        while (
            (width.toLong() / sample) * (height.toLong() / sample) > maxPixels &&
            sample < (1 shl 20)
        ) {
            sample = sample shl 1
        }
        return sample
    }
}
