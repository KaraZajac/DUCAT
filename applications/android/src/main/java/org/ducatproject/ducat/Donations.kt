package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray

/**
 * The donation box's other half: the receipt (§15.11).
 *
 * The vendor rule already says why — a receipt SHOULD follow observed
 * settlement automatically, because the party who benefits from the record
 * arriving is the payer, and the payee is the only one who can issue it.
 * Donations are that rule's purest case: the record is the donor's tax
 * paperwork, and no bill exists to hang the machinery on. So this walks the
 * threads born from our own `donate` cards, matches their incoming payment
 * notices to money the wallet actually saw, and issues the kind-3 receipt
 * naming the donor's own notice (§16.14).
 *
 * The same three disciplines as the tab reconciler, for the same reasons:
 * the chain answer goes through [SecondOpinion] (unreachable settles,
 * unknown defers); the notice's amount is the donor's claim and the output
 * is the truth, so the receipt says what arrived; and the mark commits
 * BEFORE the receipt goes out, so a death in between costs a thank-you,
 * never a double one.
 */
object Donations {

    private const val TAG = "Donations"

    /** §16's grace for any cross-device timestamp comparison. */
    private const val CLOCK_SKEW_SECS = 900L

    /** Transactions already receipted — the claimed_kis of this loop. */
    private fun receipted(context: Context): Set<String> {
        val arr = JSONArray(
            securePrefs(context, "ducat_contacts")
                .getString("donation_receipted", "[]"),
        )
        return (0 until arr.length()).map { arr.getString(it) }.toSet()
    }

    /**
     * How many receipted donations to remember.
     *
     * This is the only replay guard in the loop, and it is parsed on every
     * poll pass — so it cannot grow without end: a busy donation box would
     * make each pass re-parse a list one entry longer than the last, for
     * ever. Newest kept, because a second receipt is only possible while the
     * donation is still one the reconciler looks at; a transaction thousands
     * of donations old is not coming back round.
     */
    private const val RECEIPTED_KEPT = 2_000

    private fun markReceipted(context: Context, txid: String) {
        val prefs = securePrefs(context, "ducat_contacts")
        val arr = JSONArray(prefs.getString("donation_receipted", "[]"))
        arr.put(txid)
        val bounded = if (arr.length() <= RECEIPTED_KEPT) arr else JSONArray().also { out ->
            for (i in arr.length() - RECEIPTED_KEPT until arr.length()) out.put(arr.getString(i))
        }
        prefs.edit().putString("donation_receipted", bounded.toString()).apply()
    }

    fun reconcile(context: Context) {
        val store = ContactStore(context)
        // OUR card, their claim — the issuer's side only. The claimant-side
        // field receipted backwards on the first live run: a donor who
        // claimed a charity's card started thanking the charity for the
        // charity's own old payments.
        val donors = store.all().filter { it.myCardPurpose == "donate" }
        if (donors.isEmpty()) return
        // What the wallet actually holds, by the transaction that brought it.
        val ownTxids = WalletStore(context).ourTxids()
        val received = WalletStore(context).entries()
            .filter { it.txHashHex.isNotEmpty() }
            .filterNot { it.txHashHex.lowercase() in ownTxids }
            .groupBy { it.txHashHex.lowercase() }
        val done = receipted(context)
        for (donor in donors) {
            for (m in store.thread(donor.personaHex)) {
                // Their payment notice (§16.13, kind 2): advisory, verified
                // by finding the output it names. Unprompted only — a notice
                // answering a bill is a sale, and the sale machinery owns its
                // receipt — and never from before the donate relationship
                // existed, with §16's clock-skew grace on the comparison: the
                // timestamp is the donor's clock, not ours.
                if (m.outgoing || m.kind != 2) continue
                if (m.reSeq != null) continue
                if (m.timestamp < donor.myCardPurposeAt - CLOCK_SKEW_SECS) continue
                val txid = m.txidHex?.lowercase() ?: continue
                if (txid in done) continue
                val outs = received[txid] ?: continue
                val amount = outs.sumOf { it.amountPxmr }
                if (amount <= 0) continue
                if (!SecondOpinion.settles(context, txid)) continue
                // Mark first, thank second — a death in the gap costs the
                // donor a receipt they can ask for again, never two receipts
                // for one gift.
                markReceipted(context, txid)
                runCatching {
                    Mailbox.send(
                        context, donor,
                        context.getString(R.string.donate_receipt_note),
                        kind = 3, amountPxmr = amount,
                        txidHex = m.txidHex,
                        // §16.14: the notice this receipts lives in *their*
                        // log — we are answering it.
                        reSeq = m.seq, reOwn = false,
                    )
                }.onSuccess {
                    DucatLog.i(
                        TAG,
                        "donation receipted: ${formatXmr(amount)} XMR from ${donor.displayName()}",
                    )
                }.onFailure { DucatLog.w(TAG, "receipt: ${it.message}") }
            }
        }
    }
}
