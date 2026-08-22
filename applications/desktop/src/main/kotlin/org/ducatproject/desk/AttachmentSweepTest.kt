package org.ducatproject.desk

import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.StoredMessage

/**
 * The directory that only ever grew. `./gradlew :desktop:attsweep`.
 *
 * Attachments are written to filesDir/att, named by ciphertext hash, and until
 * now nothing removed one. A chat set to forget its messages after an hour
 * forgot the messages and kept every picture in them; clearing a conversation
 * did the same. At a mebibyte apiece that is a phone somebody eventually
 * cannot use, and none of those files could be shown by anything again.
 *
 * The risk in adding a sweep is not that it fails to delete. It is that it
 * deletes a picture somebody can still see — and that one is unrecoverable,
 * because the DHT record it came from was deleted the moment the bytes landed
 * (§18.7). So most of what follows is about what has to survive.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-att").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)

    val sam = "aa".repeat(32)
    val jo = "bb".repeat(32)
    store.add(Contact(sam, null, "Sam", myOutbox = "m1", theirOutbox = "t1"))
    store.add(Contact(jo, null, "Jo", myOutbox = "m2", theirOutbox = "t2"))

    // The attachment fields persist as a group, hung off attRecord — which a
    // real one always carries, since that is the record the bytes came from.
    fun msg(seq: Long, hash: String?) = StoredMessage(
        outgoing = false, seq = seq, body = "", timestamp = 1_700_000_000,
        attRecord = hash?.let { "VLD0:$it" },
        attKey = hash?.let { ByteArray(32) },
        attNonce = hash?.let { ByteArray(12) },
        attMime = hash?.let { "image/jpeg" },
        attHash = hash, attLen = if (hash == null) 0 else 1024,
    )
    store.append(sam, msg(1, "keep-sams-picture"))
    store.append(sam, msg(2, null))                    // a plain message
    store.append(jo, msg(1, "keep-jos-picture"))

    val att = java.io.File(dir, "files/att").apply { mkdirs() }
    fun file(name: String, bytes: Int) =
        java.io.File(att, name).apply { writeBytes(ByteArray(bytes)) }

    file("keep-sams-picture", 3000)
    file("keep-jos-picture", 5000)
    // Left behind when the messages that pointed at them expired or the chat
    // was cleared. Nothing can ever show these again.
    file("orphan-expired", 4000)
    file("orphan-cleared", 6000)

    val freed = Mailbox.sweepAttachments(ctx)
    check(freed == 10_000L) { "ATTSWEEP_FAIL reclaimed $freed, expected 10000" }
    check(!java.io.File(att, "orphan-expired").exists()) {
        "ATTSWEEP_FAIL an orphan survived"
    }
    check(!java.io.File(att, "orphan-cleared").exists()) {
        "ATTSWEEP_FAIL an orphan survived"
    }
    for (n in listOf("keep-sams-picture", "keep-jos-picture")) {
        check(java.io.File(att, n).exists()) {
            "ATTSWEEP_FAIL the sweep took '$n', which no one can fetch again"
        }
    }
    // A second pass finds nothing, or the sweep is doing work every poll.
    check(Mailbox.sweepAttachments(ctx) == 0L) { "ATTSWEEP_FAIL not idempotent" }

    // And the room check, which is what stops the directory growing in the
    // first place. Two limits, and either one refusing is a refusal.
    val mib = 1024L * 1024
    val budget = 512 * mib
    val floor = 500 * mib

    check(Mailbox.roomForAttachment(used = 0, free = 10 * 1024 * mib, incoming = mib)) {
        "ATTSWEEP_FAIL an empty phone refused the first picture"
    }
    check(!Mailbox.roomForAttachment(used = budget, free = 10 * 1024 * mib, incoming = mib)) {
        "ATTSWEEP_FAIL the budget did not hold on a phone with plenty of room"
    }
    check(!Mailbox.roomForAttachment(used = 0, free = floor, incoming = mib)) {
        "ATTSWEEP_FAIL filled the last of a nearly-full phone"
    }
    // Exactly at the budget with the incoming counted — the boundary is where
    // an off-by-one lets one more mebibyte through every single poll.
    check(Mailbox.roomForAttachment(used = budget - mib, free = 10 * 1024 * mib, incoming = mib)) {
        "ATTSWEEP_FAIL the last picture that fits was refused"
    }
    check(!Mailbox.roomForAttachment(used = budget - mib + 1, free = 10 * 1024 * mib, incoming = mib)) {
        "ATTSWEEP_FAIL one byte over the budget was let through"
    }

    println("ATTSWEEP_OK freed=10000 kept=2 idempotent room=budget+floor")
}
