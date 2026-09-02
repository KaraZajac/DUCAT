package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.RunningTab
import org.ducatproject.ducat.TabStore

/**
 * A receipt the reconciler could not send is owed, not forgotten.
 * `./gradlew :desktop:receiptowed`.
 *
 * The payment is marked before the receipt goes — a death between the two
 * must not match the output twice — and the send after the mark used to be
 * `runCatching { … }` with a log line in the failure arm. The Monero node
 * that found the payment says nothing about the Veilid node the receipt
 * needs, so the pair failed apart in the field: the till read "Receipt sent
 * to Sam" and the bar's book "Paid ✓ — receipt sent" over a receipt Sam
 * never got, and nothing anywhere would ever send it.
 *
 * Now the tab records the debt in the same word field a cancellation or a
 * cash receipt uses, the screens read it, and the poll pays it off. What
 * this pins is the bookkeeping around that retry, with no network: a retry
 * that fails must leave the debt standing (a cleared one is the old lie
 * again), must not write a row it did not send (the poll would deliver a
 * receipt the tab does not know about, and the next retry a second one),
 * and a tab whose contact is gone must stop asking.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-receipt").toFile()
    val ctx = DeskContext(dir)
    val contacts = ContactStore(ctx)
    val store = TabStore(ctx)

    val sam = "aa".repeat(32)
    contacts.add(Contact(sam, null, "Sam", myOutbox = "m1", theirOutbox = "t1"))

    // A sale paid on chain, tipped, whose receipt never left: the state
    // reconcile leaves behind when the send after the mark fails.
    val owed = store.open(sam, "pos").let { t ->
        store.mutate(t.id) {
            it.copy(
                state = "paid", lines = listOf(BillItem("Flat white", 100)),
                settledTotal = 100, paidPxmr = 120, paidKi = "ki-1",
                wordSeq = RunningTab.WORD_UNSENT,
            )
        }!!
    }
    // The debt survives the disk — every screen reads the tab back through
    // JSON, and a sentinel that round-trips to "unknown" would read as sent.
    check(RunningTab.from(owed.toJson()).wordSeq == RunningTab.WORD_UNSENT) {
        "RECEIPT_FAIL the debt did not survive the round trip"
    }
    check(store.get(owed.id)!!.wordSeq == RunningTab.WORD_UNSENT) {
        "RECEIPT_FAIL the debt did not survive the store"
    }

    // No node: the retry fails before any row exists, and the tab still
    // owes the word. Nothing may have been written to the thread — a row
    // here is a receipt the poll would deliver behind the tab's back.
    val retry = runCatching { store.sendChainReceipt(store.get(owed.id)!!) }
    check(retry.isFailure) { "RECEIPT_FAIL a receipt went out with no node" }
    check(store.get(owed.id)!!.wordSeq == RunningTab.WORD_UNSENT) {
        "RECEIPT_FAIL a failed retry cleared the debt: ${store.get(owed.id)!!.wordSeq}"
    }
    check(contacts.thread(sam).none { it.outgoing && it.kind == 3 }) {
        "RECEIPT_FAIL a failed retry committed a row"
    }
    check(store.get(owed.id)!!.state == "paid") { "RECEIPT_FAIL the payment moved" }

    // The poll's pass has other work: it swallows the failure and leaves
    // the tab exactly as it found it.
    TabStore.sendOwedReceipts(ctx)
    check(store.get(owed.id)!!.wordSeq == RunningTab.WORD_UNSENT) {
        "RECEIPT_FAIL the poll's pass changed a tab it could not pay off"
    }

    // A tab that owes nothing is not touched: one whose receipt is on its
    // way (a row it can name), one closed before the word was kept, and a
    // billed one still waiting for its payment.
    for ((state, word) in listOf("paid" to 7L, "paid" to -1L, "settled" to RunningTab.WORD_UNSENT)) {
        val t = store.open(sam, "pos")
        store.mutate(t.id) { it.copy(state = state, settledTotal = 100, wordSeq = word) }
        val after = store.sendChainReceipt(store.get(t.id)!!)!!
        check(after.wordSeq == word && after.state == state) {
            "RECEIPT_FAIL a $state tab with word $word was touched: ${after.wordSeq}/${after.state}"
        }
    }
    check(contacts.thread(sam).none { it.outgoing }) { "RECEIPT_FAIL something was sent" }

    // Nobody to tell: the contact was forgotten under the tab. The word
    // goes to unknown, as close() records it, so the poll stops asking on
    // every pass for the rest of time.
    val jo = "bb".repeat(32)
    val orphan = store.open(jo, "pos")
    store.mutate(orphan.id) {
        it.copy(state = "paid", settledTotal = 100, paidPxmr = 100, wordSeq = RunningTab.WORD_UNSENT)
    }
    val settledDebt = store.sendChainReceipt(store.get(orphan.id)!!)!!
    check(settledDebt.wordSeq == -1L) {
        "RECEIPT_FAIL a tab with no contact keeps asking: ${settledDebt.wordSeq}"
    }
    check(store.all().none { it.state == "paid" && it.wordSeq == RunningTab.WORD_UNSENT && it.personaHex == jo }) {
        "RECEIPT_FAIL the orphan is still in the owed set"
    }

    println("RECEIPT_OK owed=kept rows=0 untouched=3 orphan=released")
}
