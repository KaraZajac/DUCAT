package org.ducatproject.desk

import org.ducatproject.ducat.SafeImage

/**
 * How much of a picture to throw away before decoding it.
 * `./gradlew :desktop:safeimage`.
 *
 * The protocol bounds the compressed bytes, which is the wrong quantity: PNG
 * compresses flat colour to almost nothing, so 416 KiB of entirely legal image
 * decodes to 20000×20000 — 400 megapixels, 1.6 GB at four bytes each. And the
 * decode sits behind `remember(...)`, so it happens again every time the
 * conversation is opened. One message, and that chat is over.
 *
 * The decoder is Android's, so what is checked here is the arithmetic that
 * tells it how much to skip. Getting that wrong is silent in both directions.
 */
fun main() {
    val f = SafeImage::sampleFor

    // The bomb. 400 Mpx against a 4 Mpx budget wants to shed a factor of 100
    // in area, which is between 8 and 16 linearly — and a sample size is
    // rounded down to a power of two by the decoder, so it has to be 16.
    val bomb = f(20000, 20000, SafeImage.MESSAGE_PIXELS)
    check(bomb == 16) { "SAFEIMG_FAIL 20000² wanted 16, got $bomb" }
    check((20000L / bomb) * (20000L / bomb) <= SafeImage.MESSAGE_PIXELS) {
        "SAFEIMG_FAIL 20000² still over budget at sample $bomb"
    }

    // The one that would quietly undo all of it: 20000 × 20000 overflows a
    // signed Int to -1727379968, which compares as comfortably under any
    // budget, so an Int multiplication returns 1 and decodes the bomb at full
    // size. This is the whole reason the arithmetic is in Long.
    check(f(46341, 46341, SafeImage.MESSAGE_PIXELS) > 1) {
        "SAFEIMG_FAIL a picture past the Int overflow point was let through whole"
    }
    check(f(65535, 65535, SafeImage.AVATAR_PIXELS) > 1) {
        "SAFEIMG_FAIL 65535² was let through whole"
    }

    // Everything that fits is left alone. Sampling a small picture would cost
    // detail on the overwhelmingly common path for no reason at all.
    for ((w, h) in listOf(64 to 64, 512 to 512, 1024 to 1024, 2048 to 1024)) {
        check(f(w, h, SafeImage.MESSAGE_PIXELS) == 1) {
            "SAFEIMG_FAIL ${w}×$h fits the budget but was sampled"
        }
    }

    // Long and thin: a panorama has few pixels for its width, and area is
    // what runs out, so it must not be sampled on width alone.
    check(f(8000, 200, SafeImage.MESSAGE_PIXELS) == 1) {
        "SAFEIMG_FAIL a panorama well under budget was sampled"
    }

    // Avatars get a tighter budget, and the worst that fits the 12 KiB wire
    // cap is about 2800² — which must come down.
    check(f(2800, 2800, SafeImage.AVATAR_PIXELS) > 1) {
        "SAFEIMG_FAIL the worst avatar that fits the wire cap was not sampled"
    }

    // A header that would not parse leaves the dimensions at -1. There is no
    // ratio to compute and the decode is about to fail anyway; inventing one
    // from nonsense is how you get a divide by zero instead of a null.
    check(f(-1, -1, SafeImage.MESSAGE_PIXELS) == 1) { "SAFEIMG_FAIL unreadable header" }
    check(f(0, 0, SafeImage.MESSAGE_PIXELS) == 1) { "SAFEIMG_FAIL zero dimensions" }
    check(f(1000, 1000, 0) == 1) { "SAFEIMG_FAIL a zero budget must not spin" }

    // Whatever is asked for, the answer is a power of two — the decoder
    // rounds down, so anything else decodes larger than the budget allows.
    for (d in listOf(3000, 7000, 12345, 30000)) {
        val s = f(d, d, SafeImage.AVATAR_PIXELS)
        check(s and (s - 1) == 0) { "SAFEIMG_FAIL sample $s for ${d}² is not a power of two" }
        check((d.toLong() / s) * (d.toLong() / s) <= SafeImage.AVATAR_PIXELS) {
            "SAFEIMG_FAIL ${d}² still over budget at sample $s"
        }
    }

    // The two-pass sequence itself. Probing for the size means setting
    // inJustDecodeBounds, and the standard way to get this wrong is to reuse
    // that Options for the real decode — every picture in the app then comes
    // back null and silently disappears, with nothing in the log. So: a real
    // PNG through the real call path, and it has to come back.
    val png = java.io.ByteArrayOutputStream().also { out ->
        val img = java.awt.image.BufferedImage(
            640, 480, java.awt.image.BufferedImage.TYPE_INT_RGB,
        )
        img.createGraphics().apply {
            color = java.awt.Color(80, 120, 200)
            fillRect(0, 0, 640, 480)
            dispose()
        }
        javax.imageio.ImageIO.write(img, "png", out)
    }.toByteArray()

    val bmp = SafeImage.fromBytes(png, SafeImage.MESSAGE_PIXELS)
    check(bmp != null) {
        "SAFEIMG_FAIL an ordinary picture decoded to nothing — the bounds probe " +
            "leaked into the real decode"
    }
    check(bmp!!.width == 640 && bmp.height == 480) {
        "SAFEIMG_FAIL decoded ${bmp.width}×${bmp.height}, expected 640×480"
    }

    // And the probe reads a real header rather than guessing. Same picture,
    // a budget it cannot fit, so the arithmetic has to have seen 640×480.
    check(SafeImage.sampleFor(640, 480, 4096) > 1) {
        "SAFEIMG_FAIL a tiny budget did not force sampling"
    }

    // Bytes that are not a picture at all: null, not a throw, and not a hang.
    check(SafeImage.fromBytes(ByteArray(64) { 0x7F }, SafeImage.AVATAR_PIXELS) == null) {
        "SAFEIMG_FAIL nonsense bytes did not decode to null"
    }
    check(SafeImage.fromBytes(ByteArray(0), SafeImage.AVATAR_PIXELS) == null) {
        "SAFEIMG_FAIL empty bytes did not decode to null"
    }

    println(
        "SAFEIMG_OK bomb=16 overflow=caught small=untouched panorama=untouched " +
            "pow2=ok roundtrip=640x480 garbage=null",
    )
}
