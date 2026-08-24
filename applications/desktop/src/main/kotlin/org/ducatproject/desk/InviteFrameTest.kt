package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony

/**
 * The round-0 invite frame, read the way it was written.
 * `./gradlew :desktop:inviteframe`.
 *
 * A length byte goes out unsigned — `out.write(size)` is the low eight bits —
 * and came back through `Byte.toInt()`, which sign-extends. So every length
 * from 128 up arrived negative, and `copyOfRange(p, p - k)` threw
 * IllegalArgumentException from the one function whose whole contract is that
 * malformed input returns null. The caller's runCatching contained it, so it
 * cost a confusing log line rather than a crash — but the contract was false,
 * and the next caller might not catch.
 *
 * These cases are mostly about what must still parse: this frame is how two
 * phones agree what an escrow *is*, and a reader that rejects an honest invite
 * strands money rather than protecting it.
 */
fun main() {
    val roster = listOf("aa".repeat(32), "bb".repeat(32))
    val refund = "5" + "A".repeat(94)          // a stagenet address, 95 chars
    val good = Ceremony.frameRound0(
        roster, 0, 1, 1, 800_000L, refund, 0L, 0L, "nonce-1", ByteArray(48) { 7 },
    )

    // The honest frame round-trips, field for field.
    val back = Ceremony.parseRound0(good) ?: error("INVITE_FAIL an honest invite did not parse")
    check(back.roster == roster) { "INVITE_FAIL roster: ${back.roster}" }
    check(back.refundAddr == refund) { "INVITE_FAIL refund: ${back.refundAddr}" }
    check(back.farePxmr == 800_000L) { "INVITE_FAIL fare: ${back.farePxmr}" }
    check(back.nonce == "nonce-1") { "INVITE_FAIL nonce: ${back.nonce}" }
    check(back.commitment.size == 48) { "INVITE_FAIL commitment: ${back.commitment.size}" }

    // **The bug.** A 200-byte refund field is longer than any real address, but
    // it is a length the writer can produce and the reader has to survive: it
    // used to arrive as -56 and throw. Null is the answer; an exception is not.
    val longRefund = "5".repeat(200)
    val longFrame = Ceremony.frameRound0(
        roster, 0, 1, 1, 800_000L, longRefund, 0L, 0L, "n", ByteArray(48) { 7 },
    )
    val longBack = runCatching { Ceremony.parseRound0(longFrame) }
    check(longBack.isSuccess) {
        "INVITE_FAIL a 200-byte field threw instead of parsing: ${longBack.exceptionOrNull()}"
    }
    check(longBack.getOrNull()?.refundAddr == longRefund) {
        "INVITE_FAIL a length above 127 did not read back unsigned"
    }

    // Every single-byte corruption, at every offset: null or a value, never a
    // throw. This is the sweep that would have caught it.
    var threw = 0
    var parsed = 0
    for (i in good.indices) {
        for (v in intArrayOf(0x00, 0x01, 0x7F, 0x80, 0xC0, 0xFF)) {
            val bent = good.copyOf().also { it[i] = v.toByte() }
            when (val r = runCatching { Ceremony.parseRound0(bent) }) {
                else -> if (r.isFailure) {
                    threw++
                    if (threw <= 3) {
                        println("INVITE  threw at offset $i with 0x${v.toString(16)}: " +
                            "${r.exceptionOrNull()}")
                    }
                } else if (r.getOrNull() != null) parsed++
            }
        }
    }
    check(threw == 0) { "INVITE_FAIL $threw corrupted frames threw instead of returning null" }

    // And truncation at every length, same rule.
    var truncThrew = 0
    for (cut in 0 until good.size) {
        if (runCatching { Ceremony.parseRound0(good.copyOf(cut)) }.isFailure) truncThrew++
    }
    check(truncThrew == 0) { "INVITE_FAIL $truncThrew truncated frames threw" }

    println(
        "INVITE_OK roundtrip=ok unsigned=200B corruptions=${good.size * 6} throws=0 " +
            "(of which still parsed=$parsed) truncations=${good.size} throws=0",
    )
}
