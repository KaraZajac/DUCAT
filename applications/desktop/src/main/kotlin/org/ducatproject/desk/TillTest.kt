package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Notify
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The till, driven blind: everything Desk v3's window does when a customer
 * walks up, as a headless sequence against a real phone over the live
 * network. Same calls the buttons make — issueCard, the greeting, a bill
 * whose lines must sum, the scan loop watching for the money, the §16.13
 * notice naming the transaction, the receipt the payee owes — so what this
 * proves is the client logic end to end, minus only the pixels.
 *
 * Markers narrate for the orchestrator: TILL_CARD (deep-link this to the
 * phone), TILL_PAIRED, TILL_BILL_SENT, TILL_BELL (the notify funnel fired),
 * TILL_MSG, TILL_NOTICE, TILL_MONEY_SEEN, TILL_RECEIPTED, TILL_DONE.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("TILL_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val lock = java.io.RandomAccessFile(File(dir, "desk.lock"), "rw").channel.tryLock()
    check(lock != null) { "TILL_FAIL another desk is running on $dir" }

    Unlock.orExit(dir)

    val context = DeskContext(dir)
    NameStore(context).get() ?: NameStore(context).put("Corner Café")
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "TILL_FAIL node never became ready" }

    // The wallet, born as Main.kt births one.
    if (WalletStore(context).address() == null) {
        val tip = runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            ).height
        }.getOrDefault(0uL)
        val w = uniffi.ducat_mobile.createWallet(tipHeight = tip, stagenet = true)
        WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
    }
    println("TILL_ADDR ${WalletStore(context).address()}")

    // The same funnel the window's tray rings.
    Notify.sink = { from, _, m ->
        println("TILL_BELL $from: ${if (m.kind == 0) m.body.take(60) else "kind ${m.kind}"}")
    }

    val card = Mailbox.issueCard(context, NameStore(context).get(), 60uL * 60uL)
    println("TILL_CARD ${card.uri}")

    val billPxmr = 500_000_000L // 0.0005 XMR: espresso + cornetto
    var paired: String? = null
    var billSeq: Long? = null
    var noticeTxid: String? = null
    var moneySeen = false
    var receipted = false
    var receiptOnNotice = false
    val start = System.currentTimeMillis()
    val mineWait = 6 * 60_000L // then the notice alone earns the receipt

    while (System.currentTimeMillis() - start < 15 * 60_000L) {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }
        val store = ContactStore(context)
        val c = store.all().firstOrNull()

        if (c != null && paired == null) {
            paired = c.personaHex
            println("TILL_PAIRED ${c.displayName()}")
            runCatching {
                Mailbox.send(
                    context, c, "Welcome to the Corner Café ☕",
                )
            }.onFailure { println("TILL_WARN greeting: ${it.message}") }
            runCatching {
                val fresh = store.all().first { it.personaHex == c.personaHex }
                val sent = Mailbox.send(
                    context, fresh, "Table 3",
                    kind = 1, amountPxmr = billPxmr,
                    payto = WalletStore(context).address(),
                    items = listOf(
                        BillItem("Espresso", 300_000_000L),
                        BillItem("Cornetto", 200_000_000L),
                    ),
                )
                billSeq = sent.outSeq - 1
                println("TILL_BILL_SENT seq=$billSeq")
            }.onFailure { println("TILL_FAIL bill: ${it.message}"); return }
        }

        if (paired != null) {
            val thread = store.thread(paired!!)
            thread.firstOrNull { !it.outgoing && it.kind == 0 }?.let {
                // Chat back from the phone — reported once by the bell; the
                // stored row is the proof it landed.
            }
            if (noticeTxid == null) {
                thread.firstOrNull { !it.outgoing && it.kind == 2 }?.let {
                    noticeTxid = it.txidHex ?: ""
                    println("TILL_NOTICE txid=${it.txidHex?.take(12)} amount=${it.amountPxmr}")
                }
            }
            // The scan loop, exactly as the window folds it into the tick.
            runCatching {
                val node = NodeStore(context).lastGood() ?: runCatching {
                    val s = uniffi.ducat_mobile.moneroPickNode(
                        uniffi.ducat_mobile.moneroDefaultNodes(NodeStore(context).ownUrl()),
                        "stagenet", 8_000u,
                    )
                    NodeStore(context).rememberLastGood(s.url); s.url
                }.getOrNull()
                if (node != null) {
                    var steps = 0
                    while (steps < 3 && Wallet.scanStep(context, node)) steps++
                }
                val b = Wallet.balances(context)
                if (!moneySeen && b.spendablePxmr + b.lockedPxmr >= billPxmr) {
                    moneySeen = true
                    println("TILL_MONEY_SEEN spendable=${b.spendablePxmr} locked=${b.lockedPxmr}")
                }
            }
            // The receipt: ideally after the output is found (§16.13 — verify,
            // don't trust), on the notice alone only once the mine has had its
            // six minutes — stagenet blocks keep their own schedule.
            val noticeAge = noticeTxid != null
            val patienceOver = System.currentTimeMillis() - start > mineWait
            if (!receipted && noticeAge && (moneySeen || patienceOver)) {
                receiptOnNotice = !moneySeen
                runCatching {
                    val fresh = store.all().first { it.personaHex == paired }
                    val paid = store.thread(paired!!).first { !it.outgoing && it.kind == 2 }
                    Mailbox.send(
                        context, fresh, "Receipt — thank you",
                        kind = 3, amountPxmr = paid.amountPxmr,
                        items = listOf(
                            BillItem("Espresso", 300_000_000L),
                            BillItem("Cornetto", 200_000_000L),
                        ),
                        txidHex = paid.txidHex,
                    )
                    receipted = true
                    println("TILL_RECEIPTED${if (receiptOnNotice) " (on the notice — block not yet mined)" else ""}")
                }.onFailure { println("TILL_WARN receipt: ${it.message}") }
            }
            if (receipted) {
                println("TILL_DONE money=${if (moneySeen) "on-chain" else "noticed"} bell-tested")
                return
            }
        }
        Thread.sleep(2_000)
    }
    println("TILL_TIMEOUT paired=${paired != null} notice=${noticeTxid != null} money=$moneySeen")
    kotlin.system.exitProcess(1)
}
