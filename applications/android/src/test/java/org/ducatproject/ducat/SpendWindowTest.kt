package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * §15.5.1's rolling window, the part with no `Context` in it.
 *
 * The velocity rule exists because a per-payment limit alone does not stop
 * twenty payments just under it, which is how a lifted phone is actually
 * drained. It is only a *rolling* window if it forgets, and it only forgets
 * correctly if it also disbelieves a stamp from the future — a phone whose
 * clock was wound forward would otherwise hold the hour open for ever and
 * ask for a PIN on every payment from then on, with nothing on screen
 * saying why. Wound-forward clocks are not hypothetical here; see
 * `future-stamps-freeze-refresh`.
 */
class SpendWindowTest {

    private fun window(vararg rows: Pair<Long, Long>) = SpendGate.encode(rows.toList())

    private fun sum(raw: String?, now: Long, windowS: Long = 3_600) =
        SpendGate.inWindow(raw, now, windowS).sumOf { it.second }

    @Test
    fun `nothing recorded is nothing spent`() {
        assertEquals(0L, sum(null, 1_000_000))
        assertEquals(0L, sum("", 1_000_000))
    }

    @Test
    fun `payments inside the hour are counted`() {
        val now = 1_000_000L
        val raw = window((now - 10) to 500L, (now - 3_000) to 1_500L)
        assertEquals(2_000L, sum(raw, now))
    }

    @Test
    fun `a payment older than the window is forgotten`() {
        val now = 1_000_000L
        val raw = window((now - 3_601) to 9_999L, (now - 60) to 100L)
        assertEquals(100L, sum(raw, now))
    }

    @Test
    fun `the window slides rather than only growing`() {
        val at = 1_000_000L
        val raw = window(at to 5_000L)
        assertEquals(5_000L, sum(raw, at + 3_599))
        assertEquals(0L, sum(raw, at + 3_601))
    }

    @Test
    fun `a stamp from the future is disbelieved, with a minute of slack`() {
        val now = 1_000_000L
        // Clocks disagree by seconds; that is not a moved clock.
        assertEquals(50L, sum(window((now + 30) to 50L), now))
        // An hour ahead is.
        assertEquals(0L, sum(window((now + 3_600) to 50L), now))
    }

    @Test
    fun `rubbish in the store is no spend rather than a crash`() {
        assertEquals(0L, sum("not a window at all", 1_000_000))
        assertEquals(0L, sum(",,:,x:y,", 1_000_000))
        // A half-written row is dropped; the rows beside it still count.
        assertEquals(7L, sum("nonsense,${1_000_000L}:7", 1_000_000))
    }
}
