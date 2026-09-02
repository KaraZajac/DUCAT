package org.ducatproject.desk

import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Groups
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.hexToBytes
import org.ducatproject.ducat.toHexString

/**
 * Proves the read marks, headless and offline: which row raises which dot.
 *
 * A conversation's dot is `inSeq` against its mark, and every row a member
 * writes advances `inSeq` — so before the marks learned to step over what
 * the thread does not show, a group message flagged the *sender's* direct
 * row (which then opened on nothing new) and the group row, where the words
 * were, showed no change. This drives the stores exactly as the arrival
 * funnel does — appendAndAdvance and its outbound twin — and asks the same
 * questions the Chats tab asks: unreadThreads, chatSeen, Groups.unread.
 * `./gradlew :desktop:markstest`.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-marks").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)
    val me = PersonaStore(ctx).personaHex()
    val sam = "aa".repeat(32)
    val jordan = "bb".repeat(32)
    for ((hex, name) in listOf(sam to "Sam", jordan to "Jordan")) {
        store.add(
            Contact(
                personaHex = hex, petname = name, assertedName = null,
                myOutbox = "VLD0:mine-$name", theirOutbox = "VLD0:theirs-$name",
            ),
        )
    }
    fun contact(hex: String) = store.all().first { it.personaHex == hex }

    // The group arrives the way one does: as Sam's roster naming us.
    val gid = ByteArray(16) { (it + 1).toByte() }
    val gidHex = gid.toHexString()
    Groups.absorbRoster(
        ctx, sam, gid,
        uniffi.ducat_mobile.groupRosterEncode(
            "ladder crew", listOf(me, sam, jordan).map { hexToBytes(it)!! },
        ),
    )
    check(Groups.get(ctx, gidHex)?.members?.toSet() == setOf(me, sam, jordan)) {
        "MARKSTEST_FAIL the roster did not seat the group: ${Groups.get(ctx, gidHex)}"
    }
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL a roster alone raised the group's dot" }

    var t = 1_700_000_000L
    var samSeq = 0L
    fun fromSam(body: String, kind: Int = 0, groupSeq: Long = 0L) {
        samSeq += 1
        store.appendAndAdvance(
            sam,
            StoredMessage(
                outgoing = false, seq = samSeq, body = body, timestamp = ++t, kind = kind,
                groupId = if (groupSeq > 0) gidHex else null, groupSeq = groupSeq,
            ),
            newInSeq = samSeq, newPrevLink = null,
        )
    }
    fun samSeen() = store.chatSeen(contact(sam))
    fun look() = Groups.markSeen(ctx, gidHex, Groups.lookAt(ctx, Groups.thread(ctx, gidHex)))

    // 1) A group message from a fully-read contact: the group's dot, not Sam's.
    fromSam("who has the ladder", groupSeq = 2)
    check(store.unreadThreads() == 0) { "MARKSTEST_FAIL a group message flagged the sender's direct row" }
    check(samSeen() == 1L) { "MARKSTEST_FAIL the direct mark did not step over the group row: ${samSeen()}" }
    check(Groups.unreadGroups(ctx) == 1) { "MARKSTEST_FAIL the group row did not flag the words said in it" }

    // 2) Looking at the group clears it.
    look()
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL the look did not clear the group" }

    // 3) A direct line waits; the group row behind it must not carry the
    //    mark past the line.
    fromSam("are you coming tonight")
    check(store.unreadThreads() == 1) { "MARKSTEST_FAIL a direct line did not flag the conversation" }
    fromSam("I found one", groupSeq = 3)
    check(samSeen() == 1L) { "MARKSTEST_FAIL the mark stepped past a waiting direct line: ${samSeen()}" }
    check(store.unreadThreads() == 1) { "MARKSTEST_FAIL the waiting line was lost behind a group row" }
    check(Groups.unreadGroups(ctx) == 1) { "MARKSTEST_FAIL the second group message did not flag the group" }
    store.setChatSeen(contact(sam))
    check(store.unreadThreads() == 0) { "MARKSTEST_FAIL opening the conversation did not settle it" }
    look()

    // 4) Rows leave — a retention window, a long press — and nothing new was
    //    said: no dot. The next word still counts.
    store.deleteMessage(sam, seq = 3, outgoing = false)
    check(Groups.thread(ctx, gidHex).size == 1) { "MARKSTEST_FAIL the deletion did not take" }
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL a deleted row raised the group's dot" }
    look()
    check(Groups.seenLook(ctx, gidHex).high[sam] == 3L) {
        "MARKSTEST_FAIL the mark came down with the deleted row: ${Groups.seenLook(ctx, gidHex).high}"
    }
    fromSam("never mind, borrowed one", groupSeq = 4)
    check(Groups.unreadGroups(ctx) == 1) { "MARKSTEST_FAIL a word after a sweep did not flag the group" }
    look()

    // 5) A gap filled in late — a word that arrives *under* the mark.
    //
    //    A group message is fanned out per member, so one member's copy can
    //    fail while the rest land. The sender retries, and the message
    //    reaches this phone after the words that followed it, carrying its
    //    original group counter: a number below the high-water mark this
    //    group has already been looked at. Nobody here has ever seen it.
    //
    //    A mark that is only a maximum cannot tell that from a duplicate —
    //    which `merge` drops on (sender, groupSeq) and is right to. So the
    //    words land quietly in the middle of the thread, above everything
    //    the reader has already read past, and the group says nothing.
    check(Groups.thread(ctx, gidHex).none { it.senderHex == sam && it.message.groupSeq == 1L }) {
        "MARKSTEST_FAIL the gap this step fills is not a gap"
    }
    fromSam("bringing the ladder round at six", groupSeq = 1)
    check(Groups.thread(ctx, gidHex).any { it.senderHex == sam && it.message.groupSeq == 1L }) {
        "MARKSTEST_FAIL the late word did not reach the merged view at all"
    }
    check(Groups.unreadGroups(ctx) == 1) {
        "MARKSTEST_FAIL a word that landed under the mark did not flag the group"
    }
    look()
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL the look did not clear the late word" }

    //    And the duplicate the retry may also produce is still not news: the
    //    same (sender, groupSeq) is one row however many times it arrives.
    fromSam("bringing the ladder round at six", groupSeq = 1)
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL a duplicate copy raised the dot" }

    //    A phone that upgrades into the counts has marks and no counts, and
    //    reading an absent count as zero would flag every group it has ever
    //    been in — once, for nothing, on a screen the user did not touch.
    //    Dropping the key is exactly what that phone looks like.
    org.ducatproject.ducat.securePrefs(ctx, "ducat_groups")
        .edit().remove("rows_$gidHex").apply()
    check(Groups.seenLook(ctx, gidHex).rows.isEmpty()) { "MARKSTEST_FAIL the counts did not go" }
    check(Groups.unreadGroups(ctx) == 0) {
        "MARKSTEST_FAIL marks without counts flagged a group nobody had spoken in"
    }
    //    …and the first look after the upgrade writes them, so the check
    //    starts working from there rather than never.
    look()
    check(Groups.seenLook(ctx, gidHex).rows[sam] == 3L) {
        "MARKSTEST_FAIL the first look after an upgrade did not record the counts: " +
            "${Groups.seenLook(ctx, gidHex).rows}"
    }

    // 6) Our own copies fanned into a member's thread are not news to us.
    store.appendAndAdvanceOutbound(
        sam,
        StoredMessage(
            outgoing = true, seq = 1, body = "thanks", timestamp = ++t,
            groupId = gidHex, groupSeq = 1,
        ),
        newOutSeq = 1, newPrevLink = ByteArray(32), sealedSlot = ByteArray(8),
    )
    check(Groups.thread(ctx, gidHex).any { it.senderHex == me }) { "MARKSTEST_FAIL our own row is not in the merged view" }
    check(Groups.unreadGroups(ctx) == 0) { "MARKSTEST_FAIL our own words flagged the group" }

    // 7) A deleted conversation comes back for a person's words — theirs or
    //    ours — and stays gone for machinery and for a group's fan-out copy.
    store.setChatVisible(jordan, false)
    store.appendAndAdvance(
        jordan,
        StoredMessage(outgoing = false, seq = 1, body = "group: ladder crew", timestamp = ++t, kind = 12, groupId = gidHex, groupSeq = 1),
        newInSeq = 1, newPrevLink = null,
    )
    check(!contact(jordan).chatVisible) { "MARKSTEST_FAIL a roster un-hid a deleted conversation" }
    check(store.unreadThreads() == 0) { "MARKSTEST_FAIL a roster in a hidden conversation counted as unread" }
    store.appendAndAdvanceOutbound(
        jordan,
        StoredMessage(outgoing = true, seq = 1, body = "on my way", timestamp = ++t, groupId = gidHex, groupSeq = 2),
        newOutSeq = 1, newPrevLink = ByteArray(32), sealedSlot = ByteArray(8),
    )
    check(!contact(jordan).chatVisible) { "MARKSTEST_FAIL our group copy un-hid a deleted conversation" }
    store.appendAndAdvanceOutbound(
        jordan,
        StoredMessage(outgoing = true, seq = 2, body = "still there?", timestamp = ++t),
        newOutSeq = 2, newPrevLink = ByteArray(32), sealedSlot = ByteArray(8),
    )
    check(contact(jordan).chatVisible) { "MARKSTEST_FAIL our own line did not bring the conversation back" }
    check(store.unreadThreads() == 0) { "MARKSTEST_FAIL our own line counted as unread" }
    store.setChatVisible(jordan, false)
    store.appendAndAdvance(
        jordan,
        StoredMessage(outgoing = false, seq = 2, body = "yes", timestamp = ++t),
        newInSeq = 2, newPrevLink = null,
    )
    check(contact(jordan).chatVisible) { "MARKSTEST_FAIL their line did not bring the conversation back" }
    check(store.unreadThreads() == 1) { "MARKSTEST_FAIL their line came back without its dot" }

    // 8) Deleting the conversation with Jordan takes Jordan's words to us,
    //    not Jordan's words to the crew: those are shown in the group and
    //    are the group's to delete.
    val crewBefore = Groups.thread(ctx, gidHex).size
    check(crewBefore > 0) { "MARKSTEST_FAIL no crew rows to keep" }
    store.deleteThread(jordan)
    check(store.thread(jordan).none { it.groupId == null }) { "MARKSTEST_FAIL a deleted conversation kept a direct line" }
    check(Groups.thread(ctx, gidHex).size == crewBefore) {
        "MARKSTEST_FAIL deleting the conversation took the crew's rows with it " +
            "(${Groups.thread(ctx, gidHex).size} of $crewBefore left)"
    }
    // And a thread with nothing but direct lines is gone outright.
    store.deleteThread(sam)
    check(store.thread(sam).none { it.groupId == null }) { "MARKSTEST_FAIL Sam's direct lines survived the delete" }
    // Sam's crew rows, minus the one deleted in step 4, are still the crew's.
    check(Groups.thread(ctx, gidHex).size == crewBefore) {
        "MARKSTEST_FAIL deleting Sam's conversation took Sam's crew rows"
    }

    println(
        "MARKSTEST_OK group=${Groups.get(ctx, gidHex)?.name} " +
            "marks=${Groups.seenLook(ctx, gidHex).high.mapKeys { it.key.take(4) }} " +
            "sam.seen=${samSeen()} unread=${store.unreadThreads()}"
    )
}
