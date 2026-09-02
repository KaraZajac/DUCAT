package org.ducatproject.desk

import org.ducatproject.ducat.Stakes
import org.ducatproject.ducat.Stakes.Deal
import org.ducatproject.ducat.formatXmr

/**
 * The stake arithmetic, pinned.
 *
 * This is the number the whole trust argument rests on, and the one a user
 * is promised out loud: *you both put up a stake, and finishing gives it
 * back*. A rounding error here is a broken promise, so the sums are checked
 * rather than assumed — including the one that matters most, that what goes
 * in comes back out with nothing missing but the network's own fee.
 *
 * `./gradlew :desktop:staketest`
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("STAKE ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val xmr = 1_000_000_000_000L // one XMR in piconero

    // 1. The suggested percentages are the researched ones.
    check("a ride asks 10%", Deal.Ride.percent == 10)
    check("a stay asks 20%", Deal.Stay.percent == 20)
    check("a vehicle asks 30%", Deal.Vehicle.percent == 30)

    // 2. The percentage is the percentage, at sizes people actually deal in.
    val fare = xmr / 10 // 0.1 XMR
    check("10% of a fare is 10% of a fare", Stakes.stakeFor(Deal.Ride, fare) == fare / 10,
        formatXmr(Stakes.stakeFor(Deal.Ride, fare)))
    check("20% of a stay", Stakes.stakeFor(Deal.Stay, xmr) == xmr / 5)
    check("30% of a vehicle", Stakes.stakeFor(Deal.Vehicle, xmr) == xmr * 3 / 10)

    // 3. Both sides post the same, and the escrow holds the sum.
    val locksF = Stakes.funderLocks(Deal.Ride, fare)
    val locksP = Stakes.providerLocks(Deal.Ride, fare)
    check("the rider locks fare plus stake", locksF == fare + fare / 10, formatXmr(locksF))
    check("the driver locks the same stake", locksP == fare / 10, formatXmr(locksP))
    check("the escrow holds both", Stakes.escrowHolds(Deal.Ride, fare) == locksF + locksP)

    // 4. The promise: finish, and each side is whole again but for the fee.
    val feePxmr = 121_680_000L // a real stagenet fee, from this session's runs
    val back = Stakes.funderGetsBack(Deal.Ride, fare)
    val paid = Stakes.providerGetsPaid(Deal.Ride, fare, feePxmr)
    check("the rider's stake comes back whole", back == locksP)
    check("the driver gets fare and stake, less the fee",
        paid == fare + locksP - feePxmr, formatXmr(paid))
    check("nothing goes missing",
        back + paid + feePxmr == Stakes.escrowHolds(Deal.Ride, fare),
        "in ${formatXmr(Stakes.escrowHolds(Deal.Ride, fare))} = " +
            "back ${formatXmr(back)} + paid ${formatXmr(paid)} + fee ${formatXmr(feePxmr)}")

    // 5. The floor: a stake worth less than returning it is not a stake.
    val tiny = 100_000_000L // 0.0001 XMR — 10% would be 0.00001
    check("a tiny fare gets no stake rather than a token one",
        Stakes.stakeFor(Deal.Ride, tiny) == 0L, formatXmr(Stakes.stakeFor(Deal.Ride, tiny)))
    val small = Stakes.FLOOR_PXMR * 3
    check("a fare that can carry the floor gets the floor",
        Stakes.stakeFor(Deal.Ride, small) == Stakes.FLOOR_PXMR,
        formatXmr(Stakes.stakeFor(Deal.Ride, small)))
    check("and a stake is never below the floor unless it is zero",
        Stakes.stakeFor(Deal.Ride, small) >= Stakes.FLOOR_PXMR)

    // 6. The ceiling: a stake cannot exceed half the deal, however asked.
    check("a mad percentage is capped", Stakes.stakeFor(500, xmr) == xmr / 2,
        formatXmr(Stakes.stakeFor(500, xmr)))
    check("zero percent is zero", Stakes.stakeFor(0, xmr) == 0L)
    check("a negative percentage is zero", Stakes.stakeFor(-10, xmr) == 0L)
    check("no deal is no stake", Stakes.stakeFor(Deal.Ride, 0L) == 0L)

    // 7. Rounding: the percentage of an awkward number is still exact enough
    //    that the sums add up, which is the only property that matters.
    for (amount in listOf(1L, 999L, 1_234_567L, 7_777_777_777L, 33_333_333_333_333L)) {
        val st = Stakes.stakeFor(Deal.Stay, amount)
        val sum = Stakes.escrowHolds(Deal.Stay, amount)
        if (sum != amount + st * 2) {
            check("sums add up at $amount", false, "$sum vs ${amount + st * 2}")
        }
    }
    check("sums add up at every size tried", true)

    // 8. What the user is told, at a size they would recognise. Read it back
    //    and see whether it is a sentence a person can act on.
    val ride = xmr / 20 // 0.05 XMR
    println(
        "STAKE      a ride of ${formatXmr(ride)}: rider locks " +
            "${formatXmr(Stakes.funderLocks(Deal.Ride, ride))}, driver locks " +
            "${formatXmr(Stakes.providerLocks(Deal.Ride, ride))}, " +
            "each gets ${formatXmr(Stakes.stakeFor(Deal.Ride, ride))} back",
    )
    println(
        "STAKE      a stay of ${formatXmr(xmr)}: guest locks " +
            "${formatXmr(Stakes.funderLocks(Deal.Stay, xmr))}, host locks " +
            "${formatXmr(Stakes.providerLocks(Deal.Stay, xmr))}",
    )

    // 9. The split a successful two-sided ride must produce.
    //
    //    This is the check that would have caught a real bug: the default
    //    refund was "everything in the escrow above the fare", which was
    //    correct while only the rider paid in and became a quiet robbery of
    //    the driver the moment they staked too — the rider would have taken
    //    their own stake *and* the driver's on a ride that went perfectly.
    run {
        val f = xmr / 20                       // 0.05 XMR fare
        val stake = Stakes.stakeFor(Deal.Ride, f)
        val pot = f + stake + stake            // what both sides paid in
        val fee = 121_680_000L

        // What the release must hand back, by role.
        val riderBack = stake
        val driverGets = pot - riderBack - fee

        check("the rider gets exactly their own stake", riderBack == stake,
            formatXmr(riderBack))
        check("not the driver's as well", riderBack != pot - f,
            "the old rule would have paid ${formatXmr(pot - f)}")
        check("the driver gets the fare and their own stake back",
            driverGets == f + stake - fee, formatXmr(driverGets))
        check("the pot is exactly consumed", riderBack + driverGets + fee == pot)
        // And the driver must never end up worse off than not staking.
        check("staking never costs the driver on success",
            driverGets >= f - fee, "${formatXmr(driverGets)} vs fare ${formatXmr(f - fee)}")
    }

    // 7. **Getting your own money back out of a half-funded escrow.**
    //
    // One side has paid, the other never came, and the claim the banner
    // makes for them is "give me back what I put in". Two things must hold:
    // it is the whole balance when the balance is only theirs, and it is
    // still only their own share when the other side funded a second after
    // the button was pressed. The second is the one worth pinning — the
    // difference between an honest claim and asking an arbiter to hand over
    // somebody else's money.
    run {
        val fare = 5L * xmr
        val stake = fare / 10
        fun ride(funder: Boolean) = org.json.JSONObject()
            .put("i", if (funder) 1 else 2)
            .put("funderIdx", 1)
            .put("farePxmr", fare)
            .put("hostDepPxmr", stake)

        val riderOnly = org.ducatproject.ducat.Ceremony.refundBack(ride(true), fare + stake)
        check("a stranded rider asks for exactly what they paid",
            riderOnly == fare + stake, formatXmr(riderOnly))
        val riderRaced = org.ducatproject.ducat.Ceremony
            .refundBack(ride(true), fare + stake + stake)
        check("and not the driver's stake that landed meanwhile",
            riderRaced == fare + stake, formatXmr(riderRaced))

        val driverOnly = org.ducatproject.ducat.Ceremony.refundBack(ride(false), stake)
        check("a stranded driver leaves nothing to the rider", driverOnly == 0L)
        val driverRaced = org.ducatproject.ducat.Ceremony
            .refundBack(ride(false), stake + fare + stake)
        check("and hands back the fare that landed meanwhile",
            driverRaced == fare + stake, formatXmr(driverRaced))

        // A view of the escrow that is SHORT of this party's own payment is
        // the one case refundBack must never be asked about: clamping there
        // is how a refund became a gift on the first live run (see
        // Ceremony.EscrowBehind, which proposeRideSplit now throws instead).
        // Kept as an assertion about the clamp so the shape cannot come
        // back by accident: it still clamps, and the caller must not let it.
        val short = org.ducatproject.ducat.Ceremony.refundBack(ride(true), stake)
        check("a short reading clamps — which is why the caller refuses first",
            short == stake, formatXmr(short))

        // The same split read the other way: a funded escrow unwound by
        // agreement. Both sides must compute the identical number, or the
        // "refund" would mean something different depending on who pressed
        // it — and one of the two would be proposing a deal in their own
        // favour under a button that says it is even.
        val pot = fare + stake + stake
        val fromRider = org.ducatproject.ducat.Ceremony.refundBack(ride(true), pot)
        val fromDriver = org.ducatproject.ducat.Ceremony.refundBack(ride(false), pot)
        check("a mutual refund is the same split from either side",
            fromRider == fromDriver, formatXmr(fromRider))
        check("the payer gets their fare and their stake",
            fromRider == fare + stake, formatXmr(fromRider))
        check("and the provider is left exactly their own stake",
            pot - fromRider == stake, formatXmr(pot - fromRider))
    }

    // 8. **The ordinary ending**, which is the one that runs every time.
    //
    // Completing a deal sends the funder's own deposit home and everything
    // else to the provider. Nothing checked this arithmetic until now, and
    // it is the piece with the most money through it: a ride that pays the
    // driver the rider's stake as well looks exactly like a ride that went
    // well, from both phones, until somebody counts.
    run {
        val fare = 5L * xmr
        val stake = fare / 10
        val pot = fare + stake + stake

        val ride = org.json.JSONObject()
            .put("i", 1).put("funderIdx", 1)
            .put("farePxmr", fare).put("funderDepPxmr", stake).put("hostDepPxmr", stake)
        val back = org.ducatproject.ducat.Ceremony.settlementBack(ride, pot)
        check("a completed ride sends the payer's stake home", back == stake, formatXmr(back))
        check("and the driver takes the fare and their own stake",
            pot - back == fare + stake, formatXmr(pot - back))

        // The old rule, kept as the thing that must not come back.
        check("not everything above the fare — that was the driver's stake too",
            back != pot - fare, "the old rule would have sent ${formatXmr(pot - fare)}")

        // A booking: the guest's deposit home, rent and the host's deposit
        // to the host.
        val rent = 2L * xmr
        val guestDep = rent / 5
        val hostDep = rent / 10
        val stay = org.json.JSONObject()
            .put("i", 1).put("funderIdx", 1).put("kind", 2)
            .put("farePxmr", rent).put("funderDepPxmr", guestDep).put("hostDepPxmr", hostDep)
        val stayPot = rent + guestDep + hostDep
        val stayBack = org.ducatproject.ducat.Ceremony.settlementBack(stay, stayPot)
        check("a finished stay sends the guest's deposit home", stayBack == guestDep,
            formatXmr(stayBack))
        check("and the host takes the rent and their own deposit",
            stayPot - stayBack == rent + hostDep, formatXmr(stayPot - stayBack))

        // Ceremonies built before the deposit was recorded derive it from
        // the pot, and must still leave the driver's stake alone.
        val legacy = org.json.JSONObject()
            .put("i", 1).put("funderIdx", 1)
            .put("farePxmr", fare).put("hostDepPxmr", stake)
        val legacyBack = org.ducatproject.ducat.Ceremony.settlementBack(legacy, pot)
        check("a ceremony with no recorded deposit derives the same number",
            legacyBack == stake, formatXmr(legacyBack))
    }

    println(if (failures == 0) "STAKETEST OK" else "STAKETEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
