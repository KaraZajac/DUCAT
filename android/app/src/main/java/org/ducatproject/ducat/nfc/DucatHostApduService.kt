package org.ducatproject.ducat.nfc

import android.nfc.cardemulation.HostApduService
import android.os.Bundle

/**
 * Presenting over NFC (§15.3).
 *
 * Registered against the AID §18.7 pins — `F0 44 55 43 41 54`, `0xF0` + "DUCAT",
 * in ISO/IEC 7816-5's registration-free proprietary range. Declared in
 * `res/xml/apduservice.xml` and never edited: the value cannot be discovered at
 * runtime, so changing it is a simultaneous update of every client that exists.
 *
 * Not implemented yet. The APDU exchange carries a `TapPresent`, and a tap that
 * half-works is worse than one that does not exist — so this refuses cleanly
 * until the bridge to `core` is in place rather than guessing at bytes the
 * protocol already specifies.
 */
class DucatHostApduService : HostApduService() {

    override fun processCommandApdu(commandApdu: ByteArray?, extras: Bundle?): ByteArray {
        // 0x6A82 — file or application not found. The honest answer while there
        // is nothing behind the AID.
        return byteArrayOf(0x6A.toByte(), 0x82.toByte())
    }

    override fun onDeactivated(reason: Int) {
        // A tap ends when the field drops. §15.3's budget is the user's wait, so
        // any session state belongs here rather than in a timeout.
    }
}
