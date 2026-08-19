// A Uri that is a file, and nothing more: everything the screens do with
// content on a desk ends at a path.

package android.net

class Uri private constructor(private val s: String) {
    val path: String? get() = s.removePrefix("file://").takeIf { it.isNotEmpty() }
    val lastPathSegment: String? get() = path?.substringAfterLast('/')
    fun toFile(): java.io.File? = path?.let { java.io.File(it) }
    override fun toString(): String = s

    companion object {
        @JvmStatic
        fun parse(s: String): Uri = Uri(s)

        @JvmStatic
        fun fromFile(f: java.io.File): Uri = Uri("file://${f.absolutePath}")

        /** `package:org.ducatproject.ducat` and its kind — a scheme and an
         *  opaque remainder, which is not a path and never becomes a file. */
        @JvmStatic
        fun fromParts(scheme: String, ssp: String, fragment: String?): Uri =
            Uri("$scheme:$ssp" + (fragment?.let { "#$it" } ?: ""))
    }
}
