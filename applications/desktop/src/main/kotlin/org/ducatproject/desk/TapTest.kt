package org.ducatproject.desk

import org.ducatproject.ducat.nfc.TapWire

/**
 * The tap, both halves, without a radio.
 *
 * Two phones touching is the one thing in this app that cannot be tested from
 * a desk — but almost none of the tap is radio. It is a SELECT, a length, and
 * a walk of offsets in 250-byte chunks, and that is where the bugs live:
 * boundaries, status words, a peer that answers something unexpected. The
 * reader and the service now meet through a plain function, so they can meet
 * here too.
 *
 * What this does not cover, and what still needs two handsets: the antenna,
 * the field dropping mid-walk, and Android routing the AID to the service at
 * all.
 *
 * `./gradlew :desktop:taptest`
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("TAP ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    /** A reader talking to a card session, the way the antenna would. */
    fun tapAgainst(offering: String?): String? {
        val session = TapWire.Session()
        return TapWire.readOver { apdu -> session.respond(apdu) { offering } }
    }

    // A real card URI, which is the length this actually carries.
    val card = "ducat:card/" + "ogFYxagACQEBAgEYp1ggFFsMvEbqytwifUe1T5by7oQXjOiPMbkbK7YBQ8uyxQQYqHhcVkxE" +
        "MDpUNnU2eGNPbnU2MkhSZncwODU2b285Q2xTazZpNlB2SE1TRWEyUkRJaUs4OmJBV3JHUkJN" +
        "TkVBQXFNTkRRZ01HVlllRmllTkdINkNtVW9sS3RmZUNCM2sYqVggyPgEnWvPspFTh2kZJQ6j" +
        "0bBPPT3XAaZnb5j_JsPcLQcYqmxDb3JuZXIgQ2Fmw6kYqxpqhi_KAlhAF8Mky2d_bGuS9Q2_" +
        "h2t-nHIbPZqT2-k7GYaoV97jXPSzm2M_42XWfio9Z-U_F7ON-rCUwpKHJJU1buJhIyJkDQ"

    check("a card comes back exactly as it went", tapAgainst(card) == card, "${card.length} chars")
    check("and it took more than one chunk", card.length > TapWire.CHUNK, "${card.length} chars")

    // The boundaries. A payload landing exactly on a chunk edge is where an
    // off-by-one lives: the walk must stop rather than ask for one more.
    listOf(1, TapWire.CHUNK - 1, TapWire.CHUNK, TapWire.CHUNK + 1, TapWire.CHUNK * 2, TapWire.CHUNK * 2 + 1)
        .forEach { n ->
            val payload = "d".repeat(n)
            check("a $n-byte card round-trips", tapAgainst(payload) == payload)
        }

    // Nothing armed and no standing code: the phone is present and offering
    // nothing, which is a different answer from not being there.
    check("a phone with nothing to offer reads as nothing", tapAgainst(null) == null)

    // Multi-byte characters must not be split by a chunk boundary in a way
    // that corrupts them — the walk is over bytes and the join is over bytes,
    // so this is really a check that nobody decodes a chunk on its own.
    val emoji = "ducat:card/" + "é☕".repeat(120)
    check(
        "a card of multi-byte characters survives the chunking",
        tapAgainst(emoji) == emoji,
        "${emoji.toByteArray(Charsets.UTF_8).size} bytes in ${emoji.length} chars",
    )

    // A peer that is not us. Android answers 6A82 for an AID it does not
    // route, and a sticker answers whatever it likes.
    check(
        "an application that is not ours reads as nothing",
        TapWire.readOver { byteArrayOf(0x6A, 0x82.toByte()) } == null,
    )
    check("a truncated answer reads as nothing", TapWire.readOver { byteArrayOf(0x90.toByte()) } == null)
    check(
        "a length with no body reads as nothing",
        TapWire.readOver { apdu ->
            if (TapWire.isSelect(apdu)) byteArrayOf(0x01, 0x00, 0x90.toByte(), 0x00)
            else byteArrayOf(0x90.toByte(), 0x00)
        } == null,
    )
    // A peer answering more than it was asked for used to walk off the end of
    // the buffer and take the whole read down with it.
    check(
        "a peer that over-answers is refused, not crashed",
        runCatching {
            TapWire.readOver { apdu ->
                if (TapWire.isSelect(apdu)) byteArrayOf(0x00, 0x10, 0x90.toByte(), 0x00)
                else ByteArray(200) { 'x'.code.toByte() } + TapWire.SW_OK
            }
        }.getOrElse { "threw: $it" } == null,
    )

    // The session holds what it had at SELECT, so a screen swapping its card
    // mid-tap cannot splice two cards into one.
    run {
        var offering = card
        val session = TapWire.Session()
        val got = TapWire.readOver { apdu ->
            session.respond(apdu) { offering }.also { offering = "ducat:card/something-else" }
        }
        check("a card swapped mid-tap does not splice", got == card, got ?: "null")
    }

    // The field dropped between the SELECT and the walk.
    run {
        val session = TapWire.Session()
        val got = TapWire.readOver { apdu ->
            session.respond(apdu) { card }.also { if (TapWire.isSelect(apdu)) session.ended() }
        }
        check("a tap broken off mid-walk reads as nothing, not as half a card", got == null)
    }

    println(if (failures == 0) "TAPTEST OK" else "TAPTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
