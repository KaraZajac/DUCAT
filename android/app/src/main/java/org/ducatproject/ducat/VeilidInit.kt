package org.ducatproject.ducat

import android.content.Context

/**
 * Hands Veilid the JavaVM and the application `Context`.
 *
 * **The class name is part of an ABI.** The native symbol is
 * `Java_org_ducatproject_ducat_VeilidInit_setupAndroid`, so renaming or moving
 * this class breaks the link at *runtime* with `UnsatisfiedLinkError`, not at
 * compile time.
 *
 * Without this, node startup fails with `Internal: Android globals are not set
 * up` — which names the cause exactly and still reads, from a Kotlin stack, like
 * the library is broken rather than uninitialised.
 */
object VeilidInit {
    @Volatile private var done = false

    external fun setupAndroid(ctx: Context)

    /**
     * Idempotent, and takes the **application** context deliberately: veilid-core
     * holds a global reference for the process lifetime, and holding an Activity
     * that way leaks it across every rotation.
     */
    @Synchronized
    fun ensure(context: Context) {
        if (done) return
        System.loadLibrary("ducat_mobile")
        setupAndroid(context.applicationContext)
        done = true
    }
}
