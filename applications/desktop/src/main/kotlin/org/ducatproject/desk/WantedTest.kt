package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Publications

/**
 * §16.20's ask, deciding half: which publication answers a reader who names
 * a period — and when nothing should.
 *
 * The ask carries a period and no publication, because a period label is
 * all a reader can honestly know (they file keys by publisher and period,
 * and so does everyone else). So the publisher resolves it against their
 * own shelf, and every wrong answer here is either the wrong thing sold or
 * the wrong person billed — which is why the deciding half is a function of
 * its own, exercised with no network in reach.
 *
 *   DUCAT_WANTED_STATE=<dir> ./gradlew :desktop:wantedtest
 */
fun main() {
    // A fresh directory per run, under the one the caller named. This is a
    // test over a store, and a store keeps what the last run put in it: a
    // second run against the same directory found two Gazettes holding the
    // same period and wantedTarget refused to guess between them — correct
    // behaviour, reported as a failure, which is the worst kind of red.
    // sitepublishtest solves it the same way.
    val state = System.getenv("DUCAT_WANTED_STATE") ?: error("WANTED_FAIL set DUCAT_WANTED_STATE")
    val root = File(state).apply { mkdirs() }
    val context = DeskContext(kotlin.io.path.createTempDirectory(root.toPath(), "run").toFile())

    val reader = "aa".repeat(32)
    val stranger = "cc".repeat(32)

    // Reused by name rather than created blind: a second run against the
    // same state dir would otherwise stack a fresh Gazette on the old one,
    // and wantedTarget would correctly refuse to guess between them — a
    // green test failing on its own leftovers, which reads as a regression.
    fun publication(name: String): String =
        Publications.publications(context).firstOrNull { it.second == name }?.first
            ?: Publications.create(context, name)

    val gazette = publication("The Gazette")
    Publications.setSubscriber(context, gazette, reader, true)
    Publications.recordIssue(
        context, gazette, "2026-09", "/tmp/sep.bin", "VLD0:sep", "11".repeat(32),
    )

    fun ok(name: String, cond: Boolean) {
        check(cond) { "WANTED_FAIL $name" }
        println("WANT ok   $name")
    }

    // The ordinary case: one publication holds it, the reader subscribes.
    ok(
        "resolves the only publication holding the period",
        Publications.wantedTarget(context, reader, "2026-09") == gazette,
    )

    // A stranger buying a one-off resolves against everything holding it —
    // asking is how you buy, so a non-subscriber must not be turned away.
    ok(
        "a non-subscriber still resolves",
        Publications.wantedTarget(context, stranger, "2026-09") == gazette,
    )

    // A period nobody shelved cannot be sold: billing for it would be
    // billing for vapour, and pay-then-ship would never ship.
    ok(
        "an unshelved period resolves to nothing",
        Publications.wantedTarget(context, reader, "2026-08") == null,
    )

    // Not a name — refused before it can be filed or listed.
    ok(
        "a period id that is not a name is refused",
        Publications.wantedTarget(context, reader, "../../etc/passwd") == null,
    )

    // Two of ours hold the same label. Guessing sells the wrong thing or
    // bills for it, so the ask goes unanswered — which the wire allows.
    val herald = publication("The Herald")
    Publications.recordIssue(
        context, herald, "2026-09", "/tmp/sep2.bin", "VLD0:sep2", "22".repeat(32),
    )
    ok(
        "an ambiguous period is not guessed at",
        Publications.wantedTarget(context, stranger, "2026-09") == null,
    )

    // ...unless the asker's own subscription disambiguates it. The reader
    // subscribes to the Gazette only, so their ask is not ambiguous at all.
    ok(
        "a subscription disambiguates what a stranger's ask cannot",
        Publications.wantedTarget(context, reader, "2026-09") == gazette,
    )

    // Subscribed to both: back to ambiguous, and still not guessed.
    Publications.setSubscriber(context, herald, reader, true)
    ok(
        "subscribing to both is ambiguous again",
        Publications.wantedTarget(context, reader, "2026-09") == null,
    )
    Publications.setSubscriber(context, herald, reader, false)

    // Asking twice is not an error, and it is not a second sale either.
    // The publisher's own ledger says what was handed over — never the
    // asker's word — so a re-ask is answered from it rather than billed.
    // On a period only one publication holds, so this measures the
    // already-sent rule and not the ambiguity one above.
    Publications.recordIssue(
        context, gazette, "2026-10", "/tmp/oct.bin", "VLD0:oct", "33".repeat(32),
    )
    Publications.markSent(context, gazette, "2026-10", reader)
    ok(
        "a period already sent is not sold twice",
        Publications.wantedTarget(context, reader, "2026-10") == null,
    )
    // ...and that is per reader, not per period: the stranger has had
    // nothing, so their ask still stands.
    ok(
        "one reader's delivery does not close another's ask",
        Publications.wantedTarget(context, stranger, "2026-10") == gazette,
    )

    // A price changes what the answer *is*, never whether there is one:
    // free and priced both resolve, and the two paths diverge after here.
    Publications.setPrice(context, gazette, 2_000_000_000L)
    ok(
        "a priced publication resolves the same way",
        Publications.wantedTarget(context, "ee".repeat(32), "2026-10") == gazette,
    )

    println("WANTED ok")
}
