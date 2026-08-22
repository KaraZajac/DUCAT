package org.ducatproject.desk

import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.foldCardAddress

/**
 * What a card is allowed to do to where somebody gets paid.
 * `./gradlew :desktop:cardaddress`.
 *
 * A card's details carry a persona and no signature over it, and
 * ContactStore.add replaces a contact wholesale by persona — so before this,
 * a card naming an existing contact and a different payto silently redirected
 * every future payment to that contact. §16.12's rotation is the authenticated
 * channel and stays untouched; this is only about the unsigned one.
 *
 * The cases below are as much about what must still work: a card is how most
 * contacts get an address in the first place, and somebody who lost their
 * phone comes back on a new card with a dead thread behind them.
 */
fun main() {
    fun c(addr: String?, pending: String? = null) = Contact(
        personaHex = "aa".repeat(32), petname = null, assertedName = "Sam",
        myOutbox = "mine", theirOutbox = "theirs",
        theirAddress = addr, pendingAddress = pending,
    )

    // Nobody yet: a card is how an address arrives at all.
    check(foldCardAddress(null, "5NEW") == ("5NEW" to null)) {
        "CARDADDR_FAIL a first card could not establish an address"
    }
    // Known contact, but we never had an address. Nothing is being replaced.
    check(foldCardAddress(c(null), "5NEW") == ("5NEW" to null)) {
        "CARDADDR_FAIL a card could not fill in a missing address"
    }
    check(foldCardAddress(c(""), "5NEW") == ("5NEW" to null)) {
        "CARDADDR_FAIL a blank stored address was treated as worth protecting"
    }

    // The attack: a card that names an existing contact and a new payto.
    // Payments keep going where they were going.
    check(foldCardAddress(c("5OLD"), "5EVIL") == ("5OLD" to "5EVIL")) {
        "CARDADDR_FAIL a card moved an address that was already working"
    }

    // Saying nothing about payment changes nothing — including a hold that is
    // still waiting for an answer.
    check(foldCardAddress(c("5OLD", "5EVIL"), null) == ("5OLD" to "5EVIL")) {
        "CARDADDR_FAIL a silent card dropped a hold the user had not answered"
    }
    check(foldCardAddress(c("5OLD", "5EVIL"), "") == ("5OLD" to "5EVIL")) {
        "CARDADDR_FAIL a blank payto was read as a claim"
    }

    // A card that agrees settles an outstanding hold: the contact re-carding
    // with the old address is them disowning the new one.
    check(foldCardAddress(c("5OLD", "5EVIL"), "5OLD") == ("5OLD" to null)) {
        "CARDADDR_FAIL agreement did not clear the hold"
    }

    // A second attacker card replaces the hold rather than stacking. Whatever
    // is held is what the user will be shown, so it must be the latest claim.
    check(foldCardAddress(c("5OLD", "5EVIL"), "5EVIL2") == ("5OLD" to "5EVIL2")) {
        "CARDADDR_FAIL a later claim did not supersede the held one"
    }

    // And the store verbs, which is where the user's answer lands.
    val dir = kotlin.io.path.createTempDirectory("ducat-cardaddr").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)
    val hex = "aa".repeat(32)

    store.add(c("5OLD", "5EVIL"))
    store.dismissPendingAddress(hex)
    store.all().first { it.personaHex == hex }.let {
        check(it.theirAddress == "5OLD" && it.pendingAddress == null) {
            "CARDADDR_FAIL dismissing changed the address: ${it.theirAddress}/${it.pendingAddress}"
        }
    }

    store.add(c("5OLD", "5NEW"))
    store.acceptPendingAddress(hex)
    store.all().first { it.personaHex == hex }.let {
        check(it.theirAddress == "5NEW" && it.pendingAddress == null) {
            "CARDADDR_FAIL accepting did not take: ${it.theirAddress}/${it.pendingAddress}"
        }
    }

    // Accepting nothing is not an error and does not blank the address — the
    // button is reachable from a screen that may have been open for a while.
    store.acceptPendingAddress(hex)
    check(store.all().first { it.personaHex == hex }.theirAddress == "5NEW") {
        "CARDADDR_FAIL accepting an empty hold destroyed the address"
    }

    // It has to survive being written down, or a hold evaporates on restart
    // and the next card lands on a contact that looks untouched.
    val round = Contact.from(c("5OLD", "5EVIL").toJson())
    check(round.pendingAddress == "5EVIL" && round.theirAddress == "5OLD") {
        "CARDADDR_FAIL the hold did not survive a JSON round trip"
    }

    println("CARDADDR_OK establish=ok replace=held agree=clears accept/dismiss=ok json=ok")
}
