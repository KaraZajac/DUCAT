package org.ducatproject.desk

import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Orders
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The counter, both sides of it, over the live network.
 *
 * The kiosk stopped showing a `monero:` code and started showing a card, and
 * the whole claim of that change is that the customer gets things a bare
 * address cannot give them: an itemised bill, a payment identified by the
 * transaction they name rather than guessed at from its amount, and a receipt
 * that lands beside it. None of that is provable by rendering a screen. So:
 * two headless clients, one real Veilid network, one real stagenet payment.
 *
 *   shop:     DUCAT_KIOSK_ROLE=shop     ./gradlew :desktop:kiosktest
 *   customer: DUCAT_KIOSK_ROLE=customer DUCAT_KIOSK_CARD=<uri> ./gradlew :desktop:kiosktest
 *
 * The shop prints KIOSK_CARD for the customer to be handed. Then
 * KIOSK_PAIRED, KIOSK_BILLED, KIOSK_NOTICE, KIOSK_PAID on one side, and
 * KIOSK_CLAIMED, KIOSK_BILL, KIOSK_SENT, KIOSK_RECEIPT on the other.
 *
 * The customer needs stagenet coin. Fund their state dir from the standing
 * bank first — see :desktop:payout.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("KIOSK_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val role = System.getenv("DUCAT_KIOSK_ROLE")?.takeIf { it.isNotEmpty() }
        ?: error("KIOSK_FAIL set DUCAT_KIOSK_ROLE=shop|customer")

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    NameStore(context).get() ?: NameStore(context).put(
        if (role == "shop") "Corner Café" else "A customer",
    )

    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 120_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "KIOSK_FAIL node never attached" }
    println("KIOSK_NODE ready")

    fun node(): String? = NodeStore(context).lastGood() ?: runCatching {
        val s = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
        )
        NodeStore(context).rememberLastGood(s.url); s.url
    }.getOrNull()

    if (WalletStore(context).address() == null) {
        val tip = runCatching { node()?.let { _ ->
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            ).height
        } }.getOrNull() ?: 0uL
        val w = uniffi.ducat_mobile.createWallet(tipHeight = tip, stagenet = true)
        WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
    }
    println("KIOSK_ADDR ${WalletStore(context).address()}")

    if (role == "shop") shop(context, ::node) else customer(context, ::node)
}

/** Two coffees, a card, a bill, and the money watched onto the chain. */
private fun shop(context: android.content.Context, node: () -> String?) {
    val basket = listOf(
        BillItem("Flat white", 300_000_000L),
        BillItem("Croissant", 200_000_000L),
    )
    // With the shop's rate on, the way the screen places it: tax computed on
    // the goods and handed to the order, so the bill a paired customer gets
    // carries it in the wire's own field — the same rail the bar tab uses.
    org.ducatproject.ducat.Tax.set(context, true, 825)
    val goods = basket.sumOf { it.amountPxmr }
    val order = Orders.begin(context, basket, org.ducatproject.ducat.Tax.on(context, goods))
    println("KIOSK_ORDER #${order.number} ${formatXmr(order.totalPxmr)} XMR")
    check(order.unpaired) { "KIOSK_FAIL a begun order should be unpaired" }
    check(order.taxPxmr != null && order.totalPxmr == goods + order.taxPxmr!!) {
        "KIOSK_FAIL the order's total does not include its tax"
    }

    val card = Mailbox.issueCard(
        context, NameStore(context).get(), 7_200uL, purpose = "sale",
    )
    println("KIOSK_CARD ${card.uri}")

    var bound: Orders.Order? = null
    val start = System.currentTimeMillis()
    while (System.currentTimeMillis() - start < 20 * 60_000L) {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }

        if (bound == null) {
            val who = runCatching { ContactStore(context).claimantOf(card.inboxKey) }.getOrNull()
            if (who != null) {
                val name = ContactStore(context).all()
                    .firstOrNull { it.personaHex == who }?.displayName() ?: who.take(8)
                println("KIOSK_PAIRED $name")
                bound = Orders.bind(context, order, who)
                println("KIOSK_BILLED tab=${bound.tabId?.take(8)} ${formatXmr(bound.totalPxmr)} XMR")
                // The bill is a real message in a real thread, itemised —
                // which is the thing a `monero:` code could never be.
                val billed = TabStore(context).get(bound.tabId!!)!!
                check(billed.lines.size == 2) { "KIOSK_FAIL the bill lost its lines" }
                check(billed.state == "settled") { "KIOSK_FAIL bill not settled" }
                check(billed.taxPxmr == order.taxPxmr) {
                    "KIOSK_FAIL the tab dropped the order's tax"
                }
                check(bound.totalPxmr == goods + order.taxPxmr!!) {
                    "KIOSK_FAIL the billed total is not goods plus tax"
                }
            }
        }

        bound?.let { o ->
            // The customer names the transaction; we never have to guess.
            ContactStore(context).thread(o.personaHex!!)
                .firstOrNull { !it.outgoing && it.kind == 2 }?.let {
                    println("KIOSK_NOTICE txid=${it.txidHex?.take(12)} amount=${it.amountPxmr}")
                }
            runCatching {
                node()?.let { url ->
                    var steps = 0
                    while (steps < 3 && Wallet.scanStep(context, url)) steps++
                    TabStore.poolSight(context, url)
                }
                TabStore.reconcile(context)
            }.onFailure { println("KIOSK_WARN scan: ${it.message}") }

            when (Orders.stateOf(context, o)) {
                Orders.State.Seen -> println("KIOSK_SEEN order #${o.number}")
                Orders.State.Confirmed -> {
                    println("KIOSK_PAID order #${o.number}")
                    // And the receipt the poller owed them.
                    val receipt = ContactStore(context).thread(o.personaHex!!)
                        .lastOrNull { it.outgoing && it.kind == 3 }
                    println(
                        if (receipt != null) "KIOSK_RECEIPTED ${formatXmr(receipt.amountPxmr ?: 0)} XMR"
                        else "KIOSK_WARN paid but no receipt sent yet",
                    )
                    println("KIOSK_DONE shop")
                    return
                }
                else -> {}
            }
        }
        Thread.sleep(5_000)
    }
    println("KIOSK_FAIL shop timed out")
    kotlin.system.exitProcess(1)
}

