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

    // The exchange itself lives in Tap.Session, which knows nothing about
    // Android and can therefore be driven by a test against the same reader
    // that will meet it on a real antenna.
    private val session = TapWire.Session()

    override fun processCommandApdu(commandApdu: ByteArray?, extras: Bundle?): ByteArray {
        val apdu = commandApdu ?: return TapWire.SW_UNKNOWN_INS
        return session.respond(apdu) {
            (Tap.offered ?: ContactStore(this).currentCardUri())
                .also {
                    if (it == null) DucatLog.i(TAG, "tap arrived with nothing to offer")
                    else if (TapWire.isSelect(apdu)) DucatLog.i(TAG, "tap: offering ${it.length} chars")
                }
        }
    }

    override fun onDeactivated(reason: Int) {
        // The field dropped; the session's snapshot goes with it.
        session.ended()
    }

    private companion object {
        const val TAG = "Tap"
    }
}
