package org.ducatproject.ducat.nfc

/**
 * The phone's HCE card, desk edition — somewhere to put the offer, and
 * nothing that radiates.
 *
 * A till screen sets `Tap.offered` to the card a customer's phone may read by
 * touching it (§15.3). A desk has no NFC controller, so the value is only
 * held: the same screen shows a QR, which is how a desk gets tapped. An
 * honest absence rather than a stub that pretends — nothing here reports a
 * tap that did not happen.
 */
object Tap {
    @Volatile
    var offered: String? = null
}
