package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.Publications
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * §16.20's ask on the wire, between two stranger desks with no operator
 * touch after setup — the half `wantedtest` cannot reach.
 *
 * `wantedtest` pins the deciding, the vectors pin the encoding, and
 * `mobile/tests/wanted.rs` pins the bridge's argument list. None of them
 * can see a kind-16 leave one node, land in another's mailbox, get
 * dispatched, and come back as a key. This does.
 *
 * The shape is the back catalogue, which is the ask's real case: a
 * publisher shelves two periods, a reader arrives afterwards and is handed
 * only the newest (that is what claiming a card does), and the older one
 * sits on the shelf visible and unopenable. Asking for it is the only way
 * to get it, and on a free publication the answer is the key itself — no
 * bill, no tab, no settlement, because free is a path and not a zero.
 *
 * Publisher markers: WANT_CARD, WANT_SHELVED, WANT_ENROLLED, WANT_ANSWERED.
 * Reader markers: WANT_LATEST, WANT_LOCKED, WANT_ASKED, WANT_OK.
 *
 *   DUCAT_WANT_ROLE=publish DUCAT_WANT_STATE=<dir>
 *   DUCAT_WANT_ROLE=read    DUCAT_WANT_STATE=<dir> DUCAT_WANT_CARD=<uri>
 */

private const val OLDER = "2026-08"
private const val NEWER = "2026-09"

