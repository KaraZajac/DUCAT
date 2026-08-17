// Android's resource annotations carry no behaviour — they tell lint that an
// Int is a resource id. Nothing here has lint, but the phone's signatures
// name them, so the names have to exist.

package androidx.annotation

@Retention(AnnotationRetention.SOURCE)
annotation class StringRes

@Retention(AnnotationRetention.SOURCE)
annotation class DrawableRes

@Retention(AnnotationRetention.SOURCE)
annotation class PluralsRes
