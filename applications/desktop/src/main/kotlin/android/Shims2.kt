// The rest of the desk's Android: Base64, Log, Build, PackageManager.
// See Shims.kt for why these names exist at all.

package android.util

object Base64 {
    @JvmField val NO_WRAP: Int = 2

    fun encodeToString(bytes: ByteArray?, flags: Int): String =
        java.util.Base64.getEncoder().encodeToString(bytes ?: ByteArray(0))

    fun decode(s: String, flags: Int): ByteArray =
        java.util.Base64.getDecoder().decode(s)
}

object Log {
    fun i(tag: String, msg: String): Int { println("I/$tag: $msg"); return 0 }
    fun w(tag: String, msg: String): Int { println("W/$tag: $msg"); return 0 }
    fun e(tag: String, msg: String): Int { System.err.println("E/$tag: $msg"); return 0 }
}
