package org.ducatproject.desk

import java.io.File
import java.security.MessageDigest
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * §16.15's big road, end to end over the live network: a 5 MB file rides a
 * swarm share referenced from a sealed message; the receiver opens the
 * message, fetches the share, verifies the hash, unseals, and compares.
 *
 *   DUCAT_FS_ROLE=send  DUCAT_FS_STATE=<dir>                       # prints card, waits for claim, sends
 *   DUCAT_FS_ROLE=recv  DUCAT_FS_STATE=<dir> DUCAT_FS_CARD=<uri>   # claims, waits, fetches, verifies
 */

private fun up(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "FS_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

private fun sha(b: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(b).joinToString("") { "%02x".format(it) }

fun main() {
    val role = System.getenv("DUCAT_FS_ROLE") ?: error("set DUCAT_FS_ROLE")
    val dir = File(System.getenv("DUCAT_FS_STATE") ?: error("set DUCAT_FS_STATE"))
        .apply { mkdirs() }
    val context = up(dir)
    when (role) {
        "send" -> {
            NameStore(context).get() ?: NameStore(context).put("Sender")
            var peer = ContactStore(context).all().firstOrNull()
            if (peer == null) {
                val card = Mailbox.issueCard(context, "Sender", 60uL * 60uL)
                println("FS_CARD ${card.uri}")
                System.out.flush()
                val deadline = System.currentTimeMillis() + 600_000
                while (peer == null && System.currentTimeMillis() < deadline) {
                    runCatching { Mailbox.collectClaims(context) }
                    peer = ContactStore(context).all().firstOrNull()
                    Thread.sleep(2_000)
                }
            }
            val to = peer ?: error("FS_FAIL nobody claimed")

            // 5 MB of structured bytes: any corruption moves the hash.
            val payload = ByteArray(5 * 1024 * 1024) { ((it * 31) % 251).toByte() }
            println("FS_PLAINSHA ${sha(payload)}")

            val rng = java.security.SecureRandom()
            val key = ByteArray(32).also(rng::nextBytes)
            val nonce = ByteArray(24).also(rng::nextBytes)
            val ct = uniffi.ducat_mobile.attachmentSeal(key, nonce, payload)
            val ctHash = MessageDigest.getInstance("SHA-256").digest(ct)
            val out = File(dir, "swarm_out/blob").apply { mkdirs() }
            val blob = File(out, "payload.bin").apply { writeBytes(ct) }
            val share = org.ducatproject.ducat.Swarm.seed(blob.absolutePath)
            val ref = uniffi.ducat_mobile.AttachmentRef(
                recordKey = null,
                swarmKey = share.shareKey,
                swarmDigest = share.indexDigestHex.chunked(2)
                    .map { it.toInt(16).toByte() }.toByteArray(),
                key = key, nonce = nonce,
                len = payload.size.toULong(),
                ctHash = ctHash,
                mime = "application/octet-stream",
                name = "big-road.bin",
            )
            Mailbox.send(context, to, "📎 big-road.bin", attachment = ref)
            println("FS_SENT")
            System.out.flush()
            Thread.sleep(12 * 60_000) // keep serving while the receiver works
        }
        "recv" -> {
            NameStore(context).get() ?: NameStore(context).put("Receiver")
            val contact = ContactStore(context).all().firstOrNull()
                ?: Mailbox.claimCard(
                    context,
                    uniffi.ducat_mobile.readContactCard(
                        System.getenv("DUCAT_FS_CARD") ?: error("set DUCAT_FS_CARD"),
                    ),
                    "sender",
                )
            val deadline = System.currentTimeMillis() + 600_000
            var msg: org.ducatproject.ducat.StoredMessage? = null
            while (msg == null && System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.pollContact(context, contact) }
                msg = ContactStore(context).thread(contact.personaHex)
                    .firstOrNull { !it.outgoing && it.attSwarm != null }
                Thread.sleep(2_000)
            }
            val m = msg ?: error("FS_FAIL the message never arrived")
            println("FS_GOT_REF share=${m.attSwarm!!.take(24)}… len=${m.attLen}")
            val started = System.currentTimeMillis()
            check(Mailbox.fetchSwarmAttachment(context, m)) { "FS_FAIL fetch failed" }
            val ms = System.currentTimeMillis() - started
            val plain = Mailbox.attachmentFile(context, m.attHash!!).readBytes()
            println("FS_OK bytes=${plain.size} sha=${sha(plain)} ms=$ms")
        }
        else -> error("unknown role $role")
    }
}
