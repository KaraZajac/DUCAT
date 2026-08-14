package org.ducatproject.ducat.nfc

import android.nfc.cardemulation.HostApduService
import android.os.Bundle
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog

/**
 * The offering half of the tap (§15.3).
 *
 * Android routes any `SELECT` of §18.7's AID — `F0 44 55 43 41 54`, `0xF0` +
 * "DUCAT" — to this service while the screen is on, whatever the app is
 * showing. What it serves is [Tap.offered] — whichever card the visible screen
 * armed — falling back to the standing profile card, so a tap works from the
 * home screen as readily as from the code screen.
 *
 * The payload is bytes of a URI and nothing else. No decisions happen here: a
 * tap hands over the same card a QR shows, and the claim, the contact, the
 * money all happen where they already happen. A card is also **claim-once**
 * (§16.9), so serving it to a second reader is harmless — the DHT refuses the
 * second claim, not this service.
 */
class DucatHostApduService : HostApduService() {

    private var payload: ByteArray? = null

    override fun processCommandApdu(commandApdu: ByteArray?, extras: Bundle?): ByteArray {
        val apdu = commandApdu ?: return Tap.SW_UNKNOWN_INS

        if (Tap.isSelect(apdu)) {
            // Snapshotted at SELECT and held for the session: the reader walks
            // offsets into one consistent value, not into whatever the screen
            // swaps to mid-tap.
            val uri = Tap.offered ?: ContactStore(this).currentCardUri()
            if (uri == null) {
                DucatLog.i(TAG, "tap arrived with nothing to offer")
                return Tap.SW_NOTHING_OFFERED
            }
            val bytes = uri.toByteArray(Charsets.UTF_8)
            payload = bytes
            DucatLog.i(TAG, "tap: offering ${bytes.size} bytes")
            return byteArrayOf(
                (bytes.size shr 8).toByte(), (bytes.size and 0xFF).toByte(),
            ) + Tap.SW_OK
        }

        if (Tap.isRead(apdu)) {
            val bytes = payload ?: return Tap.SW_NOTHING_OFFERED
            val off = Tap.readOffset(apdu)
            if (off >= bytes.size) return Tap.SW_BAD_OFFSET
            val end = minOf(off + Tap.CHUNK, bytes.size)
            return bytes.copyOfRange(off, end) + Tap.SW_OK
        }

        return Tap.SW_UNKNOWN_INS
    }

    override fun onDeactivated(reason: Int) {
        // The field dropped; the session's snapshot goes with it.
        payload = null
    }

    private companion object {
        const val TAG = "Tap"
    }
}
