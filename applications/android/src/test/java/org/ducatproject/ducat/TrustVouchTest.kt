package org.ducatproject.ducat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * §9.2's vouches, the pure pieces: which bodies are vouches, one vouch per
 * mouth, who counts as knowing somebody, and how a persona's vouches are
 * packed to travel. Asserted here because the desk (`app/src/trust.rs`,
 * `a_vouch_counts_only_when_its_signer_is_one_of_the_readers_contacts`)
 * applies the same rules, and a phone that counted a stranger's vouch, or
 * its own, would be a second protocol.
 *
 * No `Context` and no bridge: these functions take lists and strings,
 * which is why they were written to.
 */
class TrustVouchTest {

    private val hex64 = "0f".repeat(32)
    private val pat = "aa".repeat(32)
    private val sam = "bb".repeat(32)
    private val stranger = "cc".repeat(32)
    private val me = "dd".repeat(32)
    private val x = "5e".repeat(32)

    private fun vouch(
        signer: String,
        subject: String = x,
        ts: Long = 1,
        envelope: String = "e" + ts.toString(16),
    ) = Trust.VouchRecord(signerHex = signer, subjectHex = subject, ts = ts, envelopeHex = envelope)

    // ----- the links -------------------------------------------------------------

    @Test
    fun `the two vouch prefixes are recognised, whole body only, and told apart`() {
        assertEquals(Trust.Link.Vouch(hex64), Trust.linkIn("ducat:vouch/$hex64"))
        assertEquals(Trust.Link.Vouches("$hex64.$hex64"), Trust.linkIn("ducat:vouches/$hex64.$hex64"))
        // `ducat:vouches/` is not `ducat:vouch/` with "es/…" as its hex.
        assertEquals(Trust.Link.Vouches(hex64), Trust.linkIn("ducat:vouches/$hex64"))
        // Trimmed, as a pasted link arrives.
        assertEquals(Trust.Link.Vouch(hex64), Trust.linkIn("  ducat:vouch/$hex64\n"))
        // The desk ingests by stripping the prefix from the trimmed body and
        // reading the rest; words after the link fail there, so here too.
        assertNull(Trust.linkIn("ducat:vouch/$hex64 hi"))
        assertNull(Trust.linkIn("ducat:vouch/"))
        assertNull(Trust.linkIn("ducat:vouch/$hex64.$hex64"))
        assertNull(Trust.linkIn("ducat:vouches/not-hex"))
        // The older three still read as themselves.
        assertEquals(Trust.Link.Record(hex64), Trust.linkIn("ducat:record/$hex64"))
    }

    // ----- one per mouth ---------------------------------------------------------

    @Test
    fun `one vouch per signer and subject, the newest replacing the last`() {
        val shelf = listOf(vouch(pat, ts = 10), vouch(sam, ts = 20), vouch(pat, subject = me, ts = 30))
        val again = vouch(pat, ts = 40, envelope = "fresh")
        val kept = Trust.withVouch(shelf, again)
        // Pat about X is one row, the new one; Sam, and Pat about somebody
        // else, are untouched.
        assertEquals(3, kept.size)
        assertEquals(listOf("fresh"), kept.filter { it.signerHex == pat && it.subjectHex == x }.map { it.envelopeHex })
        assertTrue(kept.any { it.signerHex == sam })
        assertTrue(kept.any { it.signerHex == pat && it.subjectHex == me })
        // Case does not separate a persona from itself.
        assertEquals(3, Trust.withVouch(kept, vouch(pat.uppercase(), subject = x.uppercase(), ts = 50)).size)
    }

    @Test
    fun `a given vouch is one per subject, whichever persona signed it`() {
        val given = listOf(vouch(me, subject = x, ts = 1), vouch(me, subject = sam, ts = 2))
        // A second persona of ours vouching for X replaces the first's.
        val other = "ee".repeat(32)
        val kept = Trust.givenWith(given, vouch(other, subject = x, ts = 3, envelope = "second"))
        assertEquals(2, kept.size)
        assertEquals(listOf("second"), kept.filter { it.subjectHex == x }.map { it.envelopeHex })
        assertTrue(kept.any { it.subjectHex == sam })
    }

    // ----- who counts ------------------------------------------------------------

