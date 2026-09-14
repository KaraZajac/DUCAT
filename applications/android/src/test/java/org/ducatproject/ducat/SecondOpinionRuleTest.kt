package org.ducatproject.ducat

import org.ducatproject.ducat.SecondOpinion.Memo
import org.ducatproject.ducat.SecondOpinion.Move
import org.ducatproject.ducat.SecondOpinion.Rule
import org.ducatproject.ducat.SecondOpinion.Verdict
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What a second node's answer is allowed to do to a sale, on a clock this
 * test walks itself.
 *
 * The rule is the phone's half of `app/src/opinion.rs`, and these are that
 * file's tests asked in Kotlin: only a block elsewhere settles, everything
 * else defers, the merchant is told once at ten minutes, and the operator's
 * floor is the single exception — their risk, the size they chose.
 *
 * The cases are as much about what must still go through as about what must
 * not: a defence that stops honest sales is worse than the attack it
 * prevents, because a bar whose till refuses every payment on bad wifi stops
 * using the app.
 */
class SecondOpinionRuleTest {

    private val t0 = 1_700_000_000_000L
    private val minute = 60_000L

    @Test
    fun `a block elsewhere settles and is never asked about again`() {
        val m = Memo()
        assertEquals(Move.Settle, Rule.move(m, Verdict.Confirmed, t0))
        assertTrue(m.ok)
        // Cached: the answer cannot change, and the reconciler runs every few
        // seconds.
        assertEquals(Move.Settle, Rule.move(m, Verdict.NotYet, t0 + minute))
        assertFalse(Rule.stalled(m, t0 + 30 * minute))
    }

    @Test
    fun `the pool is not a block`() {
        // The old rule took any sighting as corroboration, so a payment still
        // replaceable in the mempool vouched for itself.
        val m = Memo()
        assertEquals(Move.Hold, Rule.move(m, Verdict.NotYet, t0))
        assertFalse(m.ok)
    }

    @Test
    fun `silence defers above the floor`() {
        // It used to settle. Every default node is plain http, so the position
        // that lets an attacker forge a block also lets them drop the second
        // node's packets — settling on silence handed them the answer.
        val m = Memo()
        assertEquals(Move.Hold, Rule.move(m, Verdict.NoAnswer, t0, amountPxmr = 5, floorPxmr = 0))
        assertFalse(m.ok)
    }

    @Test
    fun `the merchant is told once, at ten minutes, and not before`() {
        val m = Memo()
        assertEquals(Move.Hold, Rule.move(m, Verdict.NotYet, t0))
        // A node one block behind says exactly this, and blocks are two
        // minutes apart. Quiet.
        assertEquals(Move.Hold, Rule.move(m, Verdict.NotYet, t0 + 3 * minute))
        assertFalse(Rule.stalled(m, t0 + 3 * minute))

        assertEquals(Move.HoldAndSay, Rule.move(m, Verdict.NotYet, t0 + 11 * minute))
        assertTrue(Rule.stalled(m, t0 + 11 * minute))
        // Once. A notification per poll for half an hour is noise, and noise
        // is how a real one gets missed.
        assertEquals(Move.Hold, Rule.move(m, Verdict.NotYet, t0 + 30 * minute))

        // A deferral is not a verdict: the customer paid, and nothing about
        // the wait may cost them once a node catches up.
        assertEquals(Move.Settle, Rule.move(m, Verdict.Confirmed, t0 + 31 * minute))
        assertFalse(Rule.stalled(m, t0 + 31 * minute))
    }

    @Test
    fun `the ask is throttled to once a minute, and a future stamp does not wedge it`() {
        val m = Memo()
        Rule.move(m, Verdict.NotYet, t0)
        assertFalse(Rule.due(m, t0 + 30_000))
        assertTrue(Rule.due(m, t0 + minute))
        // A clock wound forward and back leaves a stamp ahead of now. A bare
        // subtraction would hold the gate shut for ever, and the only road to
        // settled runs through it.
        m.asked = t0 + 10 * minute
        assertTrue(Rule.due(m, t0))
    }

    @Test
    fun `the operator's floor settles small sales on silence, and nothing else`() {
        val floor = 5_000_000_000L // 0.005 XMR
        val small = Memo()
        assertEquals(
            Move.Settle,
            Rule.move(small, Verdict.NoAnswer, t0, amountPxmr = floor, floorPxmr = floor),
        )
        // Not cached — that was our own node's word, not a fact, and the next
        // sale asks again.
        assertFalse(small.ok)

        val big = Memo()
        assertEquals(
            Move.Hold,
            Rule.move(big, Verdict.NoAnswer, t0, amountPxmr = floor + 1, floorPxmr = floor),
        )
        // The floor forgives silence, never a node that answered. A payment in
        // the pool is replaceable whatever it is worth.
        val pooled = Memo()
        assertEquals(
            Move.Hold,
            Rule.move(pooled, Verdict.NotYet, t0, amountPxmr = 1, floorPxmr = floor),
        )
        // An unknown amount is never small: the ceremony's escrow releases
        // pass none, and they are the largest sums in the app.
        val unknown = Memo()
        assertEquals(
            Move.Hold,
            Rule.move(unknown, Verdict.NoAnswer, t0, amountPxmr = 0, floorPxmr = floor),
        )
    }

    @Test
    fun `the escrow question still settles on silence`() {
        // Deliberately not the sale rule: there is no "in a block" answer to
        // wait for, so deferring on an unreachable node stalls a ceremony with
        // a countdown rather than waiting two minutes for a block.
        val m = Memo()
        assertEquals(
            Move.Settle,
            Rule.move(m, Verdict.NoAnswer, t0, silenceSettles = true),
        )
        assertEquals(Move.Hold, Rule.move(Memo(), Verdict.NotYet, t0, silenceSettles = true))
    }

    @Test
    fun `confirmations are sized by the amount`() {
        assertEquals(3, Rule.confirmationsNeeded(1, 0))
        assertEquals(3, Rule.confirmationsNeeded(SecondOpinion.ONE_XMR, 0))
        assertEquals(10, Rule.confirmationsNeeded(SecondOpinion.ONE_XMR + 1, 0))
        // Unknown amounts are never small.
        assertEquals(3, Rule.confirmationsNeeded(0, 1_000))
        assertEquals(1, Rule.confirmationsNeeded(1_000, 1_000))
        assertEquals(3, Rule.confirmationsNeeded(1_001, 1_000))
    }

    @Test
    fun `depth counts the block itself, and nothing counts while it is in the pool`() {
        assertEquals(3, Rule.confirmationsOf(100, 102))
        assertEquals(1, Rule.confirmationsOf(102, 102))
        // Not in a block, or a tip we have not scanned up to yet.
        assertEquals(0, Rule.confirmationsOf(0, 102))
        assertEquals(0, Rule.confirmationsOf(103, 102))
    }
}
