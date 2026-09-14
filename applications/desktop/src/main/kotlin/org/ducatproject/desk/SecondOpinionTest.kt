package org.ducatproject.desk

import org.ducatproject.ducat.SecondOpinion
import org.ducatproject.ducat.SecondOpinion.Verdict

/**
 * What a second node's answer is allowed to do to a sale, through the notes
 * it keeps on disk. `./gradlew :desktop:secondopinion`.
 *
 * The rule itself is walked on a clock by the phone's JVM test
 * (`SecondOpinionRuleTest`); this is the other half — that the verdicts reach
 * the stored notes, that a settled payment is never re-litigated, that the
 * merchant is alarmed once, and that the operator's word ends the wait.
 *
 * The check exists because the scan believes one node about what is on the
 * chain, and at the moment a merchant is told they have been paid that belief
 * hands over goods. But a defence that stops honest sales is worse than the
 * attack it prevents, so the cases below are as much about what must still go
 * through as about what must not.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-2nd").toFile()
    val ctx = DeskContext(dir)
    val t0 = 1_700_000_000_000L
    val minute = 60_000L

    // In a block on a node that is not ours. Settles, and never asks again —
    // the answer cannot change, and the reconciler runs every few seconds.
    check(SecondOpinion.decide(ctx, "aa", Verdict.Confirmed, t0)) {
        "2NDTEST_FAIL a corroborated payment did not settle"
    }
    check(SecondOpinion.settles(ctx, "AA")) {
        "2NDTEST_FAIL a settled payment was re-litigated (and case-folding is broken)"
    }

    // No second node reachable. This used to settle, and that was the hole:
    // every default node is plain http, so the position that lets an attacker
    // forge a block also lets them drop the second node's packets.
    check(!SecondOpinion.decide(ctx, "bb", Verdict.NoAnswer, t0)) {
        "2NDTEST_FAIL silence settled a sale"
    }

    // Somebody else answered — the pool, or never heard of it. Hold.
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0)) {
        "2NDTEST_FAIL a payment no other node has in a block was treated as paid"
    }
    // Held, but not accused: still quiet a few minutes in, because a node one
    // block behind says exactly this and Monero blocks are two minutes apart.
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0 + 3 * minute)) {
        "2NDTEST_FAIL a lagging node stopped deferring too early"
    }
    check(!alarmed(ctx, "cc")) {
        "2NDTEST_FAIL the merchant was alarmed while a node was merely lagging"
    }

    // Past five blocks it is no longer lag. Say so — once.
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0 + 11 * minute)) {
        "2NDTEST_FAIL an uncorroborated payment settled after waiting"
    }
    check(alarmed(ctx, "cc")) { "2NDTEST_FAIL the merchant was never told" }
    check(SecondOpinion.stalled(ctx, "CC")) {
        "2NDTEST_FAIL the screens were not offered the settle-anyway state"
    }
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0 + 30 * minute)) {
        "2NDTEST_FAIL still must not settle"
    }

    // A deferral is not a verdict. The node catches up, and the sale goes
    // through — the customer paid, and nothing about the wait may cost them.
    check(SecondOpinion.decide(ctx, "cc", Verdict.Confirmed, t0 + 31 * minute)) {
        "2NDTEST_FAIL a deferred payment could not recover once corroborated"
    }
    check(SecondOpinion.settles(ctx, "cc")) { "2NDTEST_FAIL recovery did not stick" }
    check(!SecondOpinion.stalled(ctx, "cc")) {
        "2NDTEST_FAIL a corroborated payment was still offered settle-anyway"
    }

    // The operator's word, where the corroboration never came.
    check(!SecondOpinion.decide(ctx, "dd", Verdict.NoAnswer, t0)) {
        "2NDTEST_FAIL silence settled a sale"
    }
    SecondOpinion.settleAnyway(ctx, "DD")
    check(SecondOpinion.settles(ctx, "dd")) {
        "2NDTEST_FAIL the operator's word did not end the wait"
    }
    check(!SecondOpinion.stalled(ctx, "dd")) {
        "2NDTEST_FAIL a forced settle still read as stalled"
    }

    // The small-sale floor: at or under it, a lone node's word will do —
    // because the operator said so, and sized it themselves.
    val floor = 5_000_000_000L
    SecondOpinion.setFloorPxmr(ctx, floor)
    check(SecondOpinion.floorPxmr(ctx) == floor) { "2NDTEST_FAIL the floor did not stick" }
    check(SecondOpinion.decide(ctx, "ee", Verdict.NoAnswer, t0, floor, floor)) {
        "2NDTEST_FAIL a small sale did not settle under the operator's floor"
    }
    check(!SecondOpinion.settles(ctx, "ee")) {
        "2NDTEST_FAIL our own node's word was cached as corroboration"
    }
    check(!SecondOpinion.decide(ctx, "ff", Verdict.NoAnswer, t0, floor + 1, floor)) {
        "2NDTEST_FAIL a sale above the floor settled on silence"
    }
    check(SecondOpinion.confirmationsNeeded(ctx, floor) == 1) {
        "2NDTEST_FAIL a small sale wanted more than one block"
    }
    check(SecondOpinion.confirmationsNeeded(ctx, floor + 1) == 3) {
        "2NDTEST_FAIL an ordinary sale did not want three blocks"
    }
    check(SecondOpinion.confirmationsNeeded(ctx, SecondOpinion.ONE_XMR + 1) == 10) {
        "2NDTEST_FAIL a sale over a monero did not want ten blocks"
    }
    SecondOpinion.setFloorPxmr(ctx, 0)

    // Nothing to ask about: the wallet matched an output carrying no txid.
    // Amount, subaddress and height are all there is, and they already agreed.
    check(SecondOpinion.settles(ctx, "")) {
        "2NDTEST_FAIL a match with no transaction id was blocked forever"
    }

    // An escrow's deposit is keyed by amount, so each increase stands on its
    // own: corroborating 1 XMR must not vouch for the 5 XMR that follows it.
    val keys = ByteArray(0)
    check(SecondOpinion.decide(ctx, "esc_r1_1000", Verdict.Confirmed, t0)) {
        "2NDTEST_FAIL a corroborated deposit was not believed"
    }
    check(SecondOpinion.holdsEscrow(ctx, "r1", keys, 0, 1000, null)) {
        "2NDTEST_FAIL a corroborated deposit was re-asked"
    }
    check(!SecondOpinion.decide(ctx, "esc_r1_5000", Verdict.NotYet, t0)) {
        "2NDTEST_FAIL a larger deposit rode in on the smaller one's corroboration"
    }
    // Nothing claimed is nothing to check — an empty escrow is not a lie.
    check(SecondOpinion.holdsEscrow(ctx, "r1", keys, 0, 0, null)) {
        "2NDTEST_FAIL an unfunded escrow was treated as a claim"
    }

    println(
        "2NDTEST_OK block=settles silence=defers pool=defers alarm=once " +
            "forced=ok floor=small-only escrow=per-amount",
    )
}

private fun alarmed(ctx: DeskContext, key: String) =
    ctx.getSharedPreferences("second_opinion", 0).getBoolean("said_$key", false)
