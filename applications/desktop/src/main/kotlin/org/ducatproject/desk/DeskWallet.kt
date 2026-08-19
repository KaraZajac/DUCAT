package org.ducatproject.desk

import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File

/**
 * What this desk's wallet is and what is in it — creating one if there is none.
 *
 * The other half of `payout`. Testing needs a **standing address**: somewhere
 * to send stagenet coin that will still be there next week, that no `pm clear`
 * on an emulator can wipe, and that any role can be topped up from. A desk
 * state in a durable directory is that, and this prints its address so it can
 * be handed to whoever is funding.
 *
 *   DUCAT_DESK_STATE=~/.ducat-stagenet-bank ./gradlew :desktop:wallet
 *
 * Set DUCAT_WALLET_SCAN=1 to catch up and report the balance too; without it
 * this only reads what is stored, which is the fast answer when all you want
 * is the address.
 *
 * The wallet is created at the chain's **current tip**, not at genesis: a
 * wallet that does not know its own birthday scans from the beginning, which
 * is a day and a half of reading that looks exactly like having no money.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("WALLET_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    val store = WalletStore(context)

    if (store.address() == null) {
        val picked = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
        )
        NodeStore(context).rememberLastGood(picked.url)
        val w = uniffi.ducat_mobile.createWallet(tipHeight = picked.height, stagenet = true)
        store.save(w.address, w.spendKeyHex, w.restoreHeight, stagenet = true)
        println("WALLET_CREATED at height ${w.restoreHeight} via ${picked.url}")
    }

    println("WALLET_DIR ${dir.absolutePath}")
    println("WALLET_ADDRESS ${store.address()}")

    if (System.getenv("DUCAT_WALLET_SCAN") == "1") {
        val node = NodeStore(context).lastGood()
            ?: uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            ).url.also { NodeStore(context).rememberLastGood(it) }
        var steps = 0
        while (Wallet.scanStep(context, node)) {
            steps++
            if (steps % 25 == 0) {
                val p = Wallet.balances(context)
                println("WALLET_SCAN ${p.scannedTo}/${p.tip}")
            }
        }
        val b = Wallet.balances(context)
        println(
            "WALLET_BALANCE spendable ${formatXmr(b.spendablePxmr)}, " +
                "on the way ${formatXmr(b.lockedPxmr)}, ${b.spendableOutputs} note(s), " +
                "scanned ${b.scannedTo}/${b.tip}",
        )
    }
    println("WALLET OK")
}
