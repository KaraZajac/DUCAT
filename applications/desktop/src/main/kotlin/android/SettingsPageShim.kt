// The name of the system settings page for one app. On a phone this is where
// a permission refused for good can still be granted; on a desk it is a
// string that never reaches an intent anybody handles.

package android.provider

object Settings {
    @JvmField
    val ACTION_APPLICATION_DETAILS_SETTINGS: String =
        "android.settings.APPLICATION_DETAILS_SETTINGS"
}
