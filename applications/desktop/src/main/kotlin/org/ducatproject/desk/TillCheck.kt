package org.ducatproject.desk

import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File

/**
 * Read-only: scan a desk's wallet forward and print what it holds. The
 * one-shot answer to "did the till get paid?" without opening the window —
 * and the tail end of tilltest, which exits on the receipt rather than
 * waiting out stagenet's mining schedule.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("TILLCHECK_FAIL set DUCAT_DESK_STATE"),
    )
    check(dir.isDirectory) { "TILLCHECK_FAIL no desk state at $dir" }
    Unlock.orExit(dir)
    val context = DeskContext(dir)
    check(WalletStore(context).address() != null) { "TILLCHECK_FAIL no wallet here" }

    val node = NodeStore(context).lastGood() ?: run {
        val s = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(NodeStore(context).ownUrl()),
            "stagenet", 8_000u,
        )
        NodeStore(context).rememberLastGood(s.url)
        s.url
    }
    var steps = 0
    while (steps < 200 && Wallet.scanStep(context, node)) steps++
    val b = Wallet.balances(context)
    println(
        "TILLCHECK scanned=${b.scannedTo}/${b.tip} " +
            "spendable=${formatXmr(b.spendablePxmr)} locked=${formatXmr(b.lockedPxmr)} " +
            "outputs=${b.spendableOutputs}",
    )
}
