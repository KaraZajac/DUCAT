package org.ducatproject.ducat

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * §9.5's two pure pieces: what a burn must look like before it is one, and
 * the bytes its proof is bound to.
 *
 * The message is the load-bearing one. It is what Monero's out-proof signs,
 * so a phone that laid it out differently from `core/src/trust.rs` would
 * make proofs the desk — and every other implementation — refuses, and it
 * would find out only from a stranger who could not verify a burn that had
 * already cost real money. So the bytes are asserted here literally rather
 * than against a second copy of the same builder.
 */
class TrustRuleTest {

    private val persona = "00112233445566778899aabbccddeeff" +
        "0f1e2d3c4b5a69788796a5b4c3d2e1f0"

    @Test
    fun `the burn message is the domain, the persona and the purpose, zero separated`() {
        val message = Trust.burnMessage(persona, "identity")
        val domain = "DUCAT-BURN-v1".toByteArray(Charsets.US_ASCII)
        val expected = domain + byteArrayOf(0) +
            Trust.unhex(persona)!! + byteArrayOf(0) +
            "identity".toByteArray(Charsets.UTF_8)
        assertArrayEquals(expected, message)
        // 13 + 1 + 32 + 1 + 8 — the persona travels as 32 bytes, never as
        // the 64 characters it is written with.
        assertEquals(55, message.size)
    }

    @Test
    fun `a different name or purpose is a different message`() {
        val a = Trust.burnMessage(persona, "identity")
        val other = "ff".repeat(32)
        assert(!a.contentEquals(Trust.burnMessage(other, "identity")))
        assert(!a.contentEquals(Trust.burnMessage(persona, "listing")))
        // The purpose is trimmed before it is signed, so the label somebody
        // typed with a trailing space proves the same thing as the one
        // without it — and matches what the envelope will carry.
        assertArrayEquals(a, Trust.burnMessage(persona, " identity "))
    }

    @Test
    fun `a purpose in another script survives as UTF-8`() {
        val message = Trust.burnMessage(persona, "身元")
        val tail = "身元".toByteArray(Charsets.UTF_8)
        assertArrayEquals(tail, message.copyOfRange(message.size - tail.size, message.size))
    }

    @Test
    fun `the floor is a refusal, not a nudge`() {
        assertEquals(Trust.Refusal.UnderFloor, Trust.refusal(Trust.FLOOR_PXMR - 1, "identity"))
        assertEquals(Trust.Refusal.UnderFloor, Trust.refusal(0, "identity"))
        // Exactly the floor is a burn. 0.01 XMR, the price of a coffee.
        assertNull(Trust.refusal(Trust.FLOOR_PXMR, "identity"))
        assertEquals(10_000_000_000L, Trust.FLOOR_PXMR)
    }

    @Test
    fun `a purpose is a short label, and not an empty one`() {
        assertEquals(Trust.Refusal.NoPurpose, Trust.refusal(Trust.FLOOR_PXMR, ""))
        assertEquals(Trust.Refusal.NoPurpose, Trust.refusal(Trust.FLOOR_PXMR, "   "))
        assertNull(Trust.refusal(Trust.FLOOR_PXMR, "p".repeat(Trust.MAX_PURPOSE_CHARS)))
        assertEquals(
            Trust.Refusal.PurposeTooLong,
            Trust.refusal(Trust.FLOOR_PXMR, "p".repeat(Trust.MAX_PURPOSE_CHARS + 1)),
        )
        // Counted the way the wire counts it: characters, not UTF-16 units.
        // Sixteen emoji are sixteen characters here and thirty-two `.length`,
        // and refusing them would refuse a label the envelope accepts.
        assertNull(Trust.refusal(Trust.FLOOR_PXMR, "🔥".repeat(16)))
    }

    @Test
    fun `hex goes round and refuses what is not hex`() {
        val bytes = byteArrayOf(0x00, 0x0f, 0x7f, -1, -128)
        assertEquals("000f7fff80", Trust.hex(bytes))
        assertArrayEquals(bytes, Trust.unhex("000f7fff80"))
        assertArrayEquals(bytes, Trust.unhex("  000F7FFF80 "))
        assertNull(Trust.unhex("abc"))
        assertNull(Trust.unhex("zz"))
        assertNull(Trust.unhex("00 11"))
    }
}
