package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * Money through the gate, live: the whole subscription economy between two
 * stranger desks with no operator touch after setup.
 *
 * Publisher: card → roster → BILL through TabStore (the till's own rails)
 * → watch the chain → the tab goes "paid" → seed the issue (deliberately
 * only now: pay-then-ship must hold until the seed exists) → the poll
 * clock's reconcileSettled mails the period key + shipment. Markers:
 * PUBSETTLE_CARD, PUBSETTLE_BILLED, PUBSETTLE_PAID, PUBSETTLE_SEEDED,
 * PUBSETTLE_SENT.
 *
 * Subscriber: claim → the bill lands → wait for the bank's funding to
 * unlock → PAY it (Wallet.send + the §16.13 notice naming the bill) →
 * wait for the kind-13 → fetch the issue by swarm → PUBSETTLE_OK
 * <bytes> <secs> <blake3>. Markers: PUBSETTLE_BILL, PUBSETTLE_FUNDED,
 * PUBSETTLE_NOTICE.
 *
 *   DUCAT_PUB_ROLE=publish   DUCAT_PUB_STATE=<dir> DUCAT_PUB_FILE=<file>
 *   DUCAT_PUB_ROLE=subscribe DUCAT_PUB_STATE=<dir> DUCAT_PUB_CARD=<uri>
 *     DUCAT_PUB_OUT=<dir>
 */

private const val PERIOD = "2026-09"
private const val PRICE_PXMR = 500_000_000L // 0.0005 XMR

private fun ready(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "PUBSETTLE_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

private fun wallet(context: DeskContext) {
    if (WalletStore(context).address() == null) {
        val tip = runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            ).height
        }.getOrDefault(0uL)
        val w = uniffi.ducat_mobile.createWallet(tipHeight = tip, stagenet = true)
        WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
    }
}

private fun node(context: DeskContext): String? =
    NodeStore(context).lastGood() ?: runCatching {
        val s = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(NodeStore(context).ownUrl()),
            "stagenet", 8_000u,
        )
        NodeStore(context).rememberLastGood(s.url)
        s.url
    }.getOrNull()

private fun scan(context: DeskContext) {
    node(context)?.let { n ->
        var steps = 0
        while (steps < 3 && Wallet.scanStep(context, n)) steps++
    }
}

