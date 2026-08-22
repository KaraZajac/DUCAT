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
