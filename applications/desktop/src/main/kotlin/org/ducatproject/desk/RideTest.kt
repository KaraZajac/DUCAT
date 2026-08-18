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

    /** One poll of everything, as the phones' service does it. */
    fun tick() {
        runCatching { Mailbox.collectClaims(context) }
        runCatching { Mailbox.poll(context) }
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
            Ceremony.rideWith(context, rider.personaHex)
                ?.takeIf { it.optString("stage") == "done" && it.optString("address").isNotEmpty() }
        } ?: return
        val id = ride.optString("id")
        println("RIDE_BUILT ${ride.optString("address")} (id ${id.take(8)})")

        // A two-sided ride: the driver's stake goes in beside the fare.
        val owed = Ceremony.mySharePxmr(ride)
        if (owed > 0) {
            val tx = runCatching { Ceremony.fundRide(context, id) }.getOrElse {
                println("RIDE_FAIL driver could not stake: ${it.message}"); return
            }
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

        val txid = await("the rider to consent and the release to land", seconds = 1800) {
            Ceremony.all(context).firstOrNull { it.optString("id") == id }
                ?.optString("releaseTxid")?.takeIf { it.isNotEmpty() }
        } ?: return
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
    val driver = Mailbox.claimCard(context, scanned, null)
    println("RIDE_PAIRED ${driver.displayName()}")
    // Let the claim land before inviting them into a ceremony.
    repeat(5) { tick(); Thread.sleep(2_000) }

    // 2-of-2: no arbiter, both sides staked. This is the rung that has never
    // run between two real clients.
    val id = Ceremony.startRide(
        context,
        driver = ContactStore(context).all().first { it.personaHex == driver.personaHex },
        arbiter = null,
        farePxmr = fare,
        driverStakePxmr = stake,
    )
    println("RIDE_STARTED ${id.take(8)} fare=${formatXmr(fare)} stake=${formatXmr(stake)}")

    val ride = await("both sides to derive the escrow", seconds = 900) {
        Ceremony.all(context).firstOrNull { it.optString("id") == id }
            ?.takeIf { it.optString("stage") == "done" && it.optString("address").isNotEmpty() }
    } ?: return
    println("RIDE_BUILT ${ride.optString("address")}")

    val owed = Ceremony.mySharePxmr(ride)
    val tx = runCatching { Ceremony.fundRide(context, id) }.getOrElse {
        println("RIDE_FAIL rider could not fund: ${it.message}"); return
    }
    println("RIDE_FUNDED rider sent ${formatXmr(owed)} XMR — ${tx.take(16)}…")

    val want = Ceremony.expectedTotalPxmr(ride)
    await("the pot to reach ${formatXmr(want)} XMR", seconds = 1800) {
        potByOwnScan(id).takeIf { it >= want }
    } ?: return
    println("RIDE_SECURED rider's own scan sees ${formatXmr(want)} XMR")

    // The driver proposes; consent is the rider's, and never automatic in
    // the app. Here the test plays the tap.
    val pending = await("the driver's proposal", seconds = 1800) {
        Ceremony.all(context).firstOrNull {
            it.optString("id") == id && it.optString("stage") == "release_pending"
        }
    } ?: return
    println("RIDE_PROPOSED rider sees back=${pending.optLong("pendingRiderBack")} pXMR")

    runCatching { Ceremony.approveRideRelease(context, id) }.getOrElse {
        println("RIDE_FAIL rider could not sign: ${it.message}"); return
    }
    println("RIDE_SIGNED rider consented")

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