    @Test
    fun `known-by counts only the reader's contacts and never its own personas`() {
        val about = listOf(
            vouch(pat, ts = 1),
            vouch(sam, ts = 2),
            vouch(stranger, ts = 3),
            vouch(me, ts = 4),
            // Pat vouching for somebody else says nothing about X.
            vouch(pat, subject = sam, ts = 5),
        )
        val names = mapOf(pat to "Pat", sam to "Sam", me to "Me")
        // Pat is a contact, Sam is not yet: one name.
        assertEquals(listOf("Pat"), Trust.knownByOf(about, x, setOf(me)) { if (it == pat) names[it] else null })
        // Both are contacts: two, sorted. The stranger has no name and is
        // not counted; our own vouch is not a contact's, name or no name.
        assertEquals(listOf("Pat", "Sam"), Trust.knownByOf(about, x, setOf(me)) { names[it] })
        // Two rows from one signer are one name.
        val twice = about + vouch(pat, ts = 9, envelope = "again")
        assertEquals(listOf("Pat", "Sam"), Trust.knownByOf(twice, x, setOf(me)) { names[it] })
        // Two contacts with the same petname are one word too — the desk dedups names.
        assertEquals(listOf("Pat"), Trust.knownByOf(about, x, setOf(me)) { if (it == stranger) null else "Pat" })
        // Nothing read about them: nobody.
        assertEquals(emptyList<String>(), Trust.knownByOf(about, "99".repeat(32), setOf(me)) { names[it] })
        // Case does not separate a persona from itself.
        assertEquals(listOf("Pat", "Sam"), Trust.knownByOf(about, x.uppercase(), setOf(me.uppercase())) { names[it] })
    }

    // ----- what a persona shows --------------------------------------------------

    @Test
    fun `the vouches link carries this persona's, newest first, packed to one message`() {
        val received = listOf(
            vouch(pat, subject = me, ts = 10, envelope = "aa10"),
            vouch(sam, subject = me, ts = 30, envelope = "bb30"),
            vouch(stranger, subject = "77".repeat(32), ts = 20, envelope = "cc20"),
            vouch(stranger, subject = me, ts = 20, envelope = "cc20"),
        )
        assertEquals("ducat:vouches/bb30.cc20.aa10", Trust.vouchesLinkOf(received, me))
        assertEquals("ducat:vouches/bb30.cc20.aa10", Trust.vouchesLinkOf(received, me.uppercase()))
        // Nobody has vouched for this persona yet.
        assertNull(Trust.vouchesLinkOf(received, "99".repeat(32)))
        assertNull(Trust.vouchesLinkOf(emptyList(), me))

        // Envelopes as long as a real one: a text is capped, so what fits
        // goes and the newest go first — the same packing as a record.
        val long = (1..10).map {
            vouch("%02x".format(it).repeat(32), subject = me, ts = it.toLong(), envelope = "%02x".format(it).repeat(300))
        }
        val link = Trust.vouchesLinkOf(long, me)!!
        assertTrue(link.length <= Trust.MAX_LINK_CHARS)
        val packed = link.removePrefix(Trust.VOUCHES_PREFIX).split('.')
        assertEquals(3, packed.size)
        assertEquals("0a".repeat(300), packed.first())
        // And never more than sixty-four, which is where a reader stops.
        val tiny = (1..100).map { vouch("%02x".format(it).repeat(32), subject = me, ts = it.toLong(), envelope = "%02x".format(it)) }
        val capped = Trust.vouchesLinkOf(tiny, me)!!.removePrefix(Trust.VOUCHES_PREFIX)
        assertEquals(Trust.MAX_RECORD_ENVELOPES, capped.split('.').size)
        assertEquals(Trust.MAX_RECORD_ENVELOPES, Trust.splitRecord(capped).size)
        // What the reader reads back is what was sent, in the order sent.
        assertEquals(Trust.linkIn(link), Trust.Link.Vouches(link.removePrefix(Trust.VOUCHES_PREFIX)))
    }

    // ----- the refusal -----------------------------------------------------------

    @Test
    fun `a vouch for oneself is refused before anything is signed`() {
        // The rule `Trust.vouch` applies before it looks for a key or asks
        // the bridge: the same hex, however it is cased or padded, is us.
        assertTrue(Trust.selfVouch(me, me))
        assertTrue(Trust.selfVouch(me, me.uppercase()))
        assertTrue(Trust.selfVouch(" $me\n", me))
        assertFalse(Trust.selfVouch(me, x))
        assertFalse(Trust.selfVouch(me, me.dropLast(2) + "00"))
    }
}
