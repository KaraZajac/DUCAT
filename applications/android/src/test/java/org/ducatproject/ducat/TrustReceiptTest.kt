package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * §9.2's pure pieces: which bodies are receipts, and what a reader may say
 * about a record. Both are asserted here rather than against a second copy
 * of the same code because the desk (`app/src/trust.rs`) applies the same
 * rules, and a phone that counted a signer twice, or read a link the desk
 * would not, would be a second protocol.
 *
 * No `Context` and no bridge: these functions take lists and strings, which
 * is why they were written to.
 */
class TrustReceiptTest {

    private val hex64 = "0f".repeat(32)
    private val alice = "aa".repeat(32)
    private val bob = "bb".repeat(32)
    private val carol = "cc".repeat(32)
    private val subject = "5e".repeat(32)

    private fun receipt(
        signer: String,
        rating: Int,
        ts: Long,
        about: String = subject,
        envelope: String = "e".repeat(6) + ts.toString(16),
    ) = Trust.AttestationRecord(
        signerHex = signer, subjectHex = about, amountPxmr = 1_000_000_000_000L,
        rating = rating, ts = ts, txidHex = null, note = null, envelopeHex = envelope,
    )

    // ----- the links -------------------------------------------------------------

    @Test
    fun `the three prefixes are recognised, and nothing else is`() {
        assertEquals(Trust.Link.Burn(hex64), Trust.linkIn("ducat:burn/$hex64"))
        assertEquals(Trust.Link.Attest(hex64), Trust.linkIn("ducat:attest/$hex64"))
        assertEquals(Trust.Link.Record("$hex64.$hex64"), Trust.linkIn("ducat:record/$hex64.$hex64"))
        // Trimmed: a pasted link arrives with the newline the clipboard gave it.
        assertEquals(Trust.Link.Burn(hex64), Trust.linkIn("  ducat:burn/$hex64\n"))
        // Upper-case hex is still hex; the reader lowercases what it keeps.
        assertEquals(Trust.Link.Attest("ABCD"), Trust.linkIn("ducat:attest/ABCD"))
        assertNull(Trust.linkIn("ducat:card/$hex64"))
        assertNull(Trust.linkIn("ducat:site/$hex64"))
        assertNull(Trust.linkIn("see my proof ducat:burn/$hex64"))
        assertNull(Trust.linkIn("hello"))
        assertNull(Trust.linkIn(""))
    }

    @Test
    fun `a link is the whole body or it is not a link`() {
        // The desk ingests by stripping the prefix from the trimmed body and
        // reading the rest as hex; words after the link would make it fail
        // there, so they make it fail here too, on both the shelf and the
        // screen — the two clients agree about which messages were receipts.
        assertNull(Trust.linkIn("ducat:attest/$hex64 thanks!"))
        assertNull(Trust.linkIn("ducat:burn/"))
        assertNull(Trust.linkIn("ducat:burn/not-hex"))
        assertNull(Trust.linkIn("ducat:record/$hex64,$hex64"))
    }

    @Test
    fun `a record splits on dots, skips empties, and stops at sixty-four`() {
        assertEquals(listOf("aa", "bb"), Trust.splitRecord("aa.bb"))
        assertEquals(listOf("aa", "bb"), Trust.splitRecord(".aa..bb."))
        val many = (1..100).joinToString(".") { "%02x".format(it) }
        assertEquals(Trust.MAX_RECORD_ENVELOPES, Trust.splitRecord(many).size)
        assertEquals("01", Trust.splitRecord(many).first())
        assertEquals("40", Trust.splitRecord(many).last())
    }

    // ----- the arithmetic --------------------------------------------------------

    @Test
    fun `one voice per signer, rated by the latest`() {
        val about = listOf(
            receipt(alice, 5, 1_700_000_000),
            receipt(alice, 3, 1_700_000_001),
            receipt(alice, 4, 1_699_999_999),
        )
        val s = Trust.summarize(about) { it == alice }
        // Three receipts read; one burned signer; her latest says three.
        assertEquals(Trust.RecordSummary(receipts = 3, weighted = 1, ratingX10 = 30), s)
    }

