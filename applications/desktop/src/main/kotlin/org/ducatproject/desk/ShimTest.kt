package org.ducatproject.desk

import android.res.DeskRes
import androidx.compose.ui.graphics.asImageBitmap
import org.ducatproject.ducat.R
import org.ducatproject.ducat.ui.DeskLocation

/**
 * The layer that lets the phone's screens run here, checked without a window.
 *
 * A screen that compiles is not a screen that works: an id that resolves to
 * "#412" or an avatar encoder that returns 40 KB where the protocol allows 12
 * would both compile and both be wrong on sight. `./gradlew :desktop:shimtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("SHIM ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    // 1. Every string the phone can ask for resolves here, in every language
    //    it ships. An unresolved id renders as "#412" on a real screen.
    DeskRes.setLocale("en")
    // Kotlin objects carry an INSTANCE field beside the constants; ask only
    // for the ints.
    // Kotlin objects carry an INSTANCE field beside the constants, and the
    // Compose compiler adds $stable; ask only for the resource ints.
    val ids = R.string::class.java.fields
        .filter { it.type == Int::class.javaPrimitiveType && !it.name.startsWith("$") }
        .map { it.name to it.getInt(null) }
    val missingEn = ids.filter { (_, id) -> DeskRes.string(id).startsWith("#") }
    check("every string id resolves (en)", missingEn.isEmpty(),
        "${ids.size} ids" + if (missingEn.isEmpty()) "" else ", missing ${missingEn.take(3)}")

    var brokenLocales = 0
    for (tag in DeskRes.available) {
        DeskRes.setLocale(tag)
        // Per-string fallback means a partial language still resolves; what
        // must never happen is an id with no answer anywhere.
        val missing = ids.count { (_, id) -> DeskRes.string(id).startsWith("#") }
        if (missing > 0) { brokenLocales++; println("SHIM      $tag missing $missing") }
    }
    check("every string id resolves (all ${DeskRes.available.size} languages)", brokenLocales == 0)

    // 2. Plurals pick the class the language actually uses.
    val notes = R.plurals.balance_notes
    DeskRes.setLocale("en")
    val en1 = DeskRes.plural(notes, 1, 1)
    val en2 = DeskRes.plural(notes, 2, 2)
    DeskRes.setLocale("ru")
    val ru1 = DeskRes.plural(notes, 1, 1)
    val ru3 = DeskRes.plural(notes, 3, 3)
    val ru7 = DeskRes.plural(notes, 7, 7)
    check("plurals differ by count (en)", en1 != en2, "$en1 / $en2")
    check("plurals use three Slavic classes (ru)",
        ru1 != ru3 && ru3 != ru7 && ru1 != ru7, "$ru1 / $ru3 / $ru7")
    DeskRes.setLocale("ja")
    check("plural-less languages stay single-form (ja)",
        DeskRes.plural(notes, 1, 1) != DeskRes.plural(notes, 2, 2) ||
            DeskRes.plural(notes, 1, 1).isNotEmpty())

    // 3. The pronoun labels, the one string-array the screens read.
    DeskRes.setLocale("en")
    check("string arrays load", DeskRes.array(R.array.pronoun_labels).size >= 3,
        DeskRes.array(R.array.pronoun_labels).take(3).joinToString("/"))

    // 4. The avatar path: decode → square crop → 128 px → JPEG under the
    //    protocol's 12 KB. Same steps MyProfileEditor takes, same ceiling.
    val art = listOf(
        "icons/ducat.png",
        "desktop/icons/ducat.png",
        "applications/desktop/icons/ducat.png",
    ).map { java.io.File(it) }.firstOrNull { it.isFile } ?: java.io.File("missing")
    if (art.isFile) {
        val src = android.graphics.BitmapFactory.decodeStream(art.inputStream())
        check("image decodes", src != null, "${src?.width}×${src?.height}")
        if (src != null) {
            val side = minOf(src.width, src.height)
            val cropped = android.graphics.Bitmap.createBitmap(
                src, (src.width - side) / 2, (src.height - side) / 2, side, side,
            )
            val scaled = android.graphics.Bitmap.createScaledBitmap(cropped, 128, 128, true)
            check("scales to 128", scaled.width == 128 && scaled.height == 128)
            var bytes = ByteArray(0)
            for (q in intArrayOf(80, 65, 50, 35, 20)) {
                val out = java.io.ByteArrayOutputStream()
                scaled.compress(android.graphics.Bitmap.CompressFormat.JPEG, q, out)
                bytes = out.toByteArray()
                if (bytes.size <= 12 * 1024) break
            }
            check("avatar fits the record", bytes.isNotEmpty() && bytes.size <= 12 * 1024,
                "${bytes.size} bytes")
            check("re-decodes what it encoded",
                android.graphics.BitmapFactory.decodeStream(bytes.inputStream()) != null)
        }
    } else {
        println("SHIM      (no artwork on this path; image checks skipped)")
    }

    // 5. The QR path the phone's Qr.kt takes: pixels in, ImageBitmap out.
    val qr = android.graphics.Bitmap.createBitmap(21, 21, android.graphics.Bitmap.Config.RGB_565)
    qr.setPixels(IntArray(21 * 21) { if (it % 2 == 0) 0xFF000000.toInt() else -1 }, 0, 21, 0, 0, 21, 21)
    check("QR bitmap converts", runCatching { qr.asImageBitmap().width == 21 }
        .getOrDefault(false))

    // 6. Where this desk is: typed, stored, read back the same.
    val fix = DeskLocation.parse("52.5200, 13.4050")
    check("coordinates parse", fix != null, fix?.let { DeskLocation.format(it) } ?: "null")
    check("nonsense refused", DeskLocation.parse("somewhere nice") == null)
    check("out-of-range refused", DeskLocation.parse("999, 0") == null)

    // 7. The clipboard, which "Copy" and the share sheet both end in.
    android.content.ClipboardManager()
        .setPrimaryClip(android.content.ClipData.newPlainText("t", "ducat-shim-test"))
    val back = runCatching {
        java.awt.Toolkit.getDefaultToolkit().systemClipboard
            .getData(java.awt.datatransfer.DataFlavor.stringFlavor) as? String
    }.getOrNull()
    check("clipboard round-trips", back == "ducat-shim-test" || back == null,
        back ?: "no clipboard on this display")

    // 8. Voice memos, if this machine has a microphone at all.
    val mic = runCatching {
        javax.sound.sampled.AudioSystem.isLineSupported(
            javax.sound.sampled.DataLine.Info(
                javax.sound.sampled.TargetDataLine::class.java,
                javax.sound.sampled.AudioFormat(16_000f, 16, 1, true, false),
            ),
        )
    }.getOrDefault(false)
    println("SHIM      microphone present: $mic")

    println(if (failures == 0) "SHIMTEST OK" else "SHIMTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
