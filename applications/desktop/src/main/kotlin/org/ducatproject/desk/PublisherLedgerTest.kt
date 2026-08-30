package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Publications

/**
 * The publisher's ledger, exercised without a network: roster membership,
 * the issue log, re-recording a period after a re-seed (fresh share, kept
 * sent-list), and the ordering the room renders. Pure store arithmetic —
 * the wire half is pubswarmtest's job.
 *
 *   DUCAT_LEDGER_STATE=<dir> ./gradlew :desktop:publedgertest
 */
fun main() {
    val state = System.getenv("DUCAT_LEDGER_STATE") ?: error("PUBLEDGER_FAIL set DUCAT_LEDGER_STATE")
    val context = DeskContext(File(state).apply { mkdirs() })

    val pub = Publications.create(context, "The Ledger Gazette")
    check(Publications.publications(context).any { it.first == pub && it.second == "The Ledger Gazette" }) {
        "PUBLEDGER_FAIL created publication not listed"
    }

    // Roster: in, out, idempotent.
    val alice = "aa".repeat(32)
    val bob = "bb".repeat(32)
    Publications.setSubscriber(context, pub, alice, true)
    Publications.setSubscriber(context, pub, bob, true)
    Publications.setSubscriber(context, pub, bob, true)
    check(Publications.subscribers(context, pub).toSet() == setOf(alice, bob)) {
        "PUBLEDGER_FAIL roster after adds"
    }
    Publications.setSubscriber(context, pub, bob, false)
    check(Publications.subscribers(context, pub) == listOf(alice)) {
        "PUBLEDGER_FAIL roster after remove"
    }

    // Issues: record, mark, order.
    check(Publications.recordIssue(context, pub, "2026-08", "/tmp/aug.bin", "VLD0:aug", "11".repeat(32))) {
        "PUBLEDGER_FAIL recordIssue refused"
    }
    Publications.recordIssue(context, pub, "2026-09", "/tmp/sep.bin", "VLD0:sep", "22".repeat(32))
    Publications.markSent(context, pub, "2026-08", alice)
    val issues = Publications.issues(context, pub)
    check(issues.map { it.periodId } == listOf("2026-09", "2026-08")) {
        "PUBLEDGER_FAIL order: ${issues.map { it.periodId }}"
    }
    check(issues.last().sentTo == setOf(alice)) { "PUBLEDGER_FAIL sent list" }

    // A re-seed replaces the shipment and keeps the sends that happened.
    Publications.recordIssue(context, pub, "2026-08", "/tmp/aug.bin", "VLD0:aug2", "33".repeat(32))
    val aug = Publications.issues(context, pub).first { it.periodId == "2026-08" }
    check(aug.swarmKey == "VLD0:aug2" && aug.sentTo == setOf(alice)) {
        "PUBLEDGER_FAIL re-record: key=${aug.swarmKey} sent=${aug.sentTo}"
    }

    // A second publication's ledger is its own.
    val other = Publications.create(context, "Other")
    check(Publications.subscribers(context, other).isEmpty() && Publications.issues(context, other).isEmpty()) {
        "PUBLEDGER_FAIL ledgers bleed between publications"
    }

    println("PUBLEDGER_OK ${Publications.issues(context, pub).size} issues, roster survives re-seed")
}