private fun ready(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "WANTWIRE_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

/** Shelf-sized and recognisable, so a wrong period is obvious in the bytes. */
private fun issueFile(dir: File, period: String): File =
    File(dir, "$period.txt").apply {
        writeText("The Asked Gazette — $period\n".repeat(200))
    }

fun main() {
    val role = System.getenv("DUCAT_WANT_ROLE") ?: error("WANTWIRE_FAIL set DUCAT_WANT_ROLE")
    val state = System.getenv("DUCAT_WANT_STATE") ?: error("WANTWIRE_FAIL set DUCAT_WANT_STATE")
    val dir = File(state).apply { mkdirs() }
    val deadline = System.currentTimeMillis() + 45 * 60_000L

    when (role) {
        "publish" -> {
            val context = ready(dir)
            NameStore(context).get() ?: NameStore(context).put("The Asked Gazette")

            // Reused across restarts, like pubsettletest's: a second create()
            // would orphan the shelf the reader is already holding keys for.
            val pubId = Publications.publications(context)
                .firstOrNull { it.second == "The Asked Gazette" }?.first
                ?: Publications.create(context, "The Asked Gazette")
            // Free on purpose. The priced answer is pubsettletest's path with
            // an ask in front of it; the free answer exists nowhere else.
            Publications.setPrice(context, pubId, 0L)

            // purpose "publish", not the default "profile": the claims
            // funnel enrols only on a publish card (§16.20's
            // scan-to-subscribe), so a profile card claims a thread and
            // subscribes nobody. The Publishing room mints it the same way.
            val card = Mailbox.issueCard(
                context, "The Asked Gazette", 60uL * 60uL, purpose = "publish",
            )
            Publications.bindCard(context, pubId, card.inboxKey)
            println("WANT_CARD ${card.uri}")
            System.out.flush()

            // Oldest first, so `issues` (newest first) hands the newcomer the
            // newer one and leaves the older locked — the back catalogue.
            val have = Publications.issues(context, pubId).map { it.periodId }.toSet()
            for (p in listOf(OLDER, NEWER)) {
                if (p in have) continue
                check(Publications.shelveIssue(context, pubId, p, issueFile(dir, p))) {
                    "WANTWIRE_FAIL could not shelve $p"
                }
                System.err.println("shelved $p")
            }
            println("WANT_SHELVED $OLDER $NEWER")
            System.out.flush()

            var enrolled = false
            var answered = false
            while (System.currentTimeMillis() < deadline) {
                // collectClaims enrols and hands over the newest; poll is what
                // dispatches an arriving kind-16 into Publications.onWanted.
                // Both are the app's own lines — the test proves the wiring,
                // not a private copy of it.
                runCatching { Mailbox.collectClaims(context) }
                runCatching { Mailbox.poll(context) }

                val issues = Publications.issues(context, pubId).associateBy { it.periodId }
                if (!enrolled && issues[NEWER]?.sentTo?.isNotEmpty() == true) {
                    enrolled = true
                    println("WANT_ENROLLED $NEWER handed over on the claim")
                    System.out.flush()
                }
                // The ask's whole point: the older period goes out having
                // never been billed and never been offered.
                if (enrolled && !answered && issues[OLDER]?.sentTo?.isNotEmpty() == true) {
                    answered = true
                    check(Publications.billedFor(context, pubId, OLDER).isEmpty()) {
                        "WANTWIRE_FAIL a free ask raised a bill"
                    }
                    println("WANT_ANSWERED $OLDER sent, no bill raised")
                    System.out.flush()
                }
                if (answered) {
                    // Keep serving: the reader still has to read the shelf.
                    Thread.sleep(5_000)
                    runCatching { Mailbox.poll(context) }
                    if (System.currentTimeMillis() > deadline - 40 * 60_000L) continue
                }
                Thread.sleep(3_000)
            }
            error("WANTWIRE_FAIL publisher timed out (enrolled=$enrolled answered=$answered)")
        }

        "read" -> {
            val cardUri = System.getenv("DUCAT_WANT_CARD")
                ?: error("WANTWIRE_FAIL set DUCAT_WANT_CARD")
            val context = ready(dir)
            NameStore(context).get() ?: NameStore(context).put("Reader")

            val publisher = ContactStore(context).all().firstOrNull()
                ?: Mailbox.claimCard(
                    context, uniffi.ducat_mobile.readContactCard(cardUri), "the asked gazette",
                )
            val hex = publisher.personaHex
            System.err.println("publisher ${hex.take(8)}…")

            var gotLatest = false
            var sawLocked = false
            var asked = false
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.poll(context) }
                val owned = Publications.subscription(context, hex)?.third?.keys ?: emptySet()

                if (!gotLatest && NEWER in owned) {
                    gotLatest = true
                    println("WANT_LATEST $NEWER arrived on the claim")
                    System.out.flush()
                }

                // The shelf index is what makes a locked row possible at all:
                // it is sealed under the head key, which arrived with that
                // first delivery, so the reader can see the whole catalogue
                // and open only the part they hold.
                if (gotLatest && !sawLocked) {
                    val n = Publications.refreshShelf(context, hex)
                    val shelf = Publications.shelvedPeriods(context, hex).keys
                    System.err.println("shelf read: $n period(s) $shelf owned=$owned")
                    if (OLDER in shelf && OLDER !in owned) {
                        sawLocked = true
                        println("WANT_LOCKED $OLDER on the shelf, no key held")
                        System.out.flush()
                    }
                }

                if (sawLocked && !asked) {
                    val fresh = ContactStore(context).all().first { it.personaHex == hex }
                    check(Publications.askForPeriod(context, fresh, OLDER)) {
                        "WANTWIRE_FAIL the ask would not send"
                    }
                    asked = true
                    check(Publications.askedFor(context, hex, OLDER)) {
                        "WANTWIRE_FAIL the thread does not remember the ask"
                    }
                    println("WANT_ASKED $OLDER")
                    System.out.flush()
                }

                if (asked && OLDER in owned) {
                    // Free means free: nothing on this thread ever billed.
                    val bills = ContactStore(context).thread(hex).filter { !it.outgoing && it.kind == 1 }
                    check(bills.isEmpty()) {
                        "WANTWIRE_FAIL a free ask came back as ${bills.size} bill(s)"
                    }
                    println("WANT_OK $OLDER unlocked by asking, no bill on the thread")
                    return
                }
                Thread.sleep(3_000)
            }
            error(
                "WANTWIRE_FAIL reader timed out (latest=$gotLatest locked=$sawLocked asked=$asked)",
            )
        }
        else -> error("WANTWIRE_FAIL unknown role $role")
    }
}
