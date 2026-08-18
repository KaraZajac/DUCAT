package org.ducatproject.desk

import org.ducatproject.ducat.DeskVault
import java.io.File

/**
 * Lock an existing desk that was created before there was a lock.
 *
 *   DUCAT_DESK_STATE=~/ducat-arbiter DUCAT_DESK_PASSPHRASE='…' \
 *     ./gradlew :desktop:vaultset
 *
 * Encrypts every store in place — write, verify, then delete the plaintext —
 * and prints what it did. Afterwards this desk needs its passphrase to start,
 * including the headless tools, which take it from the same environment
 * variable used here.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("VAULTSET_FAIL set DUCAT_DESK_STATE"),
    )
    check(dir.isDirectory) { "VAULTSET_FAIL no desk at $dir" }
    val pass = Unlock.fromEnvironment()
        ?: error("VAULTSET_FAIL set DUCAT_DESK_PASSPHRASE")

    if (DeskVault.exists(dir)) {
        // Already locked: the useful thing left is to sweep up any store
        // written since (a new one starts life beside the sealed ones).
        DeskVault.unlock(dir, pass).getOrElse {
            println("VAULTSET_FAIL that passphrase does not open this desk")
            kotlin.system.exitProcess(1)
        }
        println("VAULTSET already locked; swept any plaintext left beside it")
    } else {
        DeskVault.create(dir, pass).getOrElse {
            println("VAULTSET_FAIL ${it.message}")
            kotlin.system.exitProcess(1)
        }
        println("VAULTSET locked $dir")
    }
    val sealed = File(dir, "prefs").listFiles()?.count { it.name.endsWith(".enc") } ?: 0
    val plain = File(dir, "prefs").listFiles()?.count { it.name.endsWith(".json") } ?: 0
    println("VAULTSET sealed=$sealed plaintext-left=$plain")
    if (plain > 0) kotlin.system.exitProcess(1)
}