fun main() {
    val role = System.getenv("DUCAT_PUB_ROLE") ?: error("PUBSETTLE_FAIL set DUCAT_PUB_ROLE")
    val state = System.getenv("DUCAT_PUB_STATE") ?: error("PUBSETTLE_FAIL set DUCAT_PUB_STATE")
    val dir = File(state).apply { mkdirs() }

    when (role) {
        "publish" -> {
            val file = System.getenv("DUCAT_PUB_FILE") ?: error("PUBSETTLE_FAIL set DUCAT_PUB_FILE")
            val context = ready(dir)
            NameStore(context).get() ?: NameStore(context).put("The Gazette Desk")
            wallet(context)

            val card = Mailbox.issueCard(context, "The Gazette Desk", 60uL * 60uL)
            println("PUBSETTLE_CARD ${card.uri}")
            System.out.flush()

            // Reuse across restarts: the ledger, the tab and the cabinet are
            // all durable, and a second create() would orphan the billed
            // history under a fresh publication id.
            val pubId = Publications.publications(context)
                .firstOrNull { it.second == "The Settled Gazette" }?.first
                ?: Publications.create(context, "The Settled Gazette")
            Publications.setPrice(context, pubId, PRICE_PXMR)

            var billed = Publications.billedFor(context, pubId, PERIOD).isNotEmpty()
            var paidSeen = false
            var seeded = false
            val start = System.currentTimeMillis()
            while (System.currentTimeMillis() - start < 45 * 60_000L) {
                runCatching { Mailbox.collectClaims(context) }
                runCatching { Mailbox.poll(context) }

                val reader = ContactStore(context).all().firstOrNull()
                if (reader != null && !billed) {
                    Publications.setSubscriber(context, pubId, reader.personaHex, true)
                    val n = Publications.billPeriod(context, pubId, PERIOD)
                    if (n == 1) {
                        billed = true
                        println("PUBSETTLE_BILLED ${formatXmr(PRICE_PXMR)} XMR to ${reader.displayName()}")
                        System.out.flush()
                    }
                }

                if (billed) {
                    runCatching { scan(context) }
                    // The exact lines the desk window's tick runs — the test
                    // proves the wiring, not a private copy of it.
                    runCatching { TabStore.reconcile(context) }
                    runCatching { Publications.reconcileSettled(context) }

                    val tabId = Publications.billedFor(context, pubId, PERIOD).values.firstOrNull()
                    val paid = tabId?.let { TabStore(context).get(it)?.state?.startsWith("paid") } == true
                    if (paid && !paidSeen) {
                        paidSeen = true
                        println("PUBSETTLE_PAID the chain says so")
                        System.out.flush()
                    }
                    // Pay-then-ship, proven in the awkward order: the seed
                    // happens only after the money, and the reconcile above
                    // has been running the whole time with nothing to send.
                    if (paidSeen && !seeded) {
                        val share = Swarm.seed(file)
                        Publications.recordIssue(
                            context, pubId, PERIOD, file,
                            share.shareKey, share.indexDigestHex,
                        )
                        seeded = true
                        println("PUBSETTLE_SEEDED ${share.shareKey.take(24)}…")
                        System.out.flush()
                    }
                    if (seeded) {
                        val sent = Publications.issues(context, pubId)
                            .firstOrNull { it.periodId == PERIOD }?.sentTo ?: emptySet()
                        if (sent.isNotEmpty()) {
                            println("PUBSETTLE_SENT by reconcile, serving until killed")
                            System.out.flush()
                            while (true) {
                                runCatching { Mailbox.poll(context) }
                                Thread.sleep(5_000)
                            }
                        }
                    }
                }
                Thread.sleep(3_000)
            }
            error("PUBSETTLE_FAIL publisher timed out (billed=$billed paid=$paidSeen seeded=$seeded)")
        }
        "subscribe" -> {
            val cardUri = System.getenv("DUCAT_PUB_CARD") ?: error("PUBSETTLE_FAIL set DUCAT_PUB_CARD")
            val out = System.getenv("DUCAT_PUB_OUT") ?: error("PUBSETTLE_FAIL set DUCAT_PUB_OUT")
            File(out).mkdirs()
            val context = ready(dir)
            NameStore(context).get() ?: NameStore(context).put("Reader")
            wallet(context)

            val scanned = uniffi.ducat_mobile.readContactCard(cardUri)
            val publisher = Mailbox.claimCard(context, scanned, "the gazette")
            System.err.println("claimed ${publisher.personaHex.take(8)}…")

            var billSeq: Long? = null
            var billAmount = 0L
            var billPayto: String? = null
            var paidTx: String? = null
            val start = System.currentTimeMillis()
            while (System.currentTimeMillis() - start < 45 * 60_000L) {
                runCatching { Mailbox.poll(context) }
                val store = ContactStore(context)
                val thread = store.thread(publisher.personaHex)

                if (billSeq == null) {
                    thread.firstOrNull { !it.outgoing && it.kind == 1 }?.let { bill ->
                        billSeq = bill.seq
                        billAmount = bill.amountPxmr
                        billPayto = bill.payto
                        println("PUBSETTLE_BILL ${formatXmr(bill.amountPxmr)} XMR seq=${bill.seq}")
                        System.out.flush()
                        // Restart safety: our own outgoing notice answering
                        // this bill is durable proof the money already left —
                        // without this, a relaunched subscriber pays twice.
                        thread.firstOrNull {
                            it.outgoing && it.kind == 2 && it.reSeq == bill.seq
                        }?.let { prior ->
                            paidTx = prior.txidHex ?: ""
                            System.err.println("already paid (${prior.txidHex?.take(12)}…)")
                        }
                    }
                }

                if (billSeq != null && paidTx == null) {
                    runCatching { scan(context) }
                    val b = Wallet.balances(context)
                    val fee = runCatching {
                        Wallet.feeFor(context, b.spendableOutputs.coerceAtLeast(1), 1)
                    }.getOrDefault(0L)
                    if (b.spendablePxmr >= billAmount + fee * 2 && fee > 0) {
                        println("PUBSETTLE_FUNDED spendable=${formatXmr(b.spendablePxmr)}")
                        System.out.flush()
                        val n = node(context) ?: error("PUBSETTLE_FAIL no monero node")
                        val to = billPayto ?: error("PUBSETTLE_FAIL the bill named no address")
                        val res = Wallet.send(
                            context, n, to, billAmount,
                            contactHex = publisher.personaHex,
                            note = "subscription",
                        )
                        // §16.13's notice, exactly as PaySheet words it: the
                        // payment answers their bill and names its transaction.
                        val fresh = store.all().first { it.personaHex == publisher.personaHex }
                        runCatching {
                            Mailbox.send(
                                context, fresh, "subscription",
                                kind = 2, amountPxmr = billAmount,
                                reSeq = billSeq, reOwn = false,
                                txidHex = res.txidHex,
                            )
                        }
                        paidTx = res.txidHex
                        println("PUBSETTLE_NOTICE txid=${res.txidHex.take(12)}…")
                        System.out.flush()
                    } else if (b.spendablePxmr + b.lockedPxmr >= billAmount) {
                        System.err.println(
                            "funds seen, ${b.blocksToUnlock} block(s) to unlock " +
                                "(${formatXmr(b.lockedPxmr)} locked)",
                        )
                    }
                }

                if (paidTx != null) {
                    val ship = Publications.shipment(context, publisher.personaHex, PERIOD)
                    val key = Publications.subscription(context, publisher.personaHex)
                        ?.third?.get(PERIOD)
                    if (ship != null && key != null) {
                        System.err.println("manifest filed: key + shipment for $PERIOD")
                        val t0 = System.currentTimeMillis()
                        val bytes = Swarm.fetch(ship.first, ship.second, out)
                        val secs = (System.currentTimeMillis() - t0) / 1000.0
                        // Every piece already verified against the index
                        // digest on the way down — the engine's whole point;
                        // the runner byte-compares against the source file
                        // for the end-to-end fact no internal check can fake.
                        val fetched = File(out).walkTopDown().first { it.isFile }
                        println("PUBSETTLE_OK $bytes $secs ${fetched.absolutePath}")
                        return
                    }
                }
                Thread.sleep(3_000)
            }
            error("PUBSETTLE_FAIL subscriber timed out (bill=${billSeq != null} paid=${paidTx != null})")
        }
        else -> error("PUBSETTLE_FAIL unknown role $role")
    }
}
