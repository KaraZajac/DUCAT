package org.ducatproject.desk

import org.ducatproject.ducat.DeskVault
import java.io.File

/**
 * How a desk gets its key, before anything reads a store.
 *
 * Order matters and so does what is *absent*: there is no key file, no
 * machine-derived secret, nothing on disk that opens the disk. Either a human
 * typed the passphrase into this process, or an operator put it in the
 * environment of a service they run deliberately.
 *
 * A desk with no vault at all still runs — plaintext, as before — because
 * refusing to start would strand every existing desk. The window says which
 * of the two it is rather than leaving it to be assumed.
 */
object Unlock {
    /** The passphrase a headless tool was given, if any. */
    fun fromEnvironment(): String? =
        System.getenv("DUCAT_DESK_PASSPHRASE")?.takeIf { it.isNotEmpty() }

    /**
     * Try to open the vault without asking anyone. Returns true when the desk
     * is ready to read its stores: either it opened, or there is nothing to
     * open. False means a vault exists and needs a passphrase this process
     * was not given.
     */
    fun tryQuiet(dir: File): Boolean {
        if (!DeskVault.exists(dir)) return true
        if (DeskVault.unlocked) return true
        val pass = fromEnvironment() ?: return false
        return DeskVault.unlock(dir, pass).isSuccess
    }

    /**
     * For headless tools: open or die with an explanation. A till that starts
     * with an empty wallet because its vault would not open is worse than one
     * that refuses to start.
     */
    fun orExit(dir: File) {
        if (tryQuiet(dir)) return
        System.err.println(
            "ducat-desk: this desk is locked.\n" +
                "Give it the passphrase: DUCAT_DESK_PASSPHRASE=… <command>",
        )
        kotlin.system.exitProcess(2)
    }
}
