package org.ducatproject.desk

import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.PersonaStore
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeStop

/**
 * Ring a phone: send one real chat message from the desk's standing identity
 * over the live network, so a backgrounded phone's DHT watch has something to
 * ring about. Built to verify the poller's battery tier — a quiet pocket
 * sweeps on a heartbeat, a message must still arrive within a wait chunk.
 *
 * Uses the same state directory as the desk UI (do not run both at once; the
 * desk.lock is respected). `./gradlew :desktop:ringtest`, optionally with
 * DUCAT_RING_TO=<persona hex prefix> to pick the contact when there are
 * several; without it, exactly one contact must exist.
 */
fun main() {
    val base = System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotEmpty() }
        ?: "${System.getProperty("user.home")}/.local/share"
    val dir = File(System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() } ?: "$base/ducat-desk")
    check(dir.isDirectory) { "RINGTEST_FAIL no desk state at $dir" }
    val lock = java.io.RandomAccessFile(File(dir, "desk.lock"), "rw").channel.tryLock()
    check(lock != null) { "RINGTEST_FAIL the desk UI is running on $dir — close it first" }

    Unlock.orExit(dir)

    val context = DeskContext(dir)
    val contacts = ContactStore(context).all()
    println("ringtest: ${contacts.size} contact(s): " +
        contacts.joinToString { "${it.displayName()}=${it.personaHex.take(8)}" })
    val want = System.getenv("DUCAT_RING_TO")
    val to = if (want.isNullOrEmpty()) contacts.singleOrNull()
        ?: error("RINGTEST_FAIL ${contacts.size} contacts — set DUCAT_RING_TO=<hex prefix>")
    else contacts.firstOrNull { it.personaHex.startsWith(want) }
        ?: error("RINGTEST_FAIL no contact matches $want")

    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < deadline) {
        val s = nodeStatus()
        if (s.publicInternetReady) break
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "RINGTEST_FAIL node never became ready" }

    val stamp = System.currentTimeMillis()
    Mailbox.send(context, to, "ring $stamp")
    println("RINGTEST_OK sent \"ring $stamp\" to ${to.displayName()} (${to.personaHex.take(8)}…)")
    nodeStop()
}
