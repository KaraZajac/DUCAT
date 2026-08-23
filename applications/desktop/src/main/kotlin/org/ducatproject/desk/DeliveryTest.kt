package org.ducatproject.desk

import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.StoredMessage

/**
 * Whether a message has actually left the phone. `./gradlew :desktop:delivery`
 *
 * A send persists the row *before* it writes the DHT slot, and it has to: the
 * sealed bytes are committed from that moment, because a re-seal would put
 * different content under a sequence number that may already have gone out.
 * So a failed write leaves a message sitting in the thread — which is right,
 * it goes out with the next one — but until now it looked exactly like a
 * message that had been delivered.
 *
 * `delivered` existed on StoredMessage the whole time, round-tripped through
 * JSON, and was never once set or read.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-deliv").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)
    val sam = "aa".repeat(32)
    store.add(Contact(sam, null, "Sam", myOutbox = "m", theirOutbox = "t"))

    fun msg(seq: Long, delivered: Boolean) = StoredMessage(
        outgoing = true, seq = seq, body = "message $seq",
        timestamp = 1_700_000_000, delivered = delivered,
    )

    store.append(sam, msg(1, delivered = false))
    store.append(sam, msg(2, delivered = false))
    // Theirs. An inbound message is delivered by definition — it arrived.
    store.append(sam, StoredMessage(outgoing = false, seq = 1, body = "hi", timestamp = 1))

    fun undelivered() = store.thread(sam).filter { it.outgoing && !it.delivered }.map { it.seq }

    check(undelivered() == listOf(1L, 2L)) { "DELIV_FAIL setup: ${undelivered()}" }

    // The write lands for the first one only.
    store.markDelivered(sam, 1)
    check(undelivered() == listOf(2L)) {
        "DELIV_FAIL marking seq 1 changed the wrong rows: ${undelivered()}"
    }
    // It survives being written down, or every restart would re-accuse a
    // message that went out fine.
    check(store.thread(sam).first { it.outgoing && it.seq == 1L }.delivered) {
        "DELIV_FAIL the mark did not persist"
    }

    // Idempotent: the interrupted-send path can mark a seq that is already
    // delivered, and a poll runs it repeatedly.
    store.markDelivered(sam, 1)
    check(undelivered() == listOf(2L)) { "DELIV_FAIL not idempotent" }
    // And a sequence number nobody has is not an error.
    store.markDelivered(sam, 99)
    check(undelivered() == listOf(2L)) { "DELIV_FAIL an unknown seq disturbed the thread" }

    // The inbound message is untouched — it has no delivery of ours to track,
    // and marking by seq alone would have caught it, since seq 1 exists on
    // both sides of a thread.
    val theirs = store.thread(sam).first { !it.outgoing }
    check(theirs.delivered && theirs.body == "hi") {
        "DELIV_FAIL an incoming message was rewritten by an outgoing mark"
    }

    // Everything written before this existed defaults to delivered. Without
    // that, one update would put "not sent yet" under every message anybody
    // had ever sent.
    val old = StoredMessage.from(
        org.json.JSONObject(msg(7, delivered = true).toJson().toString())
            .also { it.remove("delivered") },
    )
    check(old.delivered) { "DELIV_FAIL an older message read back as undelivered" }

    println("DELIV_OK marked=1 pending=[2] idempotent inbound=untouched legacy=delivered")
}
