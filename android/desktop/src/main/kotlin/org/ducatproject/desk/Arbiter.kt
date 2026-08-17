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
    val actedOn = HashSet<String>()
    var lastStages = ""
    while (true) {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }
        val all = Ceremony.all(context)
        val stages = all.joinToString { "${it.optString("id").take(8)}=${it.optString("stage")}" }
        if (stages != lastStages) {
            println("ARBITER_CEREMONIES $stages")
            lastStages = stages
        }
        for (o in all.filter { it.optString("stage") == "release_pending" }) {
            val id = o.optString("id")
            if (id !in actedOn) {
                println(
                    "ARBITER_RULING_REQUESTED ${id.take(8)} " +
                        "riderBack=${o.optLong("pendingRiderBack", -1)} pXMR " +
                        "(approve with: echo 'approve ${id.take(8)}' >> ${rulings.absolutePath})"
                )
                actedOn.add(id)
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
                runCatching { Ceremony.approveRideRelease(context, id) }
                    .onSuccess { println("ARBITER_RULED ${id.take(8)} — co-signed; the proposer completes") }
                    .onFailure { println("ARBITER_RULING_FAILED ${id.take(8)}: ${it.message}") }
            }
            rulings.delete()
        }
        Thread.sleep(2_000)
    }
}
