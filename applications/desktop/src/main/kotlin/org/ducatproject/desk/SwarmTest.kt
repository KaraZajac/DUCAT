package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Swarm
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The swarm through the CLIENT'S own code path: Kotlin → uniffi →
 * mobile::swarm → vendored engine → live Veilid. The Rust example proved
 * the engine; this proves the face the apps will actually call.
 *
 * Two roles, driven by env (JavaExec and shell quoting are bad friends):
 *
 *   DUCAT_SWARM_ROLE=seed  DUCAT_SWARM_STATE=<dir> DUCAT_SWARM_FILE=<file> \
 *     ./gradlew :desktop:swarmtest
 *   DUCAT_SWARM_ROLE=fetch DUCAT_SWARM_STATE=<dir> DUCAT_SWARM_KEY=<key> \
 *     DUCAT_SWARM_DIGEST=<hex> DUCAT_SWARM_OUT=<dir> ./gradlew :desktop:swarmtest
 *
 * The seeder prints SWARMTEST_SHARE and serves until killed; the fetcher
 * prints SWARMTEST_OK <bytes> <secs> and exits — progress ticks ride
 * stderr from the same poll the phone's screen will use.
 */
fun main() {
    val role = System.getenv("DUCAT_SWARM_ROLE") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_ROLE")
    val state = System.getenv("DUCAT_SWARM_STATE") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_STATE")
    File(state).mkdirs()

    nodeStart("$state/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "SWARMTEST_FAIL node never became ready" }

    when (role) {
        "seed" -> {
            val file = System.getenv("DUCAT_SWARM_FILE") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_FILE")
            val share = Swarm.seed(file)
            println("SWARMTEST_SHARE ${share.shareKey} ${share.indexDigestHex}")
            System.out.flush()
            while (true) Thread.sleep(5_000)
        }
        "fetch" -> {
            val key = System.getenv("DUCAT_SWARM_KEY") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_KEY")
            val digest = System.getenv("DUCAT_SWARM_DIGEST") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_DIGEST")
            val out = System.getenv("DUCAT_SWARM_OUT") ?: error("SWARMTEST_FAIL set DUCAT_SWARM_OUT")
            File(out).mkdirs()
            // The screen's poll, exercised here so the desk test covers it:
            // a ticker thread reads the same progress the phone will render.
            val ticker = Thread {
                while (!Thread.currentThread().isInterrupted) {
                    val p = Swarm.fetchProgress(key)
                    if (p.length > 0) System.err.println("progress ${p.position}/${p.length}")
                    if (p.done) break
                    try { Thread.sleep(2_000) } catch (_: InterruptedException) { break }
                }
            }.apply { isDaemon = true; start() }
            // DUCAT_SWARM_KEEP makes this a mirror rather than a reader:
            // staySeeding leaves the share serving, and the process stays
            // up, which is the only way to test the promise that matters —
            // that a bundle outlives its origin. Without it the fetcher
            // exits and takes its pieces with it.
            val keep = System.getenv("DUCAT_SWARM_KEEP") != null
            val t0 = System.currentTimeMillis()
            val bytes = Swarm.fetch(key, digest, out, staySeeding = keep)
            val secs = (System.currentTimeMillis() - t0) / 1000.0
            ticker.interrupt()
            println("SWARMTEST_OK $bytes $secs")
            System.out.flush()
            if (keep) {
                println("SWARMTEST_MIRRORING serving until killed")
                System.out.flush()
                while (true) Thread.sleep(5_000)
            }
        }
        else -> error("SWARMTEST_FAIL unknown role $role")
    }
}
