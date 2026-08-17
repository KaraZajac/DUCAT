// Permission names, so the screens that ask for them compile. A desk grants
// what it has: the file dialog is the permission.

package android

object Manifest {
    object permission {
        const val CAMERA = "android.permission.CAMERA"
        const val POST_NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"
        const val ACCESS_FINE_LOCATION = "android.permission.ACCESS_FINE_LOCATION"
        const val ACCESS_COARSE_LOCATION = "android.permission.ACCESS_COARSE_LOCATION"
        const val READ_MEDIA_IMAGES = "android.permission.READ_MEDIA_IMAGES"
        const val READ_EXTERNAL_STORAGE = "android.permission.READ_EXTERNAL_STORAGE"
    }
}