    @Test
    fun `only signers whose burn this phone verified are weighed`() {
        val about = listOf(
            receipt(alice, 5, 1_700_000_000),
            receipt(bob, 1, 1_700_000_001),
            receipt(carol, 4, 1_700_000_002),
        )
        // Nobody verified: everything is counted, nothing is weighed.
        assertEquals(Trust.RecordSummary(receipts = 3, weighted = 0, ratingX10 = 0), Trust.summarize(about) { false })
        // Two of three verified: the mean is over those two, times ten.
        val burned = setOf(alice, carol)
        assertEquals(Trust.RecordSummary(receipts = 3, weighted = 2, ratingX10 = 45), Trust.summarize(about) { it in burned })
        // Bob's one star never enters the average, however many he writes.
        val padded = about + (1..10).map { receipt(bob, 1, 1_700_000_100L + it) }
        assertEquals(Trust.RecordSummary(receipts = 13, weighted = 2, ratingX10 = 45), Trust.summarize(padded) { it in burned })
    }

    @Test
    fun `the mean is integer tenths, the desk's sum over the desk's division`() {
        val about = listOf(
            receipt(alice, 5, 1),
            receipt(bob, 4, 2),
            receipt(carol, 5, 3),
        )
        // 140 / 3 = 46, not 47: truncated, as the desk truncates.
        assertEquals(46, Trust.summarize(about) { true }.ratingX10)
        assertEquals(Trust.RecordSummary(), Trust.summarize(emptyList()) { true })
    }

    // ----- what a persona shows --------------------------------------------------

    @Test
    fun `the record link carries only this persona's receipts, newest first`() {
        val received = listOf(
            receipt(alice, 5, 10, about = subject, envelope = "aa10"),
            receipt(bob, 4, 30, about = subject, envelope = "bb30"),
            receipt(carol, 3, 20, about = "77".repeat(32), envelope = "cc20"),
            receipt(carol, 3, 20, about = subject, envelope = "cc20"),
        )
        assertEquals("ducat:record/bb30.cc20.aa10", Trust.recordLinkOf(received, subject))
        // Case does not separate a persona from itself.
        assertEquals("ducat:record/bb30.cc20.aa10", Trust.recordLinkOf(received, subject.uppercase()))
        // Nothing on the record yet.
        assertNull(Trust.recordLinkOf(received, "99".repeat(32)))
        assertNull(Trust.recordLinkOf(emptyList(), subject))
    }

    @Test
    fun `a record is packed to fit one message and never exceeds sixty-four`() {
        // Envelopes as long as a real one, ~600 hex characters: a text is
        // capped at 2000, so three fit and the fourth would be refused by
        // the bridge — which is what would have happened to the whole send.
        val long = (1..10).map { receipt(alice.replaceRange(0, 2, "%02x".format(it)), 5, it.toLong(), envelope = "%02x".format(it).repeat(300)) }
        val link = Trust.recordLinkOf(long, subject)!!
        assertTrue(link.length <= Trust.MAX_LINK_CHARS)
        val packed = link.removePrefix(Trust.RECORD_PREFIX).split('.')
        assertEquals(3, packed.size)
        // The newest went first, so a reader rating by the latest sees the latest.
        assertEquals("0a".repeat(300), packed.first())

        val tiny = (1..100).map { receipt("%02x".format(it).repeat(32), 5, it.toLong(), envelope = "%02x".format(it)) }
        val capped = Trust.recordLinkOf(tiny, subject)!!
        assertEquals(Trust.MAX_RECORD_ENVELOPES, capped.removePrefix(Trust.RECORD_PREFIX).split('.').size)
        // And what the reader reads back is what was sent.
        assertEquals(Trust.MAX_RECORD_ENVELOPES, Trust.splitRecord(capped.removePrefix(Trust.RECORD_PREFIX)).size)
    }
}
