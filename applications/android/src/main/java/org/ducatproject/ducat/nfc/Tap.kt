package org.ducatproject.ducat.nfc

import android.nfc.Tag
import android.nfc.tech.IsoDep
import android.nfc.tech.Ndef

/**
 * The tap, as this phone performs it: what it offers, and how it reads.
 *
 * The exchange itself is [TapWire], which knows nothing about Android so that
 * it can be checked against itself without two handsets. This is the half
 * that needs the hardware.
 */
object Tap {
    /**
     * What this phone is currently offering to a tap.
     *
     * Set by whichever screen is presenting — the code screen, a till, a tab,
     * a pickup, the kiosk — and cleared when it leaves. Null falls back to the
     * standing profile card, so a tap works from any screen at all: the
     * promise "NFC share button" is really "your phone *is* the button".
     *
     * Volatile because the APDU service is called on a binder thread while a
     * composable writes from the main one.
     */
    @Volatile
    var offered: String? = null

    /**
     * Read a card from whatever landed on the antenna.
     *
     * Two shapes, tried in order and **both** tried. An IsoDep peer is another
     * phone running this service: select, then walk the offsets. An Ndef tag
     * is a sticker — §15.9's static world — carrying a `ducat:` or `monero:`
     * URI as a standard NDEF record.
     *
     * They are not exclusive, which is the thing this got wrong: a Type 4 tag
     * — NTAG 4xx, DESFire, most of the programmable stickers somebody would
     * actually buy — is ISO-DEP underneath, so `IsoDep.get` returns non-null
     * for it. Giving up inside that branch meant selecting an application the
     * sticker has never heard of, being told so, and returning empty-handed
     * without ever asking it the NDEF question it was waiting for. Those
     * stickers could not be read at all.
     */
    fun read(tag: Tag): String? {
        IsoDep.get(tag)?.let { iso ->
            runCatching {
                iso.use {
                    it.connect()
                    it.timeout = TapWire.TIMEOUT_MS
                    TapWire.readOver { apdu -> it.transceive(apdu) }
                }
            }.getOrNull()?.let { return it }
        }
        Ndef.get(tag)?.let { ndef ->
            runCatching {
                ndef.use {
                    it.connect()
                    val msg = it.ndefMessage ?: return null
                    for (rec in msg.records) {
                        val uri = rec.toUri()?.toString()
                        if (uri != null &&
                            (uri.startsWith("ducat:") || uri.startsWith("monero:"))
                        ) return uri
                    }
                }
            }
        }
        return null
    }

}
