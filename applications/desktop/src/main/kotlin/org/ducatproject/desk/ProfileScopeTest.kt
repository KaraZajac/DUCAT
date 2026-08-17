package org.ducatproject.desk

import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.PersonaStore
import uniffi.ducat_mobile.buildContactDetails
import uniffi.ducat_mobile.parseContactDetails

/**
 * Proves §16.9's profile scope, headless and offline.
 *
 * Reach-me identifiers — email, phone, signal — are real-world locators like the
 * plate, and must ride only a deliberate contact exchange (purpose "profile"),
 * never a till, tab, ride or hail. The car rides only a driving handshake. The
 * purpose itself must survive the wire, so a claimant reading a "sale" card can
 * scope its own reply. This exercises the exact production path — MyProfile
 * .toWire, then build/parse_contact_details. `./gradlew :desktop:profilescope`.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-ps").toFile()
    val ctx = DeskContext(dir)
    val p = MyProfile(ctx)
    p.setName("Sam")
    p.setEmail("sam@example.com")
    p.setPhone("14155550123")
    p.setSignal("sam_oc.42")
    p.setCarModel("Prius")
    p.setCarColor("silver")
    p.setPlate("ABC123")
    p.setPronouns(5)

    // 1) A contact exchange carries the reach-me identifiers; no car when not driving.
    val prof = p.toWire(purpose = "profile", driving = false)
    check(prof.email == "sam@example.com" && prof.phone == "14155550123" && prof.signal == "sam_oc.42") {
        "PROFILESCOPE_FAIL profile handshake dropped a contact method: $prof"
    }
    check(prof.carModel == null && prof.plate == null) { "PROFILESCOPE_FAIL car rode a non-driving handshake" }
    check(prof.pronouns != null) { "PROFILESCOPE_FAIL pronouns dropped from a shared profile" }

    // 2) A sale carries NONE of the reach-me identifiers, and no car.
    val sale = p.toWire(purpose = "sale", driving = false)
    check(sale.email == null && sale.phone == null && sale.signal == null) {
        "PROFILESCOPE_FAIL sale leaked a contact method: $sale"
    }
    check(sale.carModel == null && sale.plate == null) { "PROFILESCOPE_FAIL sale carried a car" }
    // The face/pronouns are the low-cost gesture and still ride (share is on).
    check(sale.pronouns != null) { "PROFILESCOPE_FAIL sale dropped pronouns" }

    // 3) A null purpose — an older peer's card — is read as the private default.
    val unknown = p.toWire(purpose = null, driving = false)
    check(unknown.email == null && unknown.phone == null && unknown.signal == null) {
        "PROFILESCOPE_FAIL null purpose leaked a contact method: $unknown"
    }

    // 4) A hail claim (driving) carries the car — a rider scans the curb for it —
    //    but still no reach-me identifiers.
    val hail = p.toWire(purpose = "hail", driving = true)
    check(hail.carModel == "Prius" && hail.carColor == "silver" && hail.plate == "ABC123") {
        "PROFILESCOPE_FAIL hail dropped the car: $hail"
    }
    check(hail.email == null && hail.phone == null && hail.signal == null) {
        "PROFILESCOPE_FAIL hail leaked a contact method: $hail"
    }

    // 5) The purpose survives the wire, and the scoped profile travels with it.
    val persona = PersonaStore(ctx).secret()
    val saleBytes = buildContactDetails(persona, "VLD0:outbox", ByteArray(1) { 1 }, "Sam", null, sale, "sale")
    val salePeer = parseContactDetails(saleBytes)
    check(salePeer.purpose == "sale") { "PROFILESCOPE_FAIL purpose lost on the wire: ${salePeer.purpose}" }
    check(salePeer.profile.email == null && salePeer.profile.phone == null && salePeer.profile.signal == null) {
        "PROFILESCOPE_FAIL a sale record carried a contact method"
    }

    val profBytes = buildContactDetails(persona, "VLD0:outbox", ByteArray(1) { 1 }, "Sam", null, prof, "profile")
    val profPeer = parseContactDetails(profBytes)
    check(profPeer.purpose == "profile") { "PROFILESCOPE_FAIL profile purpose lost on the wire: ${profPeer.purpose}" }
    check(profPeer.profile.email == "sam@example.com") { "PROFILESCOPE_FAIL profile record dropped email" }

    println(
        "PROFILESCOPE_OK profile=[${prof.email},${prof.phone},${prof.signal}] " +
            "sale=[${sale.email},${sale.phone},${sale.signal}] " +
            "hail-car=${hail.carModel}/${hail.plate} " +
            "wire.purpose=${salePeer.purpose}/${profPeer.purpose}"
    )
}
