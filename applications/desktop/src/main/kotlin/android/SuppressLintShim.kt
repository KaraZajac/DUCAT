// The other annotation with no behaviour: it addresses Android's lint, which
// the desk does not run, but the phone's sources carry it and both compile
// from the same files. A separate file because Kotlin allows one package
// declaration each and AnnotationShims.kt is androidx.annotation.

package android.annotation

@Retention(AnnotationRetention.SOURCE)
annotation class SuppressLint(vararg val value: String)
