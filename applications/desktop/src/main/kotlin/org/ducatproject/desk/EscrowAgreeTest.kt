package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import org.json.JSONArray
import org.json.JSONObject

/**
 * Everybody built the same wallet, or nobody funds it.
 * `./gradlew :desktop:escrowagree`
 *
 * Round 0's commitments travel pairwise — no broadcast channel, no echo round
 * — so a participant can send one commitment to B and a different one to C.
 * Both verify: each is self-consistent and carries a valid proof of
 * possession. B and C then derive different group keys, hence different escrow
 * addresses, and until now nothing compared them. The funder funds B's address
 * while the arbiter holds a share of C's: a 2-of-2 with the attacker, wearing
 * the shape of a 2-of-3, and the failure only surfaces when somebody tries to
 * get their money back.
 *
 * core::escrow::check_escrow_ready has made this comparison since it was
 * written and was never reachable, because it takes three reports and nothing
 * exchanged them. What is tested here is the exchange's arithmetic.
 */
fun main() {
    fun rec(mine: String, reported: Map<Int, String>, roster: Int = 3): JSONObject =
        JSONObject().apply {
            put("roster", JSONArray((1..roster).map { "%02x".format(it).repeat(32) }))
            put("address", mine)
            put("addrs", JSONObject().apply { reported.forEach { (k, v) -> put(k.toString(), v) } })
        }

    val good = "5AGood".repeat(4)
    val evil = "5AEvil".repeat(4)

    // Everyone reported, everyone agrees.
    check(Ceremony.escrowAgreed(rec(good, mapOf(1 to good, 2 to good, 3 to good)))) {
        "AGREE_FAIL a unanimous roster was not accepted"
    }

    // Not everyone has spoken. A silent participant has not agreed to
    // anything — this is the first branch check_escrow_ready states, and the
    // reason it takes three reports rather than two.
    check(!Ceremony.escrowAgreed(rec(good, mapOf(1 to good, 2 to good)))) {
        "AGREE_FAIL two of three was treated as agreement"
    }
    check(!Ceremony.escrowAgreed(rec(good, emptyMap()))) {
        "AGREE_FAIL silence was treated as agreement"
    }

    // The attack: somebody formed a different wallet. That is not "not yet",
    // and the two must not be the same answer — a caller that treated them
    // alike would sit politely in front of it.
    for (reported in listOf(
        mapOf(1 to good, 2 to evil, 3 to good),   // one dissenter
        mapOf(1 to evil, 2 to evil, 3 to evil),   // everyone else agrees, with each other
        mapOf(1 to good, 2 to good, 3 to evil),   // the arbiter is the odd one
    )) {
        val threw = runCatching { Ceremony.escrowAgreed(rec(good, reported)) }
            .exceptionOrNull()
        check(threw is Ceremony.EscrowDisagreed) {
            "AGREE_FAIL a divergent wallet did not raise EscrowDisagreed: $threw"
        }
    }

    // A disagreement is reported even before the roster is complete — waiting
    // for the last report would be waiting to be told something already known.
    check(
        runCatching { Ceremony.escrowAgreed(rec(good, mapOf(1 to evil))) }
            .exceptionOrNull() is Ceremony.EscrowDisagreed,
    ) { "AGREE_FAIL an early disagreement was deferred instead of raised" }

    // No address of our own yet: nothing to compare, so nothing is agreed.
    check(!Ceremony.escrowAgreed(rec("", mapOf(1 to good, 2 to good, 3 to good)))) {
        "AGREE_FAIL agreement was claimed before this device formed anything"
    }

    // A two-party bond needs both, not one.
    check(!Ceremony.escrowAgreed(rec(good, mapOf(1 to good), roster = 2))) {
        "AGREE_FAIL a 2-party escrow agreed on one report"
    }
    check(Ceremony.escrowAgreed(rec(good, mapOf(1 to good, 2 to good), roster = 2))) {
        "AGREE_FAIL a complete 2-party roster was refused"
    }

    println("AGREE_OK unanimous=ok silent=refused divergent=raised early=raised empty=refused")
}
