package org.ducatproject.desk

import org.ducatproject.ducat.SecondOpinion
import org.ducatproject.ducat.SecondOpinion.Verdict

/**
 * What a second node's answer is allowed to do to a sale.
 * `./gradlew :desktop:secondopinion`.
 *
 * The check exists because the scan believes one node about what is on the
 * chain, and at the moment a merchant is told they have been paid that belief
 * hands over goods. But a defence that stops honest sales is worse than the
 * attack it prevents — a bar whose till refuses every payment when the wifi is
 * bad will simply stop using the app. So the cases below are as much about
 * what must still go through as about what must not.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-2nd").toFile()
    val ctx = DeskContext(dir)
    val t0 = 1_700_000_000_000L
    val minute = 60_000L

    // Confirmed: somebody else has it. Settles, and never asks again — the
    // answer cannot change, and the reconciler runs every few seconds.
    check(SecondOpinion.decide(ctx, "aa", Verdict.Confirmed, t0)) {
        "2NDTEST_FAIL a corroborated payment did not settle"
    }
    check(SecondOpinion.settles(ctx, "AA")) {
        "2NDTEST_FAIL a settled payment was re-litigated (and case-folding is broken)"
    }

    // No second node reachable. This is the offline till, and it must still
    // take money: the attacker has to be on the wire, the flaky café does not.
    check(SecondOpinion.decide(ctx, "bb", Verdict.NoAnswer, t0)) {
        "2NDTEST_FAIL an unreachable network blocked an honest sale"
    }

    // Somebody else answered and has never heard of it. Hold — the whole point.
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0)) {
        "2NDTEST_FAIL a payment no other node has ever seen was treated as paid"
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
        "2NDTEST_FAIL an unconfirmable payment settled after waiting"
    }
    check(alarmed(ctx, "cc")) { "2NDTEST_FAIL the merchant was never told" }
    check(!SecondOpinion.decide(ctx, "cc", Verdict.NotYet, t0 + 30 * minute)) {
        "2NDTEST_FAIL still must not settle"
    }

    // A deferral is not a verdict. The node catches up, and the sale goes
    // through — the customer paid, and nothing about the wait may cost them.
    check(SecondOpinion.decide(ctx, "cc", Verdict.Confirmed, t0 + 31 * minute)) {
        "2NDTEST_FAIL a deferred payment could not recover once corroborated"
    }
    check(SecondOpinion.settles(ctx, "cc")) {
        "2NDTEST_FAIL recovery did not stick"
    }

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
        "2NDTEST_OK confirmed=settles noanswer=settles notyet=defers alarm=once " +
            "recovery=ok escrow=per-amount",
    )
}

private fun alarmed(ctx: DeskContext, key: String) =
    ctx.getSharedPreferences("second_opinion", 0).getBoolean("said_$key", false)
