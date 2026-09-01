package org.ducatproject.desk

import java.io.File
import java.security.MessageDigest
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * Multi-file swarm round trip over the live network: seed a directory (a
 * tiny website, fittingly), fetch it into a fresh root, compare hashes.
 *
 *   DUCAT_SW_ROLE=seed  ./gradlew :desktop:swarmdir   # prints SWARMDIR_SHARE
 *   DUCAT_SW_ROLE=fetch DUCAT_SW_SHARE=<key> DUCAT_SW_DIGEST=<hex> ...
 */

private fun sha(f: File): String =
    MessageDigest.getInstance("SHA-256").digest(f.readBytes())
        .joinToString("") { "%02x".format(it) }

private fun up(dir: File) {
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "SWARMDIR_FAIL node never became ready" }
    System.err.println("node ready")
}

fun main() {
    val role = System.getenv("DUCAT_SW_ROLE") ?: error("set DUCAT_SW_ROLE")
    val state = kotlin.io.path.createTempDirectory("ducat-swdir-$role").toFile()
    up(state)
    when (role) {
        "seed" -> {
            val site = File(state, "site").apply { mkdirs() }
            File(site, "index.html").writeText(
                "<html><body><h1>hello from the swarm</h1></body></html>",
            )
            File(site, "style.css").writeText("h1 { color: rebeccapurple; }")
            File(site, "assets").mkdirs()
            File(site, "assets/big.bin").writeBytes(ByteArray(3 * 1024 * 1024) { (it % 251).toByte() })
            site.walkTopDown().filter { it.isFile }.sortedBy { it.path }.forEach {
                println("SWARMDIR_FILE ${it.relativeTo(site)} ${sha(it)}")
            }
            val share = org.ducatproject.ducat.Swarm.seed(site.absolutePath)
            println("SWARMDIR_SHARE ${share.shareKey} ${share.indexDigestHex}")
            System.out.flush()
            Thread.sleep(15 * 60_000)
        }
        "fetch" -> {
            val share = System.getenv("DUCAT_SW_SHARE") ?: error("set DUCAT_SW_SHARE")
            val digest = System.getenv("DUCAT_SW_DIGEST") ?: error("set DUCAT_SW_DIGEST")
            val root = File(state, "fetched").apply { mkdirs() }
            val stay = System.getenv("DUCAT_SW_STAY") == "1"
            val started = System.currentTimeMillis()
            val bytes = org.ducatproject.ducat.Swarm.fetch(
                share, digest, root.absolutePath, staySeeding = stay,
            )
            val ms = System.currentTimeMillis() - started
            root.walkTopDown().filter { it.isFile }.sortedBy { it.path }.forEach {
                println("SWARMDIR_GOT ${it.relativeTo(root)} ${sha(it)}")
            }
            println("SWARMDIR_OK bytes=$bytes ms=$ms stay=$stay")
            System.out.flush()
            if (stay) {
                // The mirror stands: stay up so a third node can fetch from
                // us after the original seeder is gone.
                Thread.sleep(15 * 60_000)
            }
        }
        else -> error("unknown role $role")
    }
}
