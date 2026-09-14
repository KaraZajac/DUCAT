package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * §9.5's seller's minimum, the two pure pieces: what the poster's field
 * becomes on the wire, and how a buyer's own burn stands against a
 * listing's `min_burn` — the sentence the buyer sees before Ask.
 *
 * Both are the desk's rules (`Market.svelte`), asserted here so the two
 * clients cannot quietly disagree about when a name is short of the line.
 */
class TrustMinimumTest {

    private val xmr = 1_000_000_000_000L

    @Test
    fun `nothing asked is met, whatever was burned`() {
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(null, 0L))
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(0L, 0L))
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(xmr, 0L))
        // A negative minimum is no minimum: the wire has no such number.
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(null, -1L))
    }

    @Test
    fun `at or over the line is met, exactly the line included`() {
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(xmr, xmr))
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(xmr + 1, xmr))
        assertEquals(Trust.Minimum.MET, Trust.againstMinimum(Trust.FLOOR_PXMR, Trust.FLOOR_PXMR))
    }

    @Test
    fun `under the line the sentence depends on whether anything was burned`() {
        // Burned something, less than asked: "This persona has burned X,
        // less than they ask."
        assertEquals(Trust.Minimum.SHORT, Trust.againstMinimum(xmr - 1, xmr))
        assertEquals(Trust.Minimum.SHORT, Trust.againstMinimum(1L, xmr))
        // Burned nothing: "This persona has burned nothing yet." — null and
        // zero are the same answer, as are the impossible negatives.
        assertEquals(Trust.Minimum.NOTHING, Trust.againstMinimum(null, xmr))
        assertEquals(Trust.Minimum.NOTHING, Trust.againstMinimum(0L, xmr))
        assertEquals(Trust.Minimum.NOTHING, Trust.againstMinimum(-5L, xmr))
    }

    @Test
    fun `empty asks nothing`() {
        assertEquals(0L, Listings.minBurnPxmrOf(""))
        assertEquals(0L, Listings.minBurnPxmrOf("   "))
        // A zero typed on purpose is the same as nothing typed: the wire
        // refuses a written zero, so it is simply not written.
        assertEquals(0L, Listings.minBurnPxmrOf("0"))
        assertEquals(0L, Listings.minBurnPxmrOf("0.0"))
    }

    @Test
    fun `a figure in XMR is piconero, read the way every money field is read`() {
        assertEquals(10_000_000_000L, Listings.minBurnPxmrOf("0.01"))
        // A comma is a decimal point here, as it is in the price field —
        // eleven of the shipped languages write it that way.
        assertEquals(10_000_000_000L, Listings.minBurnPxmrOf("0,01"))
        assertEquals(xmr, Listings.minBurnPxmrOf("1"))
        assertEquals(2_500_000_000_000L, Listings.minBurnPxmrOf(" 2.5 "))
        // Another script's digits, and its own decimal mark.
        assertEquals(10_000_000_000L, Listings.minBurnPxmrOf("٠٫٠١"))
        // More than twelve places is truncated, never refused.
        assertEquals(1L, Listings.minBurnPxmrOf("0.0000000000019"))
    }

    @Test
    fun `not a number, a negative, or too big to fit the wire is refused`() {
        assertNull(Listings.minBurnPxmrOf("abc"))
        assertNull(Listings.minBurnPxmrOf("1.2.3"))
        assertNull(Listings.minBurnPxmrOf("-1"))
        assertNull(Listings.minBurnPxmrOf("-0.01"))
        // 2^63 piconero does not fit a signed 64-bit integer, and the
        // parser must say so rather than hand back its low bits.
        assertNull(Listings.minBurnPxmrOf("9223372.036854775808"))
    }
}