/**
 * A node picked afresh, ignoring the cached one — which, when this is
 * called, is the node that just timed out.
 */
private fun pickFresh(context: android.content.Context): String? = runCatching {
    val s = uniffi.ducat_mobile.moneroPickNode(
        uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
    )
    NodeStore(context).rememberLastGood(s.url)
    s.url
}.getOrNull()

/** Claim the card, read the bill, pay it, name the transaction, keep the receipt. */
private fun customer(context: android.content.Context, node: () -> String?) {
    val uri = System.getenv("DUCAT_KIOSK_CARD")?.takeIf { it.isNotEmpty() }
        ?: error("KIOSK_FAIL set DUCAT_KIOSK_CARD to the shop's card")

    // Catch up first: a wallet that does not know it has money cannot pay.
    runCatching {
        node()?.let { url ->
            var steps = 0
            while (steps < 400 && Wallet.scanStep(context, url)) steps++
            Wallet.refreshSpent(context, url)
        }
    }
    runCatching { node()?.let { Wallet.refreshSpent(context, it) } }
    val funds = Wallet.balances(context)
    println("KIOSK_FUNDS spendable=${formatXmr(funds.spendablePxmr)} XMR")
    // With no coin the payment leg cannot run, but the half this change is
    // actually about still can: that a card becomes a conversation and a bill
    // arrives in it, itemised. Saying so beats refusing to start, because a
    // dry test bank is a funding problem and not a regression.
    val canPay = funds.spendablePxmr > 600_000_000L
    if (!canPay) {
        println(
            "KIOSK_NOFUNDS pairing and the bill only — fund " +
                "${WalletStore(context).address()} to test the payment",
        )
    }

    // Claim-once (§16.10): a restart must recognise the contact it already
    // made rather than asking for a second claim the protocol is right to
    // refuse.
    val scanned = uniffi.ducat_mobile.readContactCard(uri)
    val shopHex = scanned.persona.joinToString("") { "%02x".format(it) }
    val shop = ContactStore(context).all().firstOrNull { it.personaHex == shopHex }
        ?: Mailbox.claimCard(context, scanned, null)
    println("KIOSK_CLAIMED ${shop.displayName()}")

    // Already paid on an earlier run? Then do not pay again. A card is
    // claim-once but a bill is not, and a harness that re-sends money every
    // time it is restarted is a harness nobody can safely re-run.
    var paid = ContactStore(context).thread(shop.personaHex)
        .any { it.outgoing && it.kind == 2 }
    if (paid) println("KIOSK_ALREADY_PAID picking up where the last run left off")
    val start = System.currentTimeMillis()
    while (System.currentTimeMillis() - start < 20 * 60_000L) {
        runCatching { Mailbox.poll(context) }
        val thread = ContactStore(context).thread(shop.personaHex)

        if (!paid) {
            thread.firstOrNull { !it.outgoing && it.kind == 1 }?.let { bill ->
                // Itemised, which is the point of the exercise.
                println(
                    "KIOSK_BILL ${formatXmr(bill.amountPxmr ?: 0)} XMR — " +
                        bill.items.joinToString(", ") {
                            "${it.description} ${formatXmr(it.amountPxmr)}"
                        },
                )
                check(bill.items.isNotEmpty()) { "KIOSK_FAIL the bill had no lines" }
                val to = bill.payto ?: error("KIOSK_FAIL the bill named no address")
                check(bill.items.sumOf { it.amountPxmr } == bill.amountPxmr) {
                    "KIOSK_FAIL the lines do not sum to the total"
                }
                println("KIOSK_BILL_OK itemised, sums, and names an address")
                if (!canPay) { println("KIOSK_DONE customer (unfunded)"); return }
                // Building a transaction fetches decoys, which is a dozen
                // round trips to a stranger's node — and stagenet nodes drop
                // them. That is transient, not a refusal, so try another node
                // rather than failing the run on somebody else's timeout.
                var res: uniffi.ducat_mobile.SendResult? = null
                var lastWhy = ""
                for (attempt in 1..5) {
                    val url = pickFresh(context) ?: error("KIOSK_FAIL no monero node")
                    val r = runCatching {
                        Wallet.send(
                            context, url, to, bill.amountPxmr!!,
                            contactHex = shop.personaHex, note = "Order",
                        )
                    }
                    r.getOrNull()?.let { res = it }
                    if (res != null) break
                    val why = r.exceptionOrNull()!!
                    lastWhy = why.message.orEmpty()
                    if (!Wallet.isNodeTrouble(why)) throw why
                    println("KIOSK_RETRY $url — $lastWhy")
                    Thread.sleep(5_000)
                }
                val sentRes = res ?: error("KIOSK_FAIL could not build a payment — $lastWhy")
                println("KIOSK_SENT ${sentRes.txidHex.take(16)}…")
                // §16.13: name the transaction, or they are back to guessing.
                Mailbox.send(
                    context, ContactStore(context).all().first { it.personaHex == shop.personaHex },
                    "Order",
                    kind = 2, amountPxmr = bill.amountPxmr, txidHex = sentRes.txidHex,
                )
                paid = true
            }
        }

        if (paid) {
            thread.firstOrNull { !it.outgoing && it.kind == 3 }?.let {
                println("KIOSK_RECEIPT ${formatXmr(it.amountPxmr ?: 0)} XMR — ${it.body.take(60)}")
                // The last claim, and the one the whole change was for: on
                // the customer's own Activity, this receipt is *attached to
                // the payment* rather than sitting in a thread beside it —
                // with the lines it paid for, which is the thing a bare
                // address could never have given them.
                val paperwork = org.ducatproject.ducat.Ledger.build(context)
                    .firstOrNull { e -> e.txid.equals(it.txidHex ?: "", ignoreCase = true) }
                if (paperwork == null) {
                    println("KIOSK_FAIL the receipt names a payment Activity does not show")
                    kotlin.system.exitProcess(1)
                }
                println(
                    "KIOSK_ACTIVITY receipted=${paperwork.receipted} " +
                        "lines=${paperwork.items.size} " +
                        "from=${paperwork.receiptBy ?: "?"}",
                )
                check(paperwork.receipted) { "KIOSK_FAIL the payment shows no receipt" }
                check(paperwork.items.isNotEmpty()) {
                    "KIOSK_FAIL the payment shows no lines"
                }
                println("KIOSK_DONE customer")
                return
            }
            runCatching { node()?.let { url -> Wallet.scanStep(context, url) } }
        }
        Thread.sleep(5_000)
    }
    println("KIOSK_FAIL customer timed out")
    kotlin.system.exitProcess(1)
}
