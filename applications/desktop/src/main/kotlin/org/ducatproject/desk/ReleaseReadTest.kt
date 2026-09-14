package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import uniffi.ducat_mobile.TxDestination

/**
 * What a co-signer is allowed to believe about where the money goes.
 * `./gradlew :desktop:releaseread`.
 *
 * Before any of this, the release consent screen stated amounts and never
 * destinations: the figure came from `amountPxmr`, which travels beside the
 * proposal and is written by the party who gains from being believed, and the
 * co-signer then approved a payload it never parsed. A driver could show the
 * rider "0.9 XMR back to you" and hand over a transaction paying the driver
 * everything.
 *
 * Reading the outputs fixed half of it. The other half is that a **change**
 * output carries no amount on the wire at all — it takes
 * `inputs − fixed − fee` — and the screen substituted the escrow's scanned
 * balance for `inputs`. Those are the same number only when the release
 * sweeps the whole escrow, and nothing made it: a proposer spending one of
 * two notes paid every fixed slice in full and halved the residual, which is
 * the co-signer's own stake, while every figure still added up (M2). So the
 * terms all come from the transaction now, and `Ceremony.sizeRelease` is the
 * pure judgement over them.
 *
 * The cases here are as much about what must still go through: an honest
 * settlement of any shape has to stay signable, or the gate is a wall.
 */
fun main() {
    fun fixed(addr: String, amount: Long) = TxDestination(addr, amount.toULong(), false)
    fun residual(addr: String) = TxDestination(addr, 0uL, true)

    val me = setOf("5MINE", "5MINE_PERCONTACT")
    val payer = "5PAYER"
    val theirs = "5THEIRS_PUBLISHED"
    val pot = 1_000_000L
    val fee = 10_000L

    fun size(dests: List<TxDestination>, inputs: Long = pot) =
        Ceremony.sizeRelease(dests, me, payer, theirs, inputs, fee)

    fun toMe(dests: List<TxDestination>, inputs: Long = pot) =
        size(dests, inputs).filter { it.address in me }.sumOf { it.amountPxmr }

    // The ordinary split: my slice is fixed, the other side is residual and
    // pays the fee out of their own share. Exact, and needs no total.
    check(toMe(listOf(fixed("5MINE", 900_000L), residual("5THEIRS"))) == 900_000L) {
        "RELREAD_FAIL a fixed slice to this device was not read back"
    }

    // **The attack.** The note said 900_000 was coming back; the transaction
    // pays it all to them. The figure the screen states is now this one, so
    // the split shows as what it is instead of as what it claimed.
    check(toMe(listOf(residual("5THEIRS"))) == 0L) {
        "RELREAD_FAIL a transaction paying this device nothing was read as paying it"
    }

    // A near miss: the right shape, one character wrong in the address. Money
    // that lands somewhere else is money this device did not get.
    check(toMe(listOf(fixed("5MINEX", 900_000L), residual("5THEIRS"))) == 0L) {
        "RELREAD_FAIL an address that merely resembles this device's counted"
    }

    // The flipped split: my side is residual, so my share is everything the
    // fixed outputs and the fee leave — out of the transaction's own inputs.
    check(toMe(listOf(fixed("5THEIRS", 100_000L), residual("5MINE"))) == 890_000L) {
        "RELREAD_FAIL a residual share was not sized from the inputs"
    }

    // **The partial sweep (M2).** Identical outputs, identical claim — and
    // the transaction spends 600_000 of the escrow's 1_000_000. The residual
    // is what the *inputs* leave, so this reads 490_000 where the balance
    // would have said 890_000. Four hundred thousand of somebody's stake.
    check(
        toMe(listOf(fixed("5THEIRS", 100_000L), residual("5MINE")), inputs = 600_000L) == 490_000L,
    ) { "RELREAD_FAIL a partial sweep was sized as though it swept the escrow" }

    // Flipped, and they awarded themselves the lot. Same shape, honest answer.
    check(toMe(listOf(fixed("5THEIRS", 990_000L), residual("5MINE"))) == 0L) {
        "RELREAD_FAIL a residual share was overstated"
    }

    // Change named by view pair rather than by address: nothing DUCAT builds,
    // and unrecognisable to anybody. Refused rather than shown — a residual
    // that is really the proposer's must not be put in front of a person at
    // all, let alone pass as this device's.
    check(runCatching { size(listOf(TxDestination("", 0uL, true))) }.isFailure) {
        "RELREAD_FAIL a nameless output was described instead of refused"
    }

    // Three destinations is a shape no release here builds. Refused unheard,
    // for the same reason: this device could not honestly describe it.
    check(
        runCatching {
            size(listOf(fixed("5MINE", 1L), fixed(payer, 1L), residual("5THEIRS")))
        }.isFailure,
    ) { "RELREAD_FAIL a three-way payout was accepted" }

    // Terms that do not close — the fixed slices and the fee exceeding the
    // inputs — are a disagreement between this walk and the crate that built
    // the transaction, and a disagreement is a refusal, never a zero.
    check(
        runCatching { size(listOf(fixed("5THEIRS", 2_000_000L), residual("5MINE"))) }.isFailure,
    ) { "RELREAD_FAIL arithmetic that does not close produced a confident figure" }

    // Either of this device's addresses counts: which one applies depends on
    // the role and on who proposed, so the set is the answer.
    check(toMe(listOf(fixed("5MINE_PERCONTACT", 900_000L), residual("5THEIRS"))) == 900_000L) {
        "RELREAD_FAIL the published per-contact address was not recognised"
    }

    // **Who each output pays**, which is what an arbiter is shown instead of
    // the proposer's claim (M4). The payer's refund address comes from the
    // round-0 frame every party echoed, so it is the one attribution nobody
    // has to take on trust; an address this node cannot place is said to be
    // unplaceable rather than rendered like the others.
    val ruled = size(listOf(fixed(payer, 400_000L), residual("5ELSEWHERE")))
    check(ruled[0].side == Ceremony.Side.PAYER) {
        "RELREAD_FAIL the frame's refund address was not recognised as the payer's"
    }
    check(ruled[1].side == Ceremony.Side.UNKNOWN && ruled[1].amountPxmr == 590_000L) {
        "RELREAD_FAIL an unplaceable residual was attributed or mis-sized"
    }
    check(
        size(listOf(fixed(theirs, 400_000L), residual(payer)))[0].side == Ceremony.Side.THEIRS,
    ) { "RELREAD_FAIL the address that party published was not recognised" }

    // A third party reads none of a split between two other people — the
    // truthful zero, and the reason the arbiter's console prints the outputs.
    check(toMe(listOf(fixed(payer, 400_000L), residual(theirs))) == 0L) {
        "RELREAD_FAIL a third party read itself a share of somebody else's split"
    }

    println(
        "RELREAD_OK fixed=exact stolen=caught nearmiss=caught residual=from-inputs " +
            "partial-sweep=caught nameless=refused three-way=refused sides=attributed",
    )
}
