package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * The bonded ride, end to end, between two real clients — repeatably.
 *
 * A two-phone pass proves a thing happened once. What a ride escrow needs is
 * the other claim: that it happens *every* time, on a network that drops
 * messages and a chain that makes you wait ten blocks. So this is the same
 * arc driven headlessly by two processes over the live Veilid network and
 * live stagenet, using the same `Ceremony` the phones compile.
 *
 * Two roles, two directories, one script:
 *
 *   DUCAT_RIDE_ROLE=driver DUCAT_DESK_STATE=/tmp/drv ./gradlew :desktop:ridetest
 *   DUCAT_RIDE_ROLE=rider  DUCAT_DESK_STATE=/tmp/rdr \
 *     DUCAT_RIDE_CARD='ducat:card/…' ./gradlew :desktop:ridetest
 *
 * The arc, and who does what:
 *
 *   1. driver issues a card; rider claims it            (the meeting)
 *   2. rider starts the escrow: 2-of-2, no arbiter      (the agreement)
 *   3. both derive one address, holding one share each  (the ceremony)
 *   4. rider funds fare + margin; driver funds a stake  (the money in)
 *   5. each side confirms the pot by its *own* scan     (nobody's word)
 *   6. driver completes: proposes the split             (the work done)
 *   7. rider consents; driver broadcasts                (the money out)
 *   8. both read the chain for what they actually got   (the proof)
 *
 * Markers narrate for the orchestrator: RIDE_CARD, RIDE_PAIRED, RIDE_BUILT,
 * RIDE_FUNDED, RIDE_SECURED, RIDE_PROPOSED, RIDE_SIGNED, RIDE_RELEASED,
 * RIDE_PAID, RIDE_DONE — and RIDE_FAIL with a reason, always.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("RIDE_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val role = System.getenv("DUCAT_RIDE_ROLE")?.takeIf { it.isNotEmpty() }
        ?: error("RIDE_FAIL set DUCAT_RIDE_ROLE to rider or driver")
    // Small on purpose: this runs for real, and a test that costs a fortune
    // is a test nobody runs twice.
    val fare = System.getenv("DUCAT_RIDE_FARE")?.toLongOrNull() ?: 500_000_000L
    val stake = System.getenv("DUCAT_RIDE_STAKE")?.toLongOrNull() ?: 200_000_000L
    // A reservation is the same escrow with a third number in it: rent, the
    // guest's deposit and the host's, all in one pot (§16.18). Everything
    // downstream — who owes what, what the pot should hold, who proposes the
    // release and who signs it — already asks `Ceremony` rather than assuming
    // a ride, so only the opening move differs. `driver` plays the host, whose
    // funding of their own deposit *is* their acceptance.
    //
    //   DUCAT_RIDE_KIND=reservation DUCAT_RIDE_FARE=2000000000 \
    //   DUCAT_RIDE_GUESTDEP=600000000 DUCAT_RIDE_STAKE=600000000 …
    val reservation = System.getenv("DUCAT_RIDE_KIND") == "reservation"
    val guestDep = System.getenv("DUCAT_RIDE_GUESTDEP")?.toLongOrNull() ?: 600_000_000L

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    NameStore(context).get() ?: NameStore(context).put(role.replaceFirstChar { it.uppercase() })

    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "RIDE_FAIL node never became ready" }

    if (WalletStore(context).address() == null) {
        val tip = runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
            ).height
        }.getOrDefault(0uL)
        val w = uniffi.ducat_mobile.createWallet(tipHeight = tip, stagenet = true)
        WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
    }
    println("RIDE_WALLET ${WalletStore(context).address()}")

    /**
     * One turn of the service loop, as the phones run it: mail *and* the
     * wallet. Leaving the scan out was the first thing this harness got
     * wrong — a desk that never scans has no money no matter who paid it.
     */
    fun tick() {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }
        runCatching {
            val node = org.ducatproject.ducat.NodeStore(context).lastGood()
                ?: uniffi.ducat_mobile.moneroPickNode(
                    uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
                ).url.also { org.ducatproject.ducat.NodeStore(context).rememberLastGood(it) }
            var n = 0
            while (n < 8 && Wallet.scanStep(context, node)) n++
        }
    }

    /** Wait for a condition, ticking meanwhile. Null on timeout. */
    fun <T> await(what: String, seconds: Long = 600, f: () -> T?): T? {
        val until = System.currentTimeMillis() + seconds * 1000
        while (System.currentTimeMillis() < until) {
            tick()
            f()?.let { return it }
            Thread.sleep(3_000)
        }
        println("RIDE_FAIL timed out waiting for $what")
        return null
    }

    /**
     * Send this side's share once the wallet can actually spend it.
     *
     * A wallet that has just been paid holds nothing spendable for ten
     * blocks, and on stagenet that is up to forty minutes. Retrying is not
     * papering over a failure: it is the same wait the app shows as "still
     * locked", and a harness that gave up here would be testing the clock.
     */
    fun fundWhenAble(idHex: String, owed: Long): String? {
        var said = ""
        var fatal: String? = null
        val txid = await("enough unlocked to send ${formatXmr(owed)} XMR", seconds = 3600) {
            val r = runCatching { Ceremony.fundRide(context, idHex) }
            val ok = r.getOrNull()
            if (ok != null) {
                ok
            } else {
                val why = r.exceptionOrNull()?.message.orEmpty()
                if (why != said) {
                    said = why
                    val b = Wallet.balances(context)
                    println(
                        "RIDE_WAIT $why (spendable ${formatXmr(b.spendablePxmr)}, " +
                            "locked ${formatXmr(b.lockedPxmr)}, scanned ${b.scannedTo}/${b.tip})",
                    )
                }
                // Waiting for coin to unlock is not a failure, and neither is
                // a Monero node that did not answer — that one killed a run
                // mid-escrow with `decoys: InterfaceError("timed out")`, which
                // is the network having a moment, not the escrow being wrong.
                // The wallet picks a different node on the next attempt, which
                // is exactly what the app's own retry does.
                val transient = why.contains("not enough unlocked") ||
                    org.ducatproject.ducat.Wallet.isNodeTrouble(
                        r.exceptionOrNull() ?: IllegalStateException(why),
                    )
                if (!transient) fatal = why
                fatal
            }
        }
        fatal?.let { println("RIDE_FAIL could not fund: $it"); return null }
        return txid
    }

    /**
     * The escrow this pair built, at whatever stage it has reached.
     *
     * Waiting for stage == "done" exactly was wrong: a proposal moves both
     * sides past it, so a harness restarted after one would sit waiting for
     * a state the ceremony had already left. What "built" means is that an
     * address exists and this device holds a share.
     */
    fun builtRide(theirHex: String? = null): org.json.JSONObject? =
        Ceremony.all(context)
            .filter { it.optString("address").isNotEmpty() }
            .filter { it.optString("stage") !in listOf("shared", "committed") }
            .lastOrNull { o ->
                theirHex == null || o.optJSONArray("roster")?.let { r ->
                    (0 until r.length()).any { r.getString(it) == theirHex }
                } == true
            }

    /** This side's own scan of the escrow — never the other party's word. */
    fun potByOwnScan(idHex: String): Long =
        runCatching { Ceremony.checkRideFunding(context, idHex) }.getOrDefault(0L)

    val mine = PersonaStore(context).personaHex()

    if (role == "driver") {
        val card = Mailbox.issueCard(context, NameStore(context).get(), 60uL * 60uL)
        println("RIDE_CARD ${card.uri}")

        val rider = await("the rider to claim the card") {
            ContactStore(context).all().firstOrNull()
        } ?: return
        println("RIDE_PAIRED ${rider.displayName()}")

        // The rider starts the escrow; this side joins it from the mailbox.
        val ride = await("the escrow invitation", seconds = 900) {
            builtRide(rider.personaHex)
        } ?: return
        val id = ride.optString("id")
        println("RIDE_BUILT ${ride.optString("address")} (id ${id.take(8)})")

        // A two-sided ride: the driver's stake goes in beside the fare.
        val owed = Ceremony.mySharePxmr(ride)
        if (ride.optString("hostFundTxid").isNotEmpty()) {
            println("RIDE_FUNDED driver already staked — ${ride.optString("hostFundTxid").take(16)}…")
        } else if (owed > 0) {
            val tx = fundWhenAble(id, owed) ?: return
            println("RIDE_FUNDED driver staked ${formatXmr(owed)} XMR — ${tx.take(16)}…")
        } else {
            println("RIDE_FUNDED driver stakes nothing (one-sided ride)")
        }

        val want = Ceremony.expectedTotalPxmr(ride)
        await("the pot to reach ${formatXmr(want)} XMR", seconds = 1800) {
            potByOwnScan(id).takeIf { it >= want }
        } ?: return
        println("RIDE_SECURED driver's own scan sees ${formatXmr(want)} XMR")

        // The ride happens here. Then the driver asks to be paid: the
        // default split hands the rider's margin back and takes the rest.
        //
        // Monero will not let a young output be spent, and the engine says so
        // rather than broadcasting something the daemon would reject with an
        // empty reason (`invalid_input`, which cost a live evening once). On
        // stagenet that is up to forty minutes of waiting, so the proposal
        // retries rather than failing — which is exactly what the app's own
        // retry button does, and the reason it exists.
        // Always propose afresh, even when a proposal is already standing.
        // FROST's signing state lives in memory keyed by (ceremony, party)
        // and dies with the process, so a restarted driver cannot complete
        // the round it started last time — the co-signature would arrive for
        // a machine that no longer exists. A fresh proposal supersedes, with
        // fresh nonces, which is exactly what the app's retry button does.
        var said = ""
        var hardFailure: String? = null
        val proposed = await("the escrow's funding to mature", seconds = 3600) {
            val r = runCatching { Ceremony.proposeRideRelease(context, id) }
            if (r.isSuccess) {
                true
            } else {
                val why = r.exceptionOrNull()?.message.orEmpty()
                if (why != said) { said = why; println("RIDE_WAIT $why") }
                // Anything that is not the maturity rule is a real failure;
                // stop waiting for a clock that will not fix it.
                if (!why.contains("confirmation")) hardFailure = why
                if (hardFailure != null) true else null
            }
        }
        hardFailure?.let { println("RIDE_FAIL propose: $it"); return }
        if (proposed != true) return
        println("RIDE_PROPOSED driver asked for the fare")

        var txid: String? = null
        // Re-propose on a long silence rather than waiting forever: over a
        // network that drops messages, "ask again" is the whole recovery
        // strategy, and the ceremony is built to be asked again.
        for (attempt in 1..6) {
            txid = await("the rider to consent and the release to land", seconds = 600) {
                Ceremony.all(context).firstOrNull { it.optString("id") == id }
                    ?.optString("txid")?.takeIf { it.isNotEmpty() }
            }
            if (txid != null) break
            println("RIDE_RETRY no release yet — proposing again ($attempt)")
            runCatching { Ceremony.proposeRideRelease(context, id) }
                .onFailure { println("RIDE_WAIT re-propose: ${it.message}") }
        }
        if (txid == null) { println("RIDE_FAIL the release never landed"); return }
        println("RIDE_RELEASED $txid")

        // What the driver actually holds, by its own scan — the only figure
        // that settles the question.
        val before = Wallet.balances(context).let { it.spendablePxmr + it.lockedPxmr }
        await("the payout to appear in the driver's wallet", seconds = 1800) {
            val node = org.ducatproject.ducat.NodeStore(context).lastGood()
                ?: return@await null
            var steps = 0
            while (steps < 40 && Wallet.scanStep(context, node)) steps++
            val b = Wallet.balances(context)
            (b.spendablePxmr + b.lockedPxmr).takeIf { it > before || it >= fare }
        }
        val b = Wallet.balances(context)
        println("RIDE_PAID driver holds ${formatXmr(b.spendablePxmr + b.lockedPxmr)} XMR")
        println("RIDE_DONE driver")
        return
    }

    // ---- rider ----
    val cardUri = System.getenv("DUCAT_RIDE_CARD")?.takeIf { it.isNotEmpty() }
        ?: error("RIDE_FAIL set DUCAT_RIDE_CARD to the driver's card")
    val scanned = uniffi.ducat_mobile.readContactCard(cardUri)
    val theirHex = scanned.persona.joinToString("") { "%02x".format(it) }
    // A card is claim-once (§16.10), so a restart must recognise the
    // contact it already made rather than asking for a second claim the
    // protocol is right to refuse.
    val driver = ContactStore(context).all().firstOrNull { it.personaHex == theirHex }
        ?.also { println("RIDE_REPAIRED ${it.displayName()} — already a contact") }
        ?: Mailbox.claimCard(context, scanned, null)
            .also { println("RIDE_PAIRED ${it.displayName()}") }
    // Let the claim land before inviting them into a ceremony.
    repeat(5) { tick(); Thread.sleep(2_000) }

    // 2-of-2: no arbiter, both sides staked. This is the rung that has never
    // run between two real clients.
    // Resume rather than build a second escrow: a harness that cannot be
    // restarted is a harness nobody restarts.
    val existing = Ceremony.rideWith(context, driver.personaHex)
        ?.takeIf { it.optString("address").isNotEmpty() }
    val id = existing?.optString("id")?.also {
        println("RIDE_RESUMED ${it.take(8)} — the escrow this pair already built")
    } ?: ContactStore(context).all().first { it.personaHex == driver.personaHex }.let { peer ->
        if (reservation) {
            Ceremony.startReservation(
                context, host = peer, arbiter = null,
                rentPxmr = fare, guestDepPxmr = guestDep, hostDepPxmr = stake,
            )
        } else {
            Ceremony.startRide(
                context, driver = peer, arbiter = null,
                farePxmr = fare, driverStakePxmr = stake,
            )
        }
    }
    println(
        if (reservation) {
            "RIDE_STARTED ${id.take(8)} rent=${formatXmr(fare)} " +
                "deposits=${formatXmr(guestDep)}/${formatXmr(stake)}"
        } else {
            "RIDE_STARTED ${id.take(8)} fare=${formatXmr(fare)} stake=${formatXmr(stake)}"
        },
    )

    val ride = await("both sides to derive the escrow", seconds = 900) {
        Ceremony.all(context).firstOrNull {
            it.optString("id") == id && it.optString("address").isNotEmpty()
        }
    } ?: return
    println("RIDE_BUILT ${ride.optString("address")}")

    // The exposed side goes second: when a driver stake was asked for, the
    // rider waits until it is in. Whoever funds first stands alone, and the
    // rider is carrying ten times what the driver is.
    val theirStake = ride.optLong("hostDepPxmr")
    if (theirStake > 0 && ride.optString("fundTxid").isEmpty()) {
        // By this side's own scan of the escrow. The driver's `hostFundTxid`
        // is written on the driver's device and never travels; the chain is
        // the only fact both sides share.
        await("the driver's stake to appear on chain", seconds = 3600) {
            potByOwnScan(id).takeIf { it >= theirStake }
        } ?: return
        println("RIDE_THEIRS_IN the driver's stake is on chain")
    }

    val owed = Ceremony.mySharePxmr(ride)
    if (ride.optString("fundTxid").isNotEmpty()) {
        println("RIDE_FUNDED rider already paid — ${ride.optString("fundTxid").take(16)}…")
    } else {
        val tx = fundWhenAble(id, owed) ?: return
        println("RIDE_FUNDED rider sent ${formatXmr(owed)} XMR — ${tx.take(16)}…")
    }

    val want = Ceremony.expectedTotalPxmr(ride)
    await("the pot to reach ${formatXmr(want)} XMR", seconds = 1800) {
        potByOwnScan(id).takeIf { it >= want }
    } ?: return
    println("RIDE_SECURED rider's own scan sees ${formatXmr(want)} XMR")

    // The driver proposes; consent is the rider's, and never automatic in
    // the app. Here the test plays the tap.
    // Sign every proposal that arrives, until the release is on chain. A
    // driver whose process restarted must propose again (its signing state
    // died with it), and the rider's consent is asked for again — the same
    // tap, not a different decision.
    var signed = 0
    val landed = await("the release to land", seconds = 3600) {
        val o = Ceremony.all(context).firstOrNull { it.optString("id") == id }
        val done = o?.optString("txid").orEmpty()
        if (done.isNotEmpty()) return@await done
        if (o?.optString("stage") == "release_pending") {
            println("RIDE_PROPOSED rider sees back=${o.optLong("pendingRiderBack")} pXMR")
            runCatching { Ceremony.approveRideRelease(context, id) }
                .onSuccess { signed++; println("RIDE_SIGNED rider consented ($signed)") }
                .onFailure { println("RIDE_WAIT sign: ${it.message}") }
        }
        null
    }
    if (landed == null) { println("RIDE_FAIL no release after consenting $signed time(s)"); return }
    println("RIDE_RELEASED $landed")

    val before = Wallet.balances(context).let { it.spendablePxmr + it.lockedPxmr }
    await("the margin to come home", seconds = 1800) {
        val node = org.ducatproject.ducat.NodeStore(context).lastGood() ?: return@await null
        var steps = 0
        while (steps < 40 && Wallet.scanStep(context, node)) steps++
        val b = Wallet.balances(context)
        (b.spendablePxmr + b.lockedPxmr).takeIf { it > before }
    }
    val rb = Wallet.balances(context)
    println("RIDE_PAID rider holds ${formatXmr(rb.spendablePxmr + rb.lockedPxmr)} XMR")
    println("RIDE_DONE rider")
}
