package android.content.pm

class PackageInfo {
    @JvmField var versionName: String? = "desk"
}

class PackageManager {
    companion object {
        @JvmField val PERMISSION_GRANTED: Int = 0
        @JvmField val PERMISSION_DENIED: Int = -1
    }

    fun getPackageInfo(pkg: String, flags: Int): PackageInfo = PackageInfo()
}
