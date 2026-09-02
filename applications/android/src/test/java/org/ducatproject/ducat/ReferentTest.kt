package org.ducatproject.ducat

import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * A reference resolved in a thread where a seq is not unique.
 *
 * The thread below is the 2026-08-24 counter: a ride offer at seq 0 on one
 * card, declined; then a fresh card, and the shop's bill — also seq 0. The
 * decline must stay on the offer, and the bill must stay payable.
 */
class ReferentTest {

    private fun msg(
        outgoing: Boolean, seq: Long, ts: Long, kind: Int = 0,
        reSeq: Long? = null, reOwn: Boolean = false,
    ) = StoredMessage(
        outgoing = outgoing, seq = seq, body = "", timestamp = ts,
        kind = kind, reSeq = reSeq, reOwn = reOwn,
    )

    private val offer = msg(outgoing = false, seq = 0, ts = 1_000, kind = 6)
    private val decline = msg(outgoing = true, seq = 0, ts = 1_010, kind = 5, reSeq = 0)
    private val bill = msg(outgoing = false, seq = 0, ts = 4_000, kind = 1)
    private val thread = listOf(offer, decline, bill)

    @Test
    fun `a reference names the nearest preceding message with that seq`() {
        assertSame(offer, thread.referent(decline))
    }

    @Test
    fun `a bill reborn at the same seq is not the declined one`() {
        // Their payment for the bill, stamped by their clock, after it.
        val paid = msg(outgoing = true, seq = 1, ts = 4_100, kind = 2, reSeq = 0)
        assertSame(bill, (thread + paid).referent(paid))
    }

    @Test
    fun `a refusal stamped before its bill by a slow clock still reaches it`() {
        // A refusal a minute "before" the bill it answers — the two stamps
        // come from two phones. Nothing precedes at seq 0 on that side in
        // this thread, so the reach forward is allowed, within the skew.
        val alone = listOf(bill)
        val refusal = msg(outgoing = true, seq = 2, ts = 3_940, kind = 5, reSeq = 0)
        assertSame(bill, alone.referent(refusal))
    }

    @Test
    fun `a stamp days out still names the only message it could`() {
        // A bill minted while the sender's clock was four days ahead, paid
        // at once by a phone whose clock was right: the answer sits four
        // days "before" the bill. Nothing else has that seq — this is what
        // the reference means, and the bill was paid.
        val alone = listOf(bill)
        val early = msg(outgoing = true, seq = 2, ts = 4_000 - 4 * 86_400, kind = 2, reSeq = 0)
        assertSame(bill, alone.referent(early))
    }

    @Test
    fun `beyond the skew, the earliest of several is the one that was there`() {
        val later = msg(outgoing = false, seq = 0, ts = 9_000, kind = 1)
        val early = msg(outgoing = true, seq = 2, ts = 4_000 - CLOCK_SKEW_SECS - 1, kind = 2, reSeq = 0)
        assertSame(bill, listOf(bill, later).referent(early))
    }

    @Test
    fun `re_own says whose log the seq is in`() {
        // The sender taking back their own seq 0 — the decline, not the offer.
        val unsend = msg(outgoing = true, seq = 3, ts = 5_000, kind = 5, reSeq = 0, reOwn = true)
        assertSame(decline, thread.referent(unsend))
    }

    @Test
    fun `a message that references nothing resolves to nothing`() {
        assertNull(thread.referent(bill))
    }
}
