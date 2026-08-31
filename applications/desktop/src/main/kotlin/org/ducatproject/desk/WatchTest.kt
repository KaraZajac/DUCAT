package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import uniffi.ducat_mobile.nodeChangedKeys
import uniffi.ducat_mobile.nodeDhtWatch
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeWaitChange

/**
 * The push probe (PUSH.md stage A): does a DHT watch on a contact's outbox
 * turn a write into a prompt ring, and does it stay armed through renewal?
 *
 *   DUCAT_WT_ROLE=writer  DUCAT_WT_STATE=<dir>   # ticks its first contact
 *   DUCAT_WT_ROLE=watcher DUCAT_WT_STATE=<dir> DUCAT_WT_CARD=<uri>
 *
 * The writer sends "tick N" every 20 s for 15 rounds, sleeps 20 minutes
 * (the renewal gap — a watch that expires unrenewed goes quiet here), then
 * sends 3 more. Every send prints `WT_SENT n=N at=<epoch ms>`.
 *
 * The watcher claims, polls once (opening the records), arms the watch,
 * and then only ever reads when the network rings: `WT_RING at=<ms>` and,
 * after the targeted read, `WT_GOT n=N at=<ms>`. Latency = GOT − SENT,
 * same host clock. A watcher that logs WT_STALL instead has been waiting
 * five minutes past a due tick — the watch died and this probe says so.
 */

private fun up(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "WT_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

fun main() {
    val role = System.getenv("DUCAT_WT_ROLE") ?: error("WT_FAIL set DUCAT_WT_ROLE")
    val dir = File(System.getenv("DUCAT_WT_STATE") ?: error("WT_FAIL set DUCAT_WT_STATE"))
        .apply { mkdirs() }

    when (role) {
        "writer" -> {
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Ticker")
            var callee = ContactStore(context).all().firstOrNull()
            if (callee == null) {
                val card = Mailbox.issueCard(context, "Ticker", 60uL * 60uL)
                println("WT_CARD ${card.uri}")
                System.out.flush()
                val deadline = System.currentTimeMillis() + 600_000
                while (callee == null && System.currentTimeMillis() < deadline) {
                    runCatching { Mailbox.collectClaims(context) }
                    callee = ContactStore(context).all().firstOrNull()
                    Thread.sleep(2_000)
                }
            }
            val to = callee ?: error("WT_FAIL nobody claimed")
            var c = to
            var n = 0
            fun tick() {
                n++
                c = Mailbox.send(context, c, "tick $n")
                println("WT_SENT n=$n at=${System.currentTimeMillis()}")
                System.out.flush()
            }
            val rounds = (System.getenv("DUCAT_WT_ROUNDS") ?: "15").toInt()
            repeat(rounds) {
                tick()
                Thread.sleep(20_000)
            }
            if (rounds >= 15) {
                println("WT_QUIET 20 minutes — the renewal gap")
                System.out.flush()
                Thread.sleep(20 * 60_000)
                repeat(3) {
                    tick()
                    Thread.sleep(20_000)
                }
            }
            println("WT_WRITER_DONE")
        }
        "watcher" -> {
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Listener")
            val contact = ContactStore(context).all().firstOrNull()
                ?: Mailbox.claimCard(
                    context,
                    uniffi.ducat_mobile.readContactCard(
                        System.getenv("DUCAT_WT_CARD") ?: error("WT_FAIL set DUCAT_WT_CARD"),
                    ),
                    "ticker",
                )
            // One sweep to open the records a watch needs open.
            runCatching { Mailbox.poll(context) }
            val armed = runCatching { nodeDhtWatch(contact.theirOutbox) }.getOrDefault(false)
            println("WT_ARMED $armed at=${System.currentTimeMillis()}")
            System.out.flush()

            var lastSeq = ContactStore(context).thread(contact.personaHex)
                .filter { !it.outgoing }.maxOfOrNull { it.seq } ?: -1L
            var lastHeard = System.currentTimeMillis()
            var lastArm = System.currentTimeMillis()
            val end = System.currentTimeMillis() + 32 * 60_000
            while (System.currentTimeMillis() < end) {
                // Stage A's lesson: a watch armed once dies quietly. The
                // phone re-stamps every pass; so does this probe now.
                if (System.currentTimeMillis() - lastArm > 15_000) {
                    lastArm = System.currentTimeMillis()
                    runCatching { nodeDhtWatch(contact.theirOutbox) }
                }
                val rang = nodeWaitChange(2_000u)
                if (!rang) {
                    if (System.currentTimeMillis() - lastHeard > 300_000) {
                        println("WT_STALL at=${System.currentTimeMillis()}")
                        System.out.flush()
                        lastHeard = System.currentTimeMillis()
                    }
                    continue
                }
                val at = System.currentTimeMillis()
                val moved = runCatching { nodeChangedKeys() }.getOrDefault(emptyList())
                println("WT_RING keys=${moved.size} at=$at")
                if (contact.theirOutbox in moved || moved.isEmpty()) {
                    runCatching { Mailbox.pollContact(context, contact) }
                    val fresh = ContactStore(context).thread(contact.personaHex)
                        .filter { !it.outgoing }
                    for (m in fresh.filter { it.seq > lastSeq }.sortedBy { it.seq }) {
                        println("WT_GOT n=${m.body.removePrefix("tick ")} at=${System.currentTimeMillis()}")
                        lastSeq = m.seq
                        lastHeard = System.currentTimeMillis()
                    }
                    System.out.flush()
                    // Idempotent re-arm; veilid only renegotiates on change.
                    runCatching { nodeDhtWatch(contact.theirOutbox) }
                }
            }
            println("WT_WATCHER_DONE")
        }
        else -> error("WT_FAIL unknown role $role")
    }
}
