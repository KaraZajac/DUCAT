package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.toHexString
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The 1.2 + 1.3 vertical, whole: a publisher desk and a subscriber desk,
 * strangers until the card claim, and then one kind-13 manifest carrying
 * the period's key AND the shipment — share key + index digest — down the
 * sealed thread, with the heavy bytes arriving by swarm and the period
 * key ready to open them.
 *
 *   DUCAT_PUB_ROLE=publish DUCAT_PUB_STATE=<dir> DUCAT_PUB_FILE=<file>
 *   DUCAT_PUB_ROLE=subscribe DUCAT_PUB_STATE=<dir> DUCAT_PUB_CARD=<uri>
 *     DUCAT_PUB_OUT=<dir>
 *
 * Publisher: issues a card (PUBSWARM_CARD <uri>), waits for the claim,
 * seeds the file, sends the manifest, serves and polls until killed.
 * Subscriber: claims, polls until the manifest lands and is filed, then
 * fetches by the FILED pair — the cabinet, not the message, is what the
 * app will read — and prints PUBSWARM_OK <bytes> <secs> <period>.
 */
fun main() {
    val role = System.getenv("DUCAT_PUB_ROLE") ?: error("PUBSWARM_FAIL set DUCAT_PUB_ROLE")
    val state = System.getenv("DUCAT_PUB_STATE") ?: error("PUBSWARM_FAIL set DUCAT_PUB_STATE")
    val dir = File(state).apply { mkdirs() }
    val context = DeskContext(dir)

    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "PUBSWARM_FAIL node never became ready" }
    System.err.println("node ready")

    val store = ContactStore(context)
    val period = "2026-09"

    when (role) {
        "publish" -> {
            val file = System.getenv("DUCAT_PUB_FILE") ?: error("PUBSWARM_FAIL set DUCAT_PUB_FILE")
            val card = Mailbox.issueCard(context, "publisher", 60uL * 60uL)
            println("PUBSWARM_CARD ${card.uri}")
            System.out.flush()

            // The subscriber's claim makes them a contact; the poll loop is
            // the same one the app runs.
            var reader: org.ducatproject.ducat.Contact? = null
            val deadline = System.currentTimeMillis() + 300_000
            while (reader == null && System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.collectClaims(context) }
                reader = store.all().firstOrNull()
                Thread.sleep(3_000)
            }
            checkNotNull(reader) { "PUBSWARM_FAIL nobody claimed the card" }
            System.err.println("claimed by ${reader.personaHex.take(8)}…")

            // The month, sealed and shipped: for this proof the payload IS
            // the sealed period bundle (the period key would open it; the
            // sealing itself is core::publish, exercised by its own tests).
            val pubId = Publications.create(context, "The Proof Gazette")
            val share = Swarm.seed(file)
            System.err.println("seeded ${share.shareKey.take(24)}…")
            val sent = Publications.sendPeriod(
                context, reader, pubId, period,
                record = null, headKey = null,
                note = "september's issue, by swarm",
                swarmKey = share.shareKey,
                swarmDigestHex = share.indexDigestHex,
            )
            check(sent) { "PUBSWARM_FAIL manifest send failed" }
            println("PUBSWARM_SENT $period")
            System.out.flush()
            // Serve and keep the mailbox warm until the runner kills us.
            while (true) {
                runCatching { Mailbox.poll(context) }
                Thread.sleep(5_000)
            }
        }
        "subscribe" -> {
            val cardUri = System.getenv("DUCAT_PUB_CARD") ?: error("PUBSWARM_FAIL set DUCAT_PUB_CARD")
            val out = System.getenv("DUCAT_PUB_OUT") ?: error("PUBSWARM_FAIL set DUCAT_PUB_OUT")
            File(out).mkdirs()
            val scanned = uniffi.ducat_mobile.readContactCard(cardUri)
            val publisher = Mailbox.claimCard(context, scanned, "the gazette")
            System.err.println("claimed ${publisher.personaHex.take(8)}…")

            // Poll until the manifest lands AND the cabinet filed it — the
            // app reads the cabinet, so the proof does too.
            var shipment: Pair<String, String>? = null
            val deadline = System.currentTimeMillis() + 300_000
            while (shipment == null && System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.poll(context) }
                shipment = Publications.shipment(context, publisher.personaHex, period)
                Thread.sleep(3_000)
            }
            checkNotNull(shipment) { "PUBSWARM_FAIL no shipment arrived" }
            val keys = Publications.subscription(context, publisher.personaHex)
            checkNotNull(keys?.third?.get(period)) { "PUBSWARM_FAIL period key not filed" }
            System.err.println("manifest filed: key + shipment for $period")

            val t0 = System.currentTimeMillis()
            val bytes = Swarm.fetch(shipment.first, shipment.second, out)
            val secs = (System.currentTimeMillis() - t0) / 1000.0
            println("PUBSWARM_OK $bytes $secs $period")
        }
        else -> error("PUBSWARM_FAIL unknown role $role")
    }
}
