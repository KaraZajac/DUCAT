package org.ducatproject.desk

import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File

/**
 * Send stagenet XMR from this desk's wallet to any address.
 *
 * `docs/field-day.md` tells whoever is testing to fund the wallets "from
 * research/monero-rs tools or any stagenet faucet", which is true and is also
 * the reason money ends up stranded: a desk that played a role in an old run
 * still holds its change, and nothing in the tree could move it. Test money
 * that cannot be swept is test money you have to ask a faucet for again.
 *
 * It scans first, because a desk that has been idle does not know what it
 * owns, and a wallet that has not caught up reports nothing spendable no
 * matter what the chain says.
 *
 *   DUCAT_DESK_STATE=/tmp/drv DUCAT_PAY_TO=5… DUCAT_PAY_XMR=0.0008 \
 *     ./gradlew :desktop:payout
 *
 * DUCAT_PAY_XMR may be `all`, which sends everything the fee leaves behind.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("PAY_FAIL set DUCAT_DESK_STATE"),
    )
    val to = System.getenv("DUCAT_PAY_TO")?.takeIf { it.isNotEmpty() }
        ?: error("PAY_FAIL set DUCAT_PAY_TO to a stagenet address")
    val want = System.getenv("DUCAT_PAY_XMR")?.takeIf { it.isNotEmpty() }
        ?: error("PAY_FAIL set DUCAT_PAY_XMR to an amount, or to all")

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    val store = WalletStore(context)
    val from = store.address() ?: error("PAY_FAIL this desk has no wallet")
    println("PAY_FROM $from")

    val node = NodeStore(context).lastGood()
        ?: uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
        ).url.also { NodeStore(context).rememberLastGood(it) }
    println("PAY_NODE $node")

    // Catch up before counting. scanStep returns false when there is nothing
    // left to read, so this stops on its own rather than after a fixed guess.
    var steps = 0
    while (Wallet.scanStep(context, node)) {
        steps++
        if (steps % 20 == 0) {
            val b = Wallet.balances(context)
            println("PAY_SCAN ${b.scannedTo}/${b.tip} — ${formatXmr(b.spendablePxmr)} XMR so far")
        }
    }
    val b = Wallet.balances(context)
    println(
        "PAY_BALANCE spendable ${formatXmr(b.spendablePxmr)}, " +
            "locked ${formatXmr(b.lockedPxmr)}, ${b.spendableOutputs} note(s)",
    )

    val amount = if (want.equals("all", ignoreCase = true)) {
        // Everything, less what the network will take for saying so — found by
        // asking rather than by arithmetic. The fee depends on which notes the
        // builder picks, and picking them depends on the amount, so a single
        // subtraction guesses wrong in the direction that fails: "not enough in
        // the notes you picked, once the fee is counted". Walk it down until
        // the quote says the whole thing fits.
        var amt = b.spendablePxmr
        var fits = 0L
        repeat(12) {
            val q = Wallet.quote(context, amt, 1)
            if (q.feePxmr <= 0) error("PAY_FAIL no fee estimate — is the node reachable?")
            if (q.affordable) { fits = amt; return@repeat }
            amt = b.spendablePxmr - q.feePxmr - q.feePxmr / 4
            if (amt <= 0) error("PAY_FAIL nothing to send once the fee is counted")
        }
        if (fits <= 0) error("PAY_FAIL could not find an amount the fee leaves room for")
        fits
    } else {
        (want.toBigDecimalOrNull() ?: error("PAY_FAIL $want is not an amount"))
            .multiply(java.math.BigDecimal(1_000_000_000_000L)).toLong()
    }
    println("PAY_SENDING ${formatXmr(amount)} XMR to ${to.take(12)}…")

    // The estimate is an estimate. `quote` prices the notes it would pick;
    // the builder prices the transaction it actually built, and when the two
    // disagree it is the builder that refuses — "not enough in the notes you
    // picked, once the fee is counted". Sweeping is the one case with no
    // headroom by definition, so back off and try again rather than making
    // the caller guess an amount that fits.
    var attempt = amount
    var sent: uniffi.ducat_mobile.SendResult? = null
    var lastWhy = ""
    repeat(6) {
        val r = runCatching { Wallet.send(context, node, to, attempt, note = "desk payout") }
        r.getOrNull()?.let { sent = it; return@repeat }
        lastWhy = r.exceptionOrNull()?.message.orEmpty()
        if (!lastWhy.contains("not enough")) error("PAY_FAIL $lastWhy")
        val back = Wallet.feeFor(context, b.spendableOutputs.coerceAtLeast(1), 1) / 2
        attempt -= back.coerceAtLeast(1_000_000L)
        if (attempt <= 0) error("PAY_FAIL nothing left once the fee is counted")
        println("PAY_RETRY ${formatXmr(attempt)} XMR — $lastWhy")
    }
    val r = sent ?: error("PAY_FAIL gave up: $lastWhy")
    println(
        "PAY_SENT ${r.txidHex} fee ${formatXmr(r.feePxmr.toLong())} XMR, " +
            "accepted by ${r.acceptedBy} node(s)",
    )
    println("PAYOUT OK")
}
