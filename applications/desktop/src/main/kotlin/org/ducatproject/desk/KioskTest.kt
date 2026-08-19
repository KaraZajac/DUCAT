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
    val order = Orders.begin(context, basket)
    println("KIOSK_ORDER #${order.number} ${formatXmr(order.totalPxmr)} XMR")
    check(order.unpaired) { "KIOSK_FAIL a begun order should be unpaired" }

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

    var paid = false
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
                val url = node() ?: error("KIOSK_FAIL no monero node")
                val res = Wallet.send(
                    context, url, to, bill.amountPxmr!!,
                    contactHex = shop.personaHex, note = "Order",
                )
                println("KIOSK_SENT ${res.txidHex.take(16)}…")
                // §16.13: name the transaction, or they are back to guessing.
                Mailbox.send(
                    context, ContactStore(context).all().first { it.personaHex == shop.personaHex },
                    "Order", PersonaStore(context).personaHex(),
                    kind = 2, amountPxmr = bill.amountPxmr, txidHex = res.txidHex,
                )
                paid = true
            }
        }

        if (paid) {
            thread.firstOrNull { !it.outgoing && it.kind == 3 }?.let {
                println("KIOSK_RECEIPT ${formatXmr(it.amountPxmr ?: 0)} XMR — ${it.body.take(60)}")
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
