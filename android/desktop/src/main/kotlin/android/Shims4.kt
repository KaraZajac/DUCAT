package android.content.pm

class PackageInfo {
    @JvmField var versionName: String? = "desk"
}

class PackageManager {
    fun getPackageInfo(pkg: String, flags: Int): PackageInfo = PackageInfo()
}
