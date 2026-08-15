// The log banner's last two acquaintances.

package android.os

object Build {
    @JvmField val MODEL: String = "desk/${System.getProperty("os.name")}"

    object VERSION {
        @JvmField val RELEASE: String = System.getProperty("os.version") ?: "?"
        @JvmField val SDK_INT: Int = 0
    }
}
