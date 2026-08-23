package org.ducatproject.desk

import org.ducatproject.ducat.RunningTab
import org.ducatproject.ducat.TabStore

/**
 * What is allowed to close somebody's tab.
 * `./gradlew :desktop:tabminor`.
 *
 * Matching admitted minor 0 — the wallet's main address — for every tab,
 * because bills that predate per-contact addressing named no address and were
 * paid there. But minor 0 is also where donations, top-ups and escrow payouts
 * land, and a payer names the amount in their notice, so an output that had
 * nothing to do with a bill could settle it: the customer gets a receipt, the
 * goods leave the counter, and nobody paid for them.
 *
 * A bill now records the subaddress it asked to be paid at. The cases below
 * are as much about what must still settle — a tab already on disk when this
 * shipped has no recorded minor and has to keep working.
 */
fun main() {
    fun tab(billedMinor: Int?) = RunningTab(
        id = "t1", origin = "bar", personaHex = "aa".repeat(32),
        openedAt = 0, lines = emptyList(), taxPxmr = null, state = "settled",
        billedMinor = billedMinor,
    )

    // A bill that named a subaddress is settled by that subaddress.
    check(TabStore.paidWhereBilled(tab(3), 3, 3)) {
        "TABMINOR_FAIL a payment to the billed address did not settle its tab"
    }
    // **The hole.** Money arriving on the merchant's main address — a
    // donation, an escrow release, a stranger paying a different bill.
    check(!TabStore.paidWhereBilled(tab(3), 3, 0)) {
        "TABMINOR_FAIL an output on the main address closed a tab billed elsewhere"
    }
    // Another customer's subaddress is another customer's money.
    check(!TabStore.paidWhereBilled(tab(3), 3, 4)) {
        "TABMINOR_FAIL an output on someone else's subaddress settled this tab"
    }
    // What the wallet would mint today does not override what was billed: a
    // tab is checked against the address the customer was actually given.
    check(!TabStore.paidWhereBilled(tab(3), 9, 9)) {
        "TABMINOR_FAIL a tab was settled by an address it never asked for"
    }

    // A tab settled before the field existed keeps the old rule, or updating
    // the app would strand every bill already waiting to be paid.
    check(TabStore.paidWhereBilled(tab(null), 3, 0)) {
        "TABMINOR_FAIL a legacy tab stopped accepting payment on the main address"
    }
    check(TabStore.paidWhereBilled(tab(null), 3, 3)) {
        "TABMINOR_FAIL a legacy tab stopped accepting its own per-contact address"
    }
    check(!TabStore.paidWhereBilled(tab(null), 3, 4)) {
        "TABMINOR_FAIL a legacy tab accepted a third party's subaddress"
    }
    // Legacy, and this contact never had a per-contact address at all: there
    // is nothing to check against, so anything of ours is admissible.
    check(TabStore.paidWhereBilled(tab(null), null, 7)) {
        "TABMINOR_FAIL a tab with no address to compare against refused everything"
    }

    // It has to survive being written down, or the strict rule silently
    // relaxes to the legacy one on the next restart — the worst outcome,
    // because nothing looks wrong.
    val round = RunningTab.from(tab(3).toJson())
    check(round.billedMinor == 3) {
        "TABMINOR_FAIL the billed subaddress did not survive a JSON round trip"
    }
    check(RunningTab.from(tab(null).toJson()).billedMinor == null) {
        "TABMINOR_FAIL an absent billed subaddress came back as something"
    }

    // And through the store, which is what reconcile actually reads.
    val dir = kotlin.io.path.createTempDirectory("ducat-tabminor").toFile()
    val store = TabStore(DeskContext(dir))
    val opened = store.open("aa".repeat(32), "bar")
    store.update(opened.copy(billedMinor = 5))
    check(store.get(opened.id)?.billedMinor == 5) {
        "TABMINOR_FAIL the store lost the billed subaddress"
    }
    // And a tab that never recorded one still reads back as not having one,
    // rather than as minor 0 — which is the value that would quietly restore
    // the old permissive behaviour under a strict-looking check.
    val plain = store.open("bb".repeat(32), "bar")
    check(store.get(plain.id)?.billedMinor == null) {
        "TABMINOR_FAIL an unbilled tab came back with a subaddress"
    }

    println(
        "TABMINOR_OK billed=strict main=refused other=refused legacy=permissive " +
            "json=ok store=ok",
    )
}
