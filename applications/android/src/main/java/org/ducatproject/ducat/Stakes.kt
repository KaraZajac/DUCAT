package org.ducatproject.ducat

/**
 * What each side puts up, and why that number.
 *
 * DUCAT has no company in the middle, so it cannot do what every marketplace
 * does when a deal goes wrong: absorb the loss and move on. What replaces
 * that is a stake each side posts and each side gets back — money that makes
 * finishing the cheaper option than walking away. One sentence to a user:
 *
 *   **You both put up a stake. Finish, and you both get it back.**
 *
 * Everything below is the arithmetic behind that sentence, in one place,
 * because a number this load-bearing must not be three constants in three
 * screens that drift apart.
 *
 * ## Where the percentages come from
 *
 * **Bisq** is the closest working precedent — peer-to-peer trades in 2-of-2
 * multisig, no custodian, deposits from *both* sides. It requires a minimum
 * of **15%** of the trade and caps at **50%**, and says plainly why both
 * sides post: "requiring both parties to post a deposit increases chances
 * that they will be willing to cooperate", and that it avoids reputation
 * systems, which cost privacy. DUCAT wants both of those properties.
 *
 * **The theory** (Asgaonkar & Krishnamachari, *Solving the Buyer and
 * Seller's Dilemma*, IEEE ICBC 2019) proves a dual-deposit escrow is
 * cheat-proof at equilibrium with deposits merely *positive*, and notes
 * safety improves as they grow — but derives no optimum. So theory sets the
 * floor at "greater than zero" and practice has to set the rest.
 *
 * **Practice sets the ceiling**, because a deposit is also a barrier: the
 * rental industry has spent a decade moving *away* from large deposits
 * precisely because they price people out. Whatever deters must still be
 * affordable at the moment of booking.
 *
 * So the numbers are chosen per deal, by how much damage the counterparty
 * can actually do:
 *
 * | Deal | Stake | Why |
 * |---|---|---|
 * | Ride | 10% | Minutes long, arranged in person, and the driver's fare is already fully at risk. Rideshare cancellation fees sit near this on a typical fare. |
 * | Stay | 20% | Airbnb hosts are advised to keep deposits "20% or less of the total booking"; Airbnb's own calculated deposit is 60% of *one night*. |
 * | Vehicle | 30% | A car can be damaged far beyond its rental price, and Turo's own protection tiers run 18–65% of trip price. The high end of what is still bookable. |
 *
 * ## The floor, and why it exists
 *
 * A stake smaller than the fee to send it back is worse than no stake: it
 * deters nothing and costs more than it is worth to return. So a stake is
 * raised to at least twice the network's fee reserve, or dropped to zero —
 * never left as decoration. Bisq does the same thing with an absolute
 * minimum in BTC.
 *
 * ## What a stake is not
 *
 * It is **not insurance**. A 30% stake on a week's car rental does not cover
 * writing the car off, and the app must never imply it does. It is a reason
 * to behave, sized to be felt and still payable — nothing more.
 */
object Stakes {
    /** The kinds of deal DUCAT escrows, and what each suggests. */
    enum class Deal(val percent: Int) {
        /** A hailed ride (§15.12): short, in person, low value. */
        Ride(10),

        /** A place to stay: overnight, unattended, the host's home. */
        Stay(20),

        /** A vehicle or equipment: the asset outlives the rental many times. */
        Vehicle(30),

        /**
         * A thing sold outright (§16.18 kind 3).
         *
         * Not a deposit — nothing comes back, so this is a *stake*: each side
         * posts one and gets it back on handover, and the pair of them is
         * what makes turning up beat not bothering. Ten percent, like a ride,
         * for the same reason: this is a short arrangement between two people
         * who are about to stand in front of each other, and a stake that
         * rivals the price is one nobody posts.
         */
        Sale(10),

        /**
         * Somebody's time (§16.18 kind 5).
         *
         * Priced by the hour, so the stake is against the agreed total rather
         * than the rate. Ten percent for the same reason as a ride, and
         * because the person being hired is usually the one who can least
         * afford to have money tied up.
         */
        Labour(10),
    }

    /**
     * A stake below this is theatre — it costs more to hand back than it is
     * worth. Twice the release's own fee reserve (§17.9's FEE_RESERVE), so
     * returning it is never the larger number.
     */
    const val FLOOR_PXMR: Long = 400_000_000L // 0.0004 XMR

    /** Nobody takes a deal whose stake rivals the deal. Bisq's own ceiling. */
    const val MAX_PERCENT: Int = 50

    /**
     * The stake each side posts on a deal of this size.
     *
     * Returns zero — deliberately, not a rounded-up token — when the deal is
     * too small for any stake to mean anything. A ride worth less than the
     * floor is a ride where the fare itself is the only thing at stake, and
     * saying so is more honest than holding a fee-sized deposit.
     */
    fun stakeFor(deal: Deal, amountPxmr: Long): Long = stakeFor(deal.percent, amountPxmr)

    /** The same, for a percentage the user chose themselves. */
    fun stakeFor(percent: Int, amountPxmr: Long): Long {
        if (amountPxmr <= 0 || percent <= 0) return 0L
        val p = percent.coerceAtMost(MAX_PERCENT)
        val raw = amountPxmr / 100L * p + (amountPxmr % 100L) * p / 100L
        return when {
            raw >= FLOOR_PXMR -> raw
            // Worth having, but only if the deal can carry the floor without
            // the stake dwarfing the thing being paid for.
            amountPxmr >= FLOOR_PXMR * 2 -> FLOOR_PXMR
            else -> 0L
        }
    }

    /** What the paying side locks: the price, plus their own stake. */
    fun funderLocks(deal: Deal, amountPxmr: Long): Long =
        amountPxmr + stakeFor(deal, amountPxmr)

    /** What the paid side locks: their stake alone. */
    fun providerLocks(deal: Deal, amountPxmr: Long): Long = stakeFor(deal, amountPxmr)

    /** Everything the escrow should hold once both sides have paid in. */
    fun escrowHolds(deal: Deal, amountPxmr: Long): Long =
        funderLocks(deal, amountPxmr) + providerLocks(deal, amountPxmr)

    /**
     * What each side walks away with when it goes right: the payer gets their
     * stake back, the provider gets the price and their own stake back. The
     * network fee comes out of the provider's side, because they are the one
     * being paid — the payer's refund is a fixed slice by name (§15.12's
     * split release), so it arrives exactly as promised.
     */
    fun funderGetsBack(deal: Deal, amountPxmr: Long): Long = stakeFor(deal, amountPxmr)

    fun providerGetsPaid(deal: Deal, amountPxmr: Long, feePxmr: Long): Long =
        (amountPxmr + stakeFor(deal, amountPxmr) - feePxmr).coerceAtLeast(0L)
}
