package org.ducatproject.ducat.nfc

/**
 * The tap on the wire (§15.3, §18.7): the exchange itself, with no radio in
 * it and no Android under it.
 *
 * What crosses the air gap is deliberately tiny — the `ducat:` card URI, the
 * same ~400 bytes a QR carries — because tap is **presence-only**: it proves
 * two phones touched, and everything that follows (profile, bill, receipt)
 * rides the mailbox the card opens (§16.12). A tap that tried to carry the
 * conversation would need both phones present for all of it, which is exactly
 * the constraint the mailbox exists to remove.
 *
 * ## The exchange, as §18.7 pins it
 *
 * Reader → `SELECT` the AID (`00 A4 04 00 06 F0 44 55 43 41 54 00`). The
 * response data is the payload length as two big-endian bytes, so the reader
 * knows when it is done; `6985` means the phone is present but offering
 * nothing; `6A82` never arrives from us — Android answers it for unrouted
 * AIDs.
 *
 * Reader → `READ BINARY` (`00 B0 <off_hi> <off_lo> 00`), repeated. Each
 * returns up to [CHUNK] bytes of the UTF-8 URI at that offset, `9000`
 * appended; an offset at or past the end is `6B00`. Two round trips for a
 * typical card; a tap is comfortably under half a second.
 *
 * Plain ISO 7816 verbs rather than anything invented, because an iOS reader
 * (O19: iPhones can read HCE, never emulate) can speak this with the system
 * APIs as they are.
 *
 * **Separated from the radio on purpose.** Both ends of this — the walk a
 * reader does and the answers a card gives — are ordinary functions over byte
 * arrays, and that is where the bugs are: offsets, chunk boundaries, lengths,
 * status words, a peer that answers something unexpected. Two phones touching
 * cannot be tested from a desk; this can, and taptest runs the two halves
 * against each other. The phone's `Tap` supplies the antenna.
 */
object TapWire {
    /** §18.7: `0xF0` + "DUCAT", ISO/IEC 7816-5's registration-free range. */
    val AID: ByteArray = byteArrayOf(
        0xF0.toByte(), 0x44, 0x55, 0x43, 0x41, 0x54,
    )

    const val CHUNK = 250

    val SW_OK = byteArrayOf(0x90.toByte(), 0x00)
    val SW_NOTHING_OFFERED = byteArrayOf(0x69, 0x85.toByte())
    val SW_BAD_OFFSET = byteArrayOf(0x6B, 0x00)
    val SW_UNKNOWN_INS = byteArrayOf(0x6D, 0x00)

    /**
     * What this phone is currently offering to a tap.
     *
     * Set by whichever screen is presenting — the code screen, a till, a tab,
     * a pickup — and cleared when it leaves. Null falls back to the standing
     * profile card, so a tap works from any screen at all: the promise "NFC
     * share button" is really "your phone *is* the button".
     *
     * Volatile because the APDU service is called on a binder thread while a
     * composable writes from the main one.
     */
    @Volatile
    var offered: String? = null

    fun selectApdu(): ByteArray =
        byteArrayOf(0x00, 0xA4.toByte(), 0x04, 0x00, AID.size.toByte()) + AID + byteArrayOf(0x00)

    fun readApdu(offset: Int): ByteArray = byteArrayOf(
        0x00, 0xB0.toByte(), (offset shr 8).toByte(), (offset and 0xFF).toByte(), 0x00,
    )

    fun isSelect(apdu: ByteArray): Boolean =
        apdu.size >= 5 && apdu[1] == 0xA4.toByte() && apdu[4].toInt() == AID.size &&
            apdu.size >= 5 + AID.size &&
            apdu.copyOfRange(5, 5 + AID.size).contentEquals(AID)

    fun isRead(apdu: ByteArray): Boolean = apdu.size >= 4 && apdu[1] == 0xB0.toByte()

    fun readOffset(apdu: ByteArray): Int =
        ((apdu[2].toInt() and 0xFF) shl 8) or (apdu[3].toInt() and 0xFF)

    private fun sw(b: ByteArray, of: ByteArray) = b.contentEquals(of)

    /**
     * How long to give the other phone to answer.
     *
     * The default varies by device and is as low as 300 ms on some, which is
     * not enough when the peer's card service has to be started to answer the
     * SELECT — a cold process launch on a phone that has been in a pocket. A
     * tap that fails because the reader gave up first looks to both people
     * like the tap did not happen.
     */
    const val TIMEOUT_MS = 3_000

    /**
     * Walk the card off a peer, given something that speaks APDUs.
     *
     * Separated from the radio so the exchange can be checked against the
     * service that answers it, which is the half where the bugs are: offsets,
     * chunk boundaries, lengths, status words. [read] supplies a real IsoDep;
     * the test supplies a [Session].
     *
     * Null means "no card here" for every reason — nothing offered, a
     * different application, a truncated answer. The caller's screen stays up
     * and the person taps again.
     */
    fun readOver(transceive: (ByteArray) -> ByteArray): String? {
        val sel = transceive(selectApdu())
        if (sel.size < 2) return null
        val status = sel.copyOfRange(sel.size - 2, sel.size)
        if (!sw(status, SW_OK)) return null
        val total =
            if (sel.size >= 4) ((sel[0].toInt() and 0xFF) shl 8) or (sel[1].toInt() and 0xFF)
            else 0
        if (total == 0 || total > 8 * 1024) return null
        val out = ByteArray(total)
        var off = 0
        while (off < total) {
            val r = transceive(readApdu(off))
            if (r.size <= 2) return null
            if (!sw(r.copyOfRange(r.size - 2, r.size), SW_OK)) return null
            val body = r.copyOfRange(0, r.size - 2)
            // A peer that answers more than was asked for would otherwise walk
            // off the end of the buffer. It is our own service on the other
            // side today; it will not always be.
            if (off + body.size > total) return null
            body.copyInto(out, off)
            off += body.size
        }
        return String(out, Charsets.UTF_8)
    }

    /**
     * One tap, from the side that is offering.
     *
     * The state a card session needs — the payload, snapshotted at SELECT so
     * the reader walks one consistent value rather than whatever the screen
     * swapped to mid-tap — with none of the Android service around it, so the
     * exchange can be driven from a test.
     */
    class Session {
        private var payload: ByteArray? = null

        /** What to answer, and null when the field drops. */
        fun respond(apdu: ByteArray, offering: () -> String?): ByteArray {
            if (isSelect(apdu)) {
                val uri = offering() ?: return SW_NOTHING_OFFERED
                val bytes = uri.toByteArray(Charsets.UTF_8)
                payload = bytes
                return byteArrayOf(
                    (bytes.size shr 8).toByte(), (bytes.size and 0xFF).toByte(),
                ) + SW_OK
            }
            if (isRead(apdu)) {
                val bytes = payload ?: return SW_NOTHING_OFFERED
                val off = readOffset(apdu)
                if (off >= bytes.size) return SW_BAD_OFFSET
                return bytes.copyOfRange(off, minOf(off + CHUNK, bytes.size)) + SW_OK
            }
            return SW_UNKNOWN_INS
        }

        fun ended() { payload = null }
    }
}
