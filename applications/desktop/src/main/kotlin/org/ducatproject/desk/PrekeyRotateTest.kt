package org.ducatproject.desk

import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.securePrefs
import org.json.JSONObject

/**
 * The signed prekey's term, and what still opens after it ends.
 * `./gradlew :desktop:prekeyrotate`.
 *
 * `hpke.rs` says the signed-prekey fallback is forward-secret "only from the
 * next rotation". There was no next rotation: the key was written once and the
 * store refused to overwrite it, so on a phone whose contact had run out of
 * one-time keys, every message for the life of the install was sealed to one
 * key. The refusal was not arbitrary — overwriting it turned messages already
 * in flight into BadSig, because peers seal to whatever bundle they last
 * cached. So the term is only half of it; the other half is that the retired
 * key keeps working, and these cases are mostly about that half.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-prekey").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)
    val k1 = ByteArray(32) { 1 }
    val k2 = ByteArray(32) { 2 }
    val k3 = ByteArray(32) { 3 }

    fun blob() = JSONObject(securePrefs(ctx, "ducat_contacts").getString("prekeys", "{}")!!)
    fun age(field: String, ms: Long) {
        val o = blob().put(field, System.currentTimeMillis() - ms)
        securePrefs(ctx, "ducat_contacts").edit().putString("prekeys", o.toString()).apply()
    }
    fun secrets() = store.signedPrekeySecrets()

    // A first key is simply adopted.
    store.savePrekeys(ByteArray(0), k1, emptyMap())
    check(secrets().size == 1 && secrets()[0].contentEquals(k1)) {
        "PREKEY_FAIL the first signed prekey was not stored"
    }
    check(!store.signedPrekeyDue()) {
        "PREKEY_FAIL a key minted a moment ago was already due"
    }

    // Every incidental save leaves it alone: issuing a card, topping up a
    // thread. This is the rule that was protecting messages in flight, and it
    // has to survive the change that finally lets the key move.
    store.savePrekeys(ByteArray(0), k2, emptyMap())
    check(secrets()[0].contentEquals(k1)) {
        "PREKEY_FAIL a save that was not a rotation replaced the signed prekey"
    }
    // Including a save carrying nothing at all, which is what restore does.
    store.savePrekeys(ByteArray(0), ByteArray(0), emptyMap())
    check(secrets()[0].contentEquals(k1)) {
        "PREKEY_FAIL empty material erased the signed prekey"
    }

    // A term ends.
    age("signed_at", 31L * 24 * 60 * 60 * 1000)
    check(store.signedPrekeyDue()) { "PREKEY_FAIL a key past its term was not due" }

    // The rotation itself: the new key is offered, and **the old one still
    // opens**. A peer's cached bundle can be a month behind, and everything it
    // sealed in the meantime is addressed to the key just retired.
    store.savePrekeys(ByteArray(0), k2, emptyMap(), rotate = true)
    check(secrets().size == 2) { "PREKEY_FAIL the retired key was dropped on rotation" }
    check(secrets()[0].contentEquals(k2)) { "PREKEY_FAIL the new key is not the one offered" }
    check(secrets()[1].contentEquals(k1)) { "PREKEY_FAIL the retired key is not the old one" }
    check(!store.signedPrekeyDue()) { "PREKEY_FAIL a freshly rotated key was due again" }

    // Rotating to the key already in use is not a rotation, and must not
    // shunt a real previous key out of reach.
    store.savePrekeys(ByteArray(0), k2, emptyMap(), rotate = true)
    check(secrets().size == 2 && secrets()[1].contentEquals(k1)) {
        "PREKEY_FAIL re-saving the current key discarded the retired one"
    }

    // Past the grace window the old key stops being tried. This is where the
    // forward secrecy actually arrives: it is gone, not merely unpublished.
    age("signed_prev_at", 31L * 24 * 60 * 60 * 1000)
    check(secrets().size == 1 && secrets()[0].contentEquals(k2)) {
        "PREKEY_FAIL a key long past its grace window was still being offered"
    }

    // A second rotation retires k2 and forgets k1 — one predecessor, not a
    // pile. Anything sealed to k1 is beyond its window by construction.
    age("signed_at", 31L * 24 * 60 * 60 * 1000)
    store.savePrekeys(ByteArray(0), k3, emptyMap(), rotate = true)
    check(secrets().size == 2 && secrets()[0].contentEquals(k3) && secrets()[1].contentEquals(k2)) {
        "PREKEY_FAIL a second rotation did not retire the right key"
    }

    // An install that predates rotation has a key with no date on it at all.
    // It is due at once: it has been in service since the install, which is
    // the condition this whole change exists to end. Its grace window then
    // covers everything sealed to it while it was the only key there was.
    val legacyDir = kotlin.io.path.createTempDirectory("ducat-prekey-legacy").toFile()
    val legacyCtx = DeskContext(legacyDir)
    val legacy = ContactStore(legacyCtx)
    securePrefs(legacyCtx, "ducat_contacts").edit()
        .putString(
            "prekeys",
            JSONObject().put(
                "signed",
                android.util.Base64.encodeToString(k1, android.util.Base64.NO_WRAP),
            ).toString(),
        )
        .apply()
    check(legacy.signedPrekeyDue()) {
        "PREKEY_FAIL a key stored before rotation existed was never due"
    }
    legacy.savePrekeys(ByteArray(0), k2, emptyMap(), rotate = true)
    check(legacy.signedPrekeySecrets().let { it.size == 2 && it[1].contentEquals(k1) }) {
        "PREKEY_FAIL rotating an undated key left nothing to open its messages with"
    }

    // --- and what a reader does with two keys --------------------------------
    //
    // Rotation is only survivable if opening tries both, and the *reporting*
    // matters as much as the trying: everything downstream — dead-letter now,
    // or wait out the patience window — branches on which failure escapes.
    val tried = mutableListOf<String>()
    fun key(n: Int) = ByteArray(32) { n.toByte() }

    // The ordinary case: the current key opens it and the retired one is
    // never touched.
    tried.clear()
    check(
        org.ducatproject.ducat.Mailbox.openWithAny(listOf(key(1), key(2))) { k ->
            tried += "k${k[0]}"; "opened"
        } == "opened",
    ) { "PREKEY_FAIL the first key did not open it" }
    check(tried == listOf("k1")) { "PREKEY_FAIL it kept going after a key that worked" }

    // Sealed to the key we just retired — a peer working from a cached
    // bundle. This is the whole reason the retired key is kept.
    tried.clear()
    check(
        org.ducatproject.ducat.Mailbox.openWithAny(listOf(key(1), key(2))) { k ->
            tried += "k${k[0]}"
            if (k[0].toInt() == 1) throw IllegalStateException("BadSig") else "opened"
        } == "opened",
    ) { "PREKEY_FAIL a message sealed to the retired key was not opened" }
    check(tried == listOf("k1", "k2")) { "PREKEY_FAIL it did not fall through to the retired key" }

    // Nothing opens it: the failure that escapes is a real one, not swallowed.
    runCatching {
        org.ducatproject.ducat.Mailbox.openWithAny<String>(listOf(key(1), key(2))) {
            throw IllegalStateException("BadSig")
        }
    }.onSuccess { error("PREKEY_FAIL an unopenable message returned a value") }
        .onFailure { check(it.message == "BadSig") { "PREKEY_FAIL wrong failure escaped: $it" } }

    // **The one that is easy to get wrong.** The *current* key decrypts and
    // the bytes will not parse — final, and the caller dead-letters on it.
    // The retired key is then tried and fails at the seal. Keeping the last
    // error would replace a verdict with a guess, and a message that should
    // have been recorded and skipped would sit out the patience window first.
    runCatching {
        org.ducatproject.ducat.Mailbox.openWithAny<String>(listOf(key(1), key(2))) { k ->
            if (k[0].toInt() == 1) throw IllegalStateException("Malformed message")
            throw IllegalStateException("BadSig")
        }
    }.onFailure {
        check(it.message?.contains("Malformed") == true) {
            "PREKEY_FAIL a later wrong key overwrote the verdict of the right one: $it"
        }
    }
    // And the same the other way round, which is the ordinary ordering.
    runCatching {
        org.ducatproject.ducat.Mailbox.openWithAny<String>(listOf(key(1), key(2))) { k ->
            if (k[0].toInt() == 1) throw IllegalStateException("BadSig")
            throw IllegalStateException("Malformed message")
        }
    }.onFailure {
        check(it.message?.contains("Malformed") == true) {
            "PREKEY_FAIL the informative failure was not the one reported: $it"
        }
    }

    println(
        "PREKEY_ROTATE_OK adopt=ok incidental=kept empty=kept due=after-term " +
            "rotate=two-keys same=noop grace=expires second=one-predecessor " +
            "legacy=due-at-once open=first-that-works verdict=malformed-wins",
    )
}
