package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import uniffi.ducat_mobile.TxDestination

/**
 * What a co-signer is allowed to believe about where the money goes.
 * `./gradlew :desktop:releaseread`.
 *
 * Before this, the release consent screen stated amounts and never
 * destinations: the figure came from `amountPxmr`, which travels beside the
 * proposal and is written by the party who gains from being believed, and the
 * co-signer then approved a payload it never parsed. A driver could show the
 * rider "0.9 XMR back to you" and hand over a transaction paying the driver
 * everything.
 *
 * `Ceremony.releaseToMe` reads the transaction's own outputs instead. The
 * cases here are as much about what must still go through: a release that
 * cannot be sized has to defer rather than refuse, or an honest phone that has
 * not scanned the escrow yet would reject every settlement offered to it.
 */
fun main() {
    fun fixed(addr: String, amount: Long) = TxDestination(addr, amount.toULong(), false)
    fun residual(addr: String) = TxDestination(addr, 0uL, true)

    val me = setOf("5MINE", "5MINE_PERCONTACT")
    val funded = 1_000_000L

    // The ordinary split: my slice is fixed, the other side is residual and
    // pays the fee out of their own share. Exact, and needs no total.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5MINE", 900_000L), residual("5THEIRS")), me, funded,
        ) == 900_000L,
    ) { "RELREAD_FAIL a fixed slice to this device was not read back" }

    // **The attack.** The note said 900_000 was coming back; the transaction
    // pays it all to them. The figure the screen states is now this one, so
    // the split shows as what it is instead of as what it claimed.
    check(
        Ceremony.releaseToMe(listOf(residual("5THEIRS")), me, funded) == 0L,
    ) { "RELREAD_FAIL a transaction paying this device nothing was read as paying it" }

    // A near miss: the right shape, one character wrong in the address. Money
    // that lands somewhere else is money this device did not get.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5MINEX", 900_000L), residual("5THEIRS")), me, funded,
        ) == 0L,
    ) { "RELREAD_FAIL an address that merely resembles this device's counted" }

    // The flipped split: my side is residual, so my share is everything the
    // fixed outputs leave. It takes this device's own scan to size — and that
    // is the point, because the total must not come from the proposal.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5THEIRS", 100_000L), residual("5MINE")), me, funded,
        ) == 900_000L,
    ) { "RELREAD_FAIL a residual share was not sized against the funded total" }

    // Flipped, and they awarded themselves the lot. Same shape, honest answer.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5THEIRS", 990_000L), residual("5MINE")), me, funded,
        ) == 10_000L,
    ) { "RELREAD_FAIL a residual share was overstated" }

    // Unknown is not zero. A phone that has not scanned the escrow cannot
    // size a residual share, and saying "you get nothing" would refuse honest
    // releases. It defers instead, and the screen falls back to the claim.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5THEIRS", 100_000L), residual("5MINE")), me, 0L,
        ) == null,
    ) { "RELREAD_FAIL an unscanned escrow was read as paying nothing" }
    // Same when the fixed outputs exceed what this device saw arrive: the
    // scan is behind the proposal, which is a reason to defer, not to accuse.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5THEIRS", 2_000_000L), residual("5MINE")), me, funded,
        ) == null,
    ) { "RELREAD_FAIL a stale scan produced a confident wrong figure" }

    // A fixed slice this device can read needs no total at all, even with
    // nothing scanned — there is nothing to subtract it from.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5MINE", 900_000L), residual("5THEIRS")), me, 0L,
        ) == 900_000L,
    ) { "RELREAD_FAIL an exact slice was made to depend on the escrow total" }

    // Change named by view pair rather than by address: nothing DUCAT builds,
    // and unrecognisable to anybody. It reads as nameless, so a residual that
    // is really the proposer's cannot pass as this device's.
    check(
        Ceremony.releaseToMe(listOf(TxDestination("", 0uL, true)), me, funded) == 0L,
    ) { "RELREAD_FAIL a nameless residual was adopted as this device's" }

    // Either of this device's addresses counts: which one applies depends on
    // the role and on who proposed, so the set is the answer.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5MINE_PERCONTACT", 900_000L), residual("5THEIRS")), me, funded,
        ) == 900_000L,
    ) { "RELREAD_FAIL the published per-contact address was not recognised" }

    // Split across both of them, which a counter-proposal can produce.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5MINE", 400_000L), fixed("5MINE_PERCONTACT", 500_000L),
                residual("5THEIRS")),
            me, funded,
        ) == 900_000L,
    ) { "RELREAD_FAIL two slices to this device were not added up" }

    // A plain bond gives the whole escrow away deliberately. Reading zero is
    // correct and must not be mistaken for the attack — the gate compares
    // against what the screen stated, and the screen states the same zero.
    check(
        Ceremony.releaseToMe(listOf(residual("5THEIRS")), me, funded) == 0L,
    ) { "RELREAD_FAIL a deliberate giveaway did not read as zero" }

    // An **arbiter** reads zero for a different reason: it is on neither side
    // of a split between two other people. The zero is truthful and the gate
    // is satisfied by it, but it says nothing about how the principals are
    // dividing the escrow — which is why the banner does not derive the split
    // from it (an arbiter is shown the claim it is being asked to rule on),
    // and why the desk console prints the transaction's own outputs instead.
    check(
        Ceremony.releaseToMe(
            listOf(fixed("5RIDER", 400_000L), residual("5DRIVER")), me, funded,
        ) == 0L,
    ) { "RELREAD_FAIL a third party read itself a share of somebody else's split" }

    println(
        "RELREAD_OK fixed=exact stolen=caught nearmiss=caught residual=sized " +
            "unscanned=defers nameless=refused multiaddr=ok arbiter=none-of-mine",
    )
}
