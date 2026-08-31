package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.Publications
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The shelf's whole claim, tested at its sharpest edge: the publisher
 * shelves an issue, mails the manifest, and EXITS — and a reader who
 * shows up afterwards still gets the bytes, because the mailbox record
 * holds the capability and the shelf records hold the content, and
 * neither needs the publisher breathing.
 *
 *   DUCAT_PUB_ROLE=publish   DUCAT_PUB_STATE=<dir> DUCAT_PUB_FILE=<file>
 *   DUCAT_PUB_ROLE=subscribe DUCAT_PUB_STATE=<dir> DUCAT_PUB_CARD=<uri>
 *     DUCAT_PUB_OUT=<dir>
 *
 * Publisher: SHELF_CARD → claim → shelve → sendPeriod (shelf pair, no
 * swarm) → SHELF_SENT → exit ten seconds later, deliberately.
 * Subscriber: claim → poll until key + shelf filed → fetchShelf →
 * SHELF_OK <bytes> <secs> <path>.
 */

private const val PERIOD = "2026-09"

private fun up(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "SHELF_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

fun main() {
    val role = System.getenv("DUCAT_PUB_ROLE") ?: error("SHELF_FAIL set DUCAT_PUB_ROLE")
    val state = System.getenv("DUCAT_PUB_STATE") ?: error("SHELF_FAIL set DUCAT_PUB_STATE")
    val dir = File(state).apply { mkdirs() }

    when (role) {
        "publish" -> {
            val file = File(
                System.getenv("DUCAT_PUB_FILE") ?: error("SHELF_FAIL set DUCAT_PUB_FILE"),
            )
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Night Press")
            val card = Mailbox.issueCard(context, "Night Press", 60uL * 60uL)
            println("SHELF_CARD ${card.uri}")
            System.out.flush()

            var reader: org.ducatproject.ducat.Contact? = null
            val deadline = System.currentTimeMillis() + 300_000
            while (reader == null && System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.collectClaims(context) }
                reader = ContactStore(context).all().firstOrNull()
                Thread.sleep(3_000)
            }
            checkNotNull(reader) { "SHELF_FAIL nobody claimed the card" }
            System.err.println("claimed by ${reader.personaHex.take(8)}…")

            val pubId = Publications.create(context, "The Night Press")
            check(Publications.shelveIssue(context, pubId, PERIOD, file) { i, n ->
                System.err.println("shelved $i/$n")
            }) { "SHELF_FAIL shelving refused" }
            val shelf = Publications.shelfOf(context, pubId)
                ?: error("SHELF_FAIL no shelf after shelving")
            check(
                Publications.sendPeriod(
                    context, reader, pubId, PERIOD,
                    record = shelf.first, headKey = shelf.second,
                    note = "the night edition, off the shelf",
                ),
            ) { "SHELF_FAIL manifest send failed" }
            println("SHELF_SENT $PERIOD")
            System.out.flush()
            // The point of the whole test: the publisher leaves. Ten
            // seconds for the mailbox write to settle onto the network,
            // then nobody is home — and the reader must not notice.
            Thread.sleep(10_000)
            println("SHELF_PUBLISHER_GONE deliberately")
            System.out.flush()
        }
        // The desk as pure reader for a PHONE publisher: issue the card
        // (easy to hand to an emulator as a link), wait for the claim, then
        // wait for the manifest and fetch off the shelf. The phone drives
        // its own Publishing screen; this end only reads.
        "reader" -> {
            val out = File(System.getenv("DUCAT_PUB_OUT") ?: error("SHELF_FAIL set DUCAT_PUB_OUT"))
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Desk Reader")
            val card = Mailbox.issueCard(context, "Desk Reader", 60uL * 60uL)
            println("SHELF_CARD ${card.uri}")
            System.out.flush()

            var publisher: org.ducatproject.ducat.Contact? = null
            val deadline = System.currentTimeMillis() + 900_000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.collectClaims(context) }
                runCatching { Mailbox.poll(context) }
                if (publisher == null) {
                    publisher = ContactStore(context).all().firstOrNull()
                    if (publisher != null) {
                        println("SHELF_CLAIMED ${publisher.displayName()}")
                        System.out.flush()
                    }
                }
                publisher?.let { p ->
                    val sub = Publications.subscription(context, p.personaHex)
                    val period = sub?.third?.keys?.maxOrNull()
                    if (sub?.first != null && sub.second != null && period != null) {
                        System.err.println("capability filed: shelf + key for $period")
                        val t0 = System.currentTimeMillis()
                        val got = Publications.fetchShelf(
                            context, p.personaHex, period, out,
                        ) { pos, len -> System.err.println("shelf $pos/$len") }
                        val secs = (System.currentTimeMillis() - t0) / 1000.0
                        println("SHELF_OK ${got.length()} $secs ${got.absolutePath}")
                        return
                    }
                }
                Thread.sleep(3_000)
            }
            error("SHELF_FAIL nothing arrived from the phone")
        }
        "subscribe" -> {
            val cardUri = System.getenv("DUCAT_PUB_CARD") ?: error("SHELF_FAIL set DUCAT_PUB_CARD")
            val out = File(System.getenv("DUCAT_PUB_OUT") ?: error("SHELF_FAIL set DUCAT_PUB_OUT"))
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Night Reader")
            val publisher = ContactStore(context).all().firstOrNull()
                ?: Mailbox.claimCard(
                    context, uniffi.ducat_mobile.readContactCard(cardUri), "the press",
                )
            System.err.println("publisher ${publisher.personaHex.take(8)}…")

            val deadline = System.currentTimeMillis() + 300_000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.poll(context) }
                val sub = Publications.subscription(context, publisher.personaHex)
                if (sub?.first != null && sub.second != null && sub.third[PERIOD] != null) {
                    System.err.println("capability filed: shelf + key for $PERIOD")
                    val t0 = System.currentTimeMillis()
                    val got = Publications.fetchShelf(
                        context, publisher.personaHex, PERIOD, out,
                    ) { pos, len -> System.err.println("shelf $pos/$len") }
                    val secs = (System.currentTimeMillis() - t0) / 1000.0
                    println("SHELF_OK ${got.length()} $secs ${got.absolutePath}")
                    return
                }
                Thread.sleep(3_000)
            }
            error("SHELF_FAIL the manifest never arrived")
        }
        else -> error("SHELF_FAIL unknown role $role")
    }
}
