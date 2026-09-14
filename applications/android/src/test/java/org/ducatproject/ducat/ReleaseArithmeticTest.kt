package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The residual claimant's share, which is the whole of M2.
 *
 * A split release names its fixed slices on the wire and leaves the change
 * output's amount off it entirely, because that amount is
 * `inputs − fixed − fee` and only becomes a number once the transaction is
 * built. The consent screen used to substitute the escrow's **scanned
 * balance** for `inputs` — the same number only when the release sweeps the
 * whole escrow, and nothing made it. A proposer spending one of two notes
 * paid every fixed slice in full and halved the residual, which is the
 * co-signer's own stake, while every figure on the screen still added up.
 *
 * So the arithmetic is carved out and pinned here: the terms come from the
 * transaction, and terms that do not close are a refusal rather than a zero.
 */
class ReleaseArithmeticTest {

    /** An ordinary settlement: the payer's slice fixed, the payee residual. */
    @Test
    fun `the residual is what the inputs leave after the slices and the fee`() {
        assertEquals(
            700_000_000L,
            Ceremony.residualOf(
                inputsTotalPxmr = 1_000_000_000L,
                fixedPxmr = 200_000_000L,
                feePxmr = 100_000_000L,
            ),
        )
    }

    /** A sweep: nothing fixed, so the fee is the only deduction. */
    @Test
    fun `a sweep leaves everything but the fee`() {
        assertEquals(
            999_880_000_000L,
            Ceremony.residualOf(1_000_000_000_000L, 0L, 120_000_000L),
        )
    }

    /**
     * **The partial sweep, told apart from an honest one.**
     *
     * Both transactions below pay the payer the same fixed 0.5 and both are
     * internally valid. The honest one spends the whole escrow and leaves
     * the payee 0.4988; the partial one spends one note of two and leaves
     * them 0.0988 — a quarter of what the balance-based figure claimed. The
     * amounts come out different because the inputs do, which is exactly
     * why the inputs have to be read.
     */
    @Test
    fun `a partial sweep sizes the residual smaller than a full one`() {
        val fixed = 500_000_000_000L
        val fee = 1_200_000_000L
        val whole = Ceremony.residualOf(1_000_000_000_000L, fixed, fee)
        val half = Ceremony.residualOf(600_000_000_000L, fixed, fee)
        assertEquals(498_800_000_000L, whole)
        assertEquals(98_800_000_000L, half)
    }

    /**
     * Terms that do not close are null, never zero.
     *
     * The crate's own `validate` refuses such a transaction before this is
     * reached, so a null here means the walk and the crate disagree — which
     * is a finding, and a screen that rendered it as "you get nothing" would
     * be stating the disagreement as a settlement.
     */
    @Test
    fun `terms that do not close refuse instead of reading as nothing`() {
        assertNull(Ceremony.residualOf(100L, 90L, 20L))
        assertNull(Ceremony.residualOf(-1L, 0L, 0L))
        assertNull(Ceremony.residualOf(100L, -1L, 0L))
        assertNull(Ceremony.residualOf(100L, 0L, -1L))
        // And the overflow an unchecked sum would have wrapped through.
        assertNull(Ceremony.residualOf(Long.MAX_VALUE, Long.MAX_VALUE, Long.MAX_VALUE))
    }

    /** Nothing left over is a real answer, and distinct from the refusals. */
    @Test
    fun `an exactly spent escrow leaves nothing and says so`() {
        assertEquals(0L, Ceremony.residualOf(1_000L, 900L, 100L))
    }
}
