package org.ducatproject.ducat

import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * Which bill a tab was settled by, in a thread where a seq is not unique.
 *
 * A repeat customer whose card was re-claimed between visits: the shop's
 * bill for the second visit carries the same seq as the first, and the
 * first tab — still awaiting payment — must not read the second bill's
 * notice as its own.
 */
class TabBillTest {

    private fun bill(seq: Long, ts: Long, amount: Long) = StoredMessage(
        outgoing = true, seq = seq, body = "", timestamp = ts, kind = 1, amountPxmr = amount,
    )

    private fun tab(billSeq: Long, settledAt: Long, total: Long) = RunningTab(
        id = "t", origin = "bar", personaHex = "aa", openedAt = settledAt - 1_000,
        lines = emptyList(), taxPxmr = null, state = "settled",
        settledTotal = total, settledAt = settledAt, billSeq = billSeq,
    )

    private val first = bill(seq = 0, ts = 1_000, amount = 5)
    private val second = bill(seq = 0, ts = 90_000, amount = 7)
    private val thread = listOf(first, second)

    @Test
    fun `the older tab keeps the older bill at a shared seq`() {
        assertSame(first, tab(billSeq = 0, settledAt = 999_500, total = 5).billIn(thread))
    }

    @Test
    fun `the newer tab finds the newer bill`() {
        assertSame(second, tab(billSeq = 0, settledAt = 89_999_000, total = 7).billIn(thread))
    }

    @Test
    fun `a bill stamped before its settle is still the last at that seq`() {
        // A clock stepped back between the settle write and the send:
        // nothing at the seq sits after the settle, so the newest is it.
        assertSame(second, tab(billSeq = 0, settledAt = 200_000_000, total = 7).billIn(thread))
    }

    @Test
    fun `a tab from before the seq was kept matches by total`() {
        assertSame(first, tab(billSeq = -1, settledAt = 999_500, total = 5).billIn(thread))
        assertNull(tab(billSeq = -1, settledAt = 999_500, total = 6).billIn(thread))
    }

    @Test
    fun `a notice resolves to the tab's own bill and to no other`() {
        val t = tab(billSeq = 0, settledAt = 999_500, total = 5)
        val mine = t.billIn(thread)
        // Their notice for the second visit, naming seq 0 of our log.
        val notice = StoredMessage(
            outgoing = false, seq = 4, body = "", timestamp = 90_010, kind = 2,
            amountPxmr = 7, reSeq = 0, reOwn = false,
        )
        val all = thread + notice
        assert(all.referent(notice) !== mine)
        assertSame(second, all.referent(notice))
    }
}
