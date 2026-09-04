package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.WalletStore
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
 * Set DUCAT_WANT_PRICE (piconero, both roles) for the other answer. Priced
 * skips the shelf: it is sealed under the head key, which rides the *first*
 * delivery, so a reader who has paid for nothing cannot read the catalogue
 * at all — a stranger's discovery path is the board notice (§16.18.2), not
 * the shelf. The rail does not need it either; a period id is a label, and
 * asking for one is how a reader buys it. What is checked instead is that
 * the ask bills the asker, for the period they named, once, and that the
 * newcomer's own enrolment bill is left alone.
 *
 * Publisher markers: WANT_CARD, WANT_SHELVED, WANT_ENROLLED, WANT_ANSWERED,
 * WANT_BILLED. Reader markers: WANT_LATEST, WANT_LOCKED, WANT_ASKED,
 * WANT_OK, WANT_BILL.
 *
 *   DUCAT_WANT_ROLE=publish DUCAT_WANT_STATE=<dir> [DUCAT_WANT_PRICE=<pxmr>]
 *   DUCAT_WANT_ROLE=read    DUCAT_WANT_STATE=<dir> DUCAT_WANT_CARD=<uri>
 *                           [DUCAT_WANT_PRICE=<pxmr>]
 */

private const val OLDER = "2026-08"
private const val NEWER = "2026-09"

/** 0 = free. Both roles must agree, so it is read from one place. */
private val PRICE = System.getenv("DUCAT_WANT_PRICE")?.toLongOrNull() ?: 0L

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
            Publications.setPrice(context, pubId, PRICE)
            // A bill has to name somewhere to pay; without an address of our
            // own TabStore.settle has nothing to put in the payto field.
            if (PRICE > 0L && WalletStore(context).address() == null) {
                val w = uniffi.ducat_mobile.createWallet(tipHeight = 0uL, stagenet = true)
                WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
            }

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
                val newerBilled = Publications.billedFor(context, pubId, NEWER)
                val olderBilled = Publications.billedFor(context, pubId, OLDER)
                if (!enrolled) {
                    // Free hands the newcomer the newest; priced bills them
                    // for it. Either way the claim is what did it.
                    val done = if (PRICE > 0L) newerBilled.isNotEmpty()
                    else issues[NEWER]?.sentTo?.isNotEmpty() == true
                    if (done) {
                        enrolled = true
                        println(
                            if (PRICE > 0L) "WANT_ENROLLED $NEWER billed on the claim"
                            else "WANT_ENROLLED $NEWER handed over on the claim",
                        )
                        System.out.flush()
                    }
                }
                if (enrolled && !answered && PRICE > 0L && olderBilled.isNotEmpty()) {
                    answered = true
                    // One bill, to the asker, for the period they named —
                    // and the enrolment bill for the newer period is a
                    // separate piece of paper that this did not disturb.
                    check(olderBilled.size == 1) {
                        "WANTWIRE_FAIL an ask from one reader billed ${olderBilled.size}"
                    }
                    check(olderBilled.keys == newerBilled.keys) {
                        "WANTWIRE_FAIL the ask billed somebody who never asked"
                    }
                    check(issues[OLDER]?.sentTo.isNullOrEmpty()) {
                        "WANTWIRE_FAIL a priced period went out unpaid"
                    }
                    println("WANT_BILLED $OLDER billed to the asker alone")
                    System.out.flush()
                }
                // The free ask's whole point: the older period goes out
                // having never been billed and never been offered.
                if (enrolled && !answered && PRICE == 0L &&
                    issues[OLDER]?.sentTo?.isNotEmpty() == true
                ) {
                    answered = true
                    check(olderBilled.isEmpty()) {
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

                val bills = ContactStore(context).thread(hex).filter { !it.outgoing && it.kind == 1 }
                if (!gotLatest) {
                    // Free: the key itself arrives. Priced: a bill for the
                    // newest period is what claiming gets you, and no key
                    // comes until it settles.
                    val done = if (PRICE > 0L) bills.isNotEmpty() else NEWER in owned
                    if (done) {
                        gotLatest = true
                        println(
                            if (PRICE > 0L) "WANT_LATEST enrolment bill arrived"
                            else "WANT_LATEST $NEWER arrived on the claim",
                        )
                        System.out.flush()
                    }
                }

                // The shelf index is what makes a locked row possible at all:
                // it is sealed under the head key, which arrived with that
                // first delivery, so the reader can see the whole catalogue
                // and open only the part they hold.
                // Priced readers cannot read the shelf yet — the head key
                // rides the first delivery — so the ask stands on the period
                // label alone, which is all it ever carried.
                if (gotLatest && !sawLocked && PRICE > 0L) {
                    sawLocked = true
                    println("WANT_LOCKED $OLDER named without the shelf")
                    System.out.flush()
                }
                if (gotLatest && !sawLocked && PRICE == 0L) {
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

                if (asked && PRICE > 0L && bills.size >= 2) {
                    // The ask came back as paper, not as a key: the second
                    // bill is the answer, and the period is still locked.
                    check(OLDER !in owned) {
                        "WANTWIRE_FAIL a priced period unlocked without paying"
                    }
                    val amounts = bills.map { it.amountPxmr }.toSet()
                    check(amounts == setOf(PRICE)) {
                        "WANTWIRE_FAIL bills came to $amounts, not $PRICE"
                    }
                    println("WANT_BILL ${bills.size} bills at $PRICE, nothing unlocked")
                    return
                }
                if (asked && PRICE == 0L && OLDER in owned) {
                    // Free means free: nothing on this thread ever billed.
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
