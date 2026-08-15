package org.ducatproject.desk

import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeStop
import uniffi.ducat_mobile.personaPublicHex

/**
 * The stack, stood up with no window: JVM → JNA → Rust bridge → Veilid.
 * What CI (and a headless desk) can run; the window is only paint.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-desk-smoke").toFile()
    println("smoke: starting node in ${dir.absolutePath}")
    nodeStart("${dir.absolutePath}/veilid")
    val persona = personaPublicHex(createPersonaSecret())
    println("smoke: persona $persona")
    val deadline = System.currentTimeMillis() + 60_000
    while (System.currentTimeMillis() < deadline) {
        val s = nodeStatus()
        println("smoke: running=${s.running} attached=${s.attached} " +
            "ready=${s.publicInternetReady} peers=${s.reliablePeers}/${s.peers}")
        if (s.publicInternetReady) {
            println("smoke: OK — the desk reaches the network")
            nodeStop()
            dir.deleteRecursively()
            return
        }
        Thread.sleep(3_000)
    }
    nodeStop()
    dir.deleteRecursively()
    error("node never became ready")
}
