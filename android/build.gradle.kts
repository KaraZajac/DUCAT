plugins {
    id("com.android.application") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    // Required from Kotlin 2.0: the Compose compiler moved out of the Kotlin
    // plugin and into its own, versioned in lockstep with Kotlin.
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
