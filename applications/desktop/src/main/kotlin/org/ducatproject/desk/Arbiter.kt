package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The standing arbiter (§15.12): a desk that holds the third key in ride
 * escrows and, on the happy path, does nothing at all.
 *
 * It loops the same poll the phones run — the shared Mailbox dispatches
 * ceremony rounds into the shared Ceremony — so joining a 2-of-3 build is
 * not arbiter code, it is the ordinary machinery running on a machine that
 * stays on. It never proposes and never co-signs a ride release (the
 * consent gate parks proposals, and nothing here approves them): a ruling
 * UI is future work, and until it exists this arbiter can only ever help
 * build keys, which is exactly the trust a dormant third party should need.
 *
 * DUCAT_DESK_STATE names the identity. `--issue` prints a fresh contact
 * card URI and exits — how a phone gets this arbiter into its contacts.
 */
fun main(args: Array<String>) {
    val base = System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotEmpty() }
        ?: "${System.getProperty("user.home")}/.local/share"
    val dir = File(System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() } ?: "$base/ducat-desk")
    check(dir.isDirectory) { "ARBITER_FAIL no desk state at $dir" }
    val lock = java.io.RandomAccessFile(File(dir, "desk.lock"), "rw").channel.tryLock()
    check(lock != null) { "ARBITER_FAIL another desk is running on $dir" }

    Unlock.orExit(dir)

    val context = DeskContext(dir)
    // `--name Marta` names the standing identity once; cards and the serving
    // line read it from then on. An unnamed arbiter serves fine — but a card
    // that says who it is gets flipped on with more confidence.
    args.indexOf("--name").takeIf { it >= 0 && it + 1 < args.size }?.let {
        NameStore(context).put(args[it + 1])
    }
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "ARBITER_FAIL node never became ready" }

    if (args.contains("--issue")) {
        val h = Mailbox.issueCard(context, NameStore(context).get(), 60uL * 60uL * 24uL)
        println("ARBITER_CARD ${h.uri}")
        // Fall through into serving: the card's claim needs this process to
        // collect it, and the ceremony that follows needs it running.
    }

    println("ARBITER_UP serving as ${NameStore(context).get() ?: "unnamed"} — ctrl-c to stop")
    // The ruling console (§9.3): a parked proposal is a ruling REQUEST — the
    // arbiter's co-signature is the ruling, and declining is simply never
    // signing. Approval arrives as a line in <state>/rulings.txt:
    //   approve <ceremony id prefix>
    // File-based so a human (or a policy process) sits between the request
    // and the signature — the judgment is exactly what must not be automated.
    val rulings = File(dir, "rulings.txt")
    // Which rulings have been printed, and the fingerprint of the proposal
    // each was printed about — so an approval written in answer to one
    // listing cannot sign a proposal that replaced it (M11).
    val actedOn = HashMap<String, String>()
    var lastStages = ""
    while (true) {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }
        // The phones do these on their poller; a desk has no poller. A
        // half-built escrow whose missing frame is *this* node's would
        // otherwise wait for one of the principals to notice, and the
        // records of deals that never happened would stay for ever — this
        // node had seven, two of them from builds that stalled in August.
        // sweep never touches a record holding a key share, which is every
        // escrow an arbiter is actually the arbiter of.
        runCatching { Ceremony.nudge(context) }
        runCatching { Ceremony.sweep(context) }
        val all = Ceremony.all(context)
        val stages = all.joinToString { "${it.optString("id").take(8)}=${it.optString("stage")}" }
        if (stages != lastStages) {
            println("ARBITER_CEREMONIES $stages")
            lastStages = stages
        }
        for (o in all.filter { it.optString("stage") == "release_pending" }) {
            val id = o.optString("id")
            // Printed once per *proposal*, not once per ceremony: a
            // counter-offer replaces the parked payload, and an arbiter that
            // had already announced the first one would never be shown the
            // second. The fingerprint is what changed.
            val nowDigest = o.optString("pendingDigest")
            if (actedOn[id] != nowDigest) {
                // What this node knows for itself, and plainly when that is
                // nothing. An arbiter does not fund and is not shown the
                // banner that scans, so `fundedPxmr` is normally absent —
                // and printing the sentinel made that read as an escrow
                // holding minus one picomonero, next to a claim it was
                // supposed to be checked against. Saying there is no second
                // opinion is the useful thing: it tells whoever is deciding
                // that the outputs below are the only check they have.
                val scanned = o.optLong("fundedPxmr", -1L)
                println(
                    "ARBITER_RULING_REQUESTED ${id.take(8)} " +
                        "riderBack=${o.optLong("pendingRiderBack", -1)} pXMR (claimed) " +
                        (if (scanned >= 0) "escrow=$scanned pXMR (this node's own scan)"
                         else "escrow=unknown (this node does not scan — judge on the outputs below)")
                )
                // Where the money actually goes, read out of the payload.
                //
                // The line above is the proposer's word for it, and an
                // arbiter is the one signer that cannot check its own share
                // against it — it has no share. Ruling on a claim while
                // unable to read the transaction is how a captured proposer
                // gets a human to authorise something nobody agreed to, so
                // the outputs go on the console beside the claim and the
                // person deciding can compare them.
                val payload = org.ducatproject.ducat.hexToBytes(o.optString("pendingPayload"))
                if (payload == null) {
                    println("  outputs: the parked payload is gone — do not approve")
                } else {
                    runCatching { Ceremony.readRelease(context, o, payload) }
                        .onSuccess { r ->
                            // Every term, not just the addresses. The residual
                            // output carries no amount on the wire — it takes
                            // `inputs − fixed − fee` — so the inputs and the
                            // fee are what make the second line a number at
                            // all, and a release spending less than the
                            // escrow holds is the partial sweep of M2.
                            println(
                                "  spends ${r.inputsTotalPxmr} pXMR across ${r.inputs} note(s), " +
                                    "fee ${r.feePxmr} pXMR",
                            )
                            for (d in r.outs) {
                                val whose = when (d.side) {
                                    Ceremony.Side.PAYER -> "the payer's refund address, as the escrow's own frame names it"
                                    Ceremony.Side.MINE -> "an address this node controls"
                                    Ceremony.Side.THEIRS -> "the address that party published to this node"
                                    Ceremony.Side.UNKNOWN -> "NOT CHECKABLE HERE — read it against what they told you"
                                }
                                println(
                                    "  pays ${d.address} — ${d.amountPxmr} pXMR" +
                                        (if (d.residual) " (residual; pays the fee)" else "") +
                                        " — $whose",
                                )
                            }
                            r.payerBackPxmr?.let {
                                println("  back to the payer, as the bytes state it: $it pXMR")
                            }
                            println("  fingerprint ${r.digest.take(16)}…")
                        }
                        .onFailure { println("  outputs: refused ($it) — do not approve") }
                }
                println(
                    "  (approve with: echo 'approve ${id.take(8)}' >> ${rulings.absolutePath})"
                )
                // Even when the payload could not be read: the request has
                // been announced, and repeating it every two seconds would
                // bury the console. An unreadable one is refused below —
                // `readRelease` throwing is what "do not approve" means.
                actedOn[id] = nowDigest
            }
        }
        if (rulings.isFile) {
            for (line in rulings.readLines().map { it.trim() }.filter { it.startsWith("approve ") }) {
                val prefix = line.removePrefix("approve ").trim()
                val target = all.firstOrNull {
                    it.optString("stage") == "release_pending" &&
                        it.optString("id").startsWith(prefix)
                } ?: continue
                val id = target.optString("id")
                // The fingerprint the console printed, handed back with the
                // approval. A counter-offer landing between the request and
                // the line in rulings.txt replaces the parked payload, and a
                // ruling written about the proposal that was read must not
                // sign the one that replaced it (M11).
                val shown = actedOn[id]
                if (shown.isNullOrEmpty()) {
                    println("ARBITER_RULING_FAILED ${id.take(8)}: no readable proposal was printed for this")
                    continue
                }
                runCatching { Ceremony.approveRideRelease(context, id, shownDigest = shown) }
                    .onSuccess { println("ARBITER_RULED ${id.take(8)} — co-signed; the proposer completes") }
                    .onFailure { println("ARBITER_RULING_FAILED ${id.take(8)}: ${it.message}") }
            }
            rulings.delete()
        }
        Thread.sleep(2_000)
    }
}
