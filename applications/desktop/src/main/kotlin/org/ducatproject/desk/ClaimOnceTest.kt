package org.ducatproject.desk

import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox

/**
 * A card is answered once, and the sequence is what proves it.
 * `./gradlew :desktop:claimonce`.
 *
 * A DHT subkey is a mutable slot. `SMPL(1,[writer])` bounds how many subkeys a
 * member may write, not how many times, and the set helper retries against the
 * network's sequence so a later write wins. For a hail or a listing the card
 * URI — writer secret and all — is public board text, so anybody reading the
 * board could overwrite whoever answered and be adopted as the counterparty
 * instead, payment address included.
 *
 * What makes it checkable is that an honest claimant never overwrites:
 * `claimCard` reads the reply subkey first and throws `CardAlreadyUsed` if
 * anything is there. So one write is the only honest history the slot has.
 */
fun main() {
    // What one honest claim leaves behind: veilid numbers a subkey's first
    // write zero.
    check(Mailbox.claimedOnce(0u)) {
        "CLAIMONCE_FAIL a card answered exactly once was discarded"
    }
    // Somebody answered after somebody else.
    check(!Mailbox.claimedOnce(1u)) {
        "CLAIMONCE_FAIL a slot written twice was accepted"
    }
    // An attacker rewriting in a loop, which is what the cheap version of this
    // attack looks like.
    check(!Mailbox.claimedOnce(9u)) {
        "CLAIMONCE_FAIL a slot written ten times was accepted"
    }
    // No sequence reported at all. Deliberately permissive: a claim is not
    // evidence against itself, and refusing here would mean a node that
    // stopped reporting sequences could stop anybody pairing at all.
    check(Mailbox.claimedOnce(null)) {
        "CLAIMONCE_FAIL an unsequenced read was treated as contested"
    }

    // The card is *dropped*, not marked answered: nobody was adopted, and
    // `answered_by` names a contact. A card left in the registry would keep
    // the poller reading a slot whose answer can never be used.
    val dir = kotlin.io.path.createTempDirectory("ducat-claimonce").toFile()
    val store = ContactStore(DeskContext(dir))
    fun issue(key: String) = store.saveIssuedCard(
        inboxKey = key, writerPublic = ByteArray(32), writerSecret = ByteArray(32),
        outboxKey = "out_$key", outboxOwnerPublic = ByteArray(32),
        outboxOwnerSecret = ByteArray(32), uri = "ducat://$key", purpose = "profile",
    )
    issue("card_a")
    issue("card_b")
    check(store.issuedCards().size == 2) { "CLAIMONCE_FAIL setup: cards were not issued" }

    store.forgetIssuedCard("card_a")
    val left = store.issuedCards()
    check(left.size == 1 && left[0].inboxKey == "card_b") {
        "CLAIMONCE_FAIL discarding one card took the wrong one, or took both"
    }
    check(store.claimantOf("card_a") == null) {
        "CLAIMONCE_FAIL a discarded card still names a claimant"
    }
    // Idempotent: the poller can reach this twice for one card, and the second
    // pass must not throw its way out of collecting everything behind it.
    store.forgetIssuedCard("card_a")
    store.forgetIssuedCard("never_existed")
    check(store.issuedCards().size == 1) {
        "CLAIMONCE_FAIL discarding twice disturbed the registry"
    }

    println(
        "CLAIMONCE_OK once=kept twice=refused loop=refused unsequenced=kept " +
            "drop=exact idempotent=ok",
    )
}
