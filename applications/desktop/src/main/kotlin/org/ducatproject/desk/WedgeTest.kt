package org.ducatproject.desk

import uniffi.ducat_mobile.sealedPrekeyId

/**
 * One junk write must not end a conversation. `./gradlew :desktop:wedge`
 *
 * A peer's log is a DHT record they own, so they can put anything in the next
 * slot. Reading it went `sealedPrekeyId(raw)` seventy lines *above* the
 * handler that exists for exactly this, so anything that was not a well-formed
 * SealedMessage threw past pollOne, was swallowed by the per-contact catch,
 * and left the read cursor where it was. Every later poll repeated it byte for
 * byte — and every receipt, bill, ride accept and ceremony round behind it was
 * stuck too, because they all travel that one ordered log.
 *
 * What is checked here is the property the fix rests on: a decode failure is
 * *deterministic and reported as an exception this app can catch*, for every
 * shape of rubbish. The loop's own recovery is then a dead letter and a
 * cursor advance, which the Mailbox code does inline.
 */
fun main() {
    val cases = listOf(
        "empty" to ByteArray(0),
        "one byte" to byteArrayOf(0x00),
        "truncated map" to byteArrayOf(0xA5.toByte()),
        "trailing bytes" to byteArrayOf(0xA0.toByte(), 0x01, 0x02, 0x03),
        "not cbor at all" to "hello there".toByteArray(),
        "a big run of zeros" to ByteArray(4096),
        "high bytes" to ByteArray(64) { 0xFF.toByte() },
        "valid cbor, wrong shape" to byteArrayOf(0x01),
    )
    for ((what, bytes) in cases) {
        val r = runCatching { sealedPrekeyId(bytes) }
        check(r.isFailure) { "WEDGE_FAIL $what was accepted as a sealed message" }
        val e = r.exceptionOrNull()!!
        check(e is uniffi.ducat_mobile.ContactException) {
            "WEDGE_FAIL $what threw ${e.javaClass.name}, which the poll loop does not catch"
        }
        // Deterministic: the same bytes must fail the same way, or "skip it,
        // it will never open" is not a safe thing to conclude.
        val again = runCatching { sealedPrekeyId(bytes) }.exceptionOrNull()!!
        check(again.message == e.message) { "WEDGE_FAIL $what is not deterministic" }
    }

    // And the thing that made the old ladder miss them: a codec failure does
    // not say "Malformed". The fix does not depend on the wording any more —
    // this is here so that if somebody restores a substring test, it fails.
    val words = cases.mapNotNull {
        runCatching { sealedPrekeyId(it.second) }.exceptionOrNull()?.message
    }
    val recognised = words.count { m ->
        listOf("Malformed", "BadSig", "did not authenticate", "does not follow").any { m.contains(it) }
    }
    println(
        "WEDGE_OK shapes=${cases.size} all-throw-catchable deterministic " +
            "would-have-matched-old-ladder=$recognised/${words.size}",
    )
}
