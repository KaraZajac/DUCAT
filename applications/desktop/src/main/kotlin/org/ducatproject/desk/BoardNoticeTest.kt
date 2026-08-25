package org.ducatproject.desk

import org.ducatproject.ducat.Posters
import uniffi.ducat_mobile.RentalInfo
import uniffi.ducat_mobile.rentalDecode
import uniffi.ducat_mobile.rentalEncode

/**
 * What a signature and a proof of work buy on a board nobody owns.
 * `./gradlew :desktop:boardnotice`.
 *
 * A stand's write key is the cell name hashed, so anyone who can find a board
 * can overwrite any slot on it, and nothing below changes that. What the two
 * features do is narrower and worth being precise about: a notice now says who
 * wrote it, and writing one costs something.
 *
 * This drives the real bridge — the same rental_encode and rental_decode the
 * phone calls — rather than the core in isolation, because the interesting
 * failures are at the seam: an encoder that signs for one slot and a reader
 * that checks another, a poster field a caller could set for themselves.
 */
fun main() {
    val persona = ByteArray(32) { it.toByte() }
    val board = "geo:u33dc"

    fun notice(title: String, expiry: ULong = 2_000_000_000uL) = RentalInfo(
        poster = "", card = "ducat:card/aaaa", kind = 1uL, title = title,
        area = "north side", cell = "u33dc", pricePxmr = 500_000uL,
        depositPxmr = 100_000uL, expiry = expiry,
        make = null, model = null, year = null, gearbox = null, fuel = null,
        seats = null, color = null, trim = null,
        rooms = 2uL, sleeps = null, sizeM2 = null, subtype = null, features = emptyList(),
        quantity = 1uL,
    )

    // The ordinary path: post to a slot, read it back from that slot.
    val a = rentalEncode(notice("Sunny room near the park"), persona, "listing-1", board, 3u)
    val readA = rentalDecode(a, board, 3u)
    check(readA.title == "Sunny room near the park") { "BOARD_FAIL round trip" }
    check(readA.poster.length == 64) { "BOARD_FAIL poster is '${readA.poster}'" }

    // The attack the slot binding exists for. A board's write key is public,
    // so an attacker holds every slot — without the binding they could take
    // one valid notice and paper the whole cell with it, and every copy would
    // verify as the original author.
    for (slot in listOf(0u, 2u, 4u, 7u)) {
        check(runCatching { rentalDecode(a, board, slot) }.isFailure) {
            "BOARD_FAIL a notice signed for slot 3 verified at slot $slot"
        }
    }
    check(runCatching { rentalDecode(a, "geo:u33dc-1", 3u) }.isFailure) {
        "BOARD_FAIL a notice moved to another shard of the same cell"
    }
    check(runCatching { rentalDecode(a, "geo:u33dd", 3u) }.isFailure) {
        "BOARD_FAIL a notice moved to another cell"
    }

    // Substitution: same content, an attacker's key. It verifies — it has to,
    // they signed it — but as a *different author*, which is the whole point.
    val attacker = ByteArray(32) { (it + 99).toByte() }
    val b = rentalEncode(notice("Sunny room near the park"), attacker, "listing-1", board, 3u)
    val readB = rentalDecode(b, board, 3u)
    check(readB.title == readA.title) { "BOARD_FAIL the copy should be identical in content" }
    check(readB.poster != readA.poster) {
        "BOARD_FAIL a copied listing came back as the same author"
    }

    // A listing's key is its own: stable across its refreshes, and unlinkable
    // to the poster's other listings. Both halves matter — the first is what
    // makes "seen before" mean anything, the second is why this is not the
    // persona.
    val refreshed = rentalEncode(notice("Sunny room, now with wifi"), persona, "listing-1", board, 3u)
    check(rentalDecode(refreshed, board, 3u).poster == readA.poster) {
        "BOARD_FAIL a refresh changed the author"
    }
    val other = rentalEncode(notice("Garage space"), persona, "listing-2", board, 3u)
    check(rentalDecode(other, board, 3u).poster != readA.poster) {
        "BOARD_FAIL two listings from one phone share a key"
    }

    // Tampering, byte by byte. Flipping anything breaks either the signature
    // or the work; there is no edit that survives.
    var survived = 0
    for (i in a.indices step 7) {
        val edited = a.copyOf().also { it[i] = (it[i].toInt() xor 0x01).toByte() }
        if (runCatching { rentalDecode(edited, board, 3u) }.isSuccess) survived++
    }
    check(survived == 0) { "BOARD_FAIL $survived edited notices verified" }

    // The downgrade that would make all of it decorative: an unsigned notice.
    // There is no version of this the reader accepts, so an attacker cannot
    // skip the work by simply not doing it.
    check(runCatching { rentalDecode(ByteArray(0), board, 3u) }.isFailure) {
        "BOARD_FAIL empty bytes were read as a notice"
    }
    check(runCatching { rentalDecode(a.copyOf(a.size - 4), board, 3u) }.isFailure) {
        "BOARD_FAIL a truncated notice was accepted"
    }

    // And the poster store, which is what a reader actually sees.
    val dir = kotlin.io.path.createTempDirectory("ducat-board").toFile()
    val ctx = DeskContext(dir)
    val t0 = 1_700_000_000_000L
    val day = 24L * 60 * 60 * 1000

    check(Posters.seen(ctx, readA.poster, t0) == t0) { "BOARD_FAIL first sighting" }
    // Seeing them again does not restart the clock — a listing refreshed every
    // six hours would otherwise be permanently new.
    check(Posters.seen(ctx, readA.poster, t0 + 5 * day) == t0) {
        "BOARD_FAIL a later sighting moved the first-seen date"
    }
    check(!Posters.settled(ctx, readA.poster, t0 + day)) { "BOARD_FAIL settled after one day" }
    check(Posters.settled(ctx, readA.poster, t0 + 4 * day)) { "BOARD_FAIL not settled after four" }
    // The impostor is a stranger, which is the signal.
    check(!Posters.settled(ctx, readB.poster, t0 + 4 * day)) {
        "BOARD_FAIL an unseen author read as established"
    }
    // An empty poster is never established — a bug that let one through would
    // put the badge on everything.
    check(!Posters.settled(ctx, "", t0 + 400 * day)) { "BOARD_FAIL blank poster settled" }

    // Forgetting: a listing's key dies with the listing, and keeping every one
    // forever is a record of everywhere this phone has browsed.
    check(Posters.sweep(ctx, t0 + 100 * day) == 1) { "BOARD_FAIL nothing was swept" }
    check(!Posters.settled(ctx, readA.poster, t0 + 100 * day)) {
        "BOARD_FAIL a swept poster is still known"
    }

    // What a poster actually pays. Not an assertion — a phone is slower than
    // this and the number is the point of the exercise, so it gets printed
    // rather than pinned.
    val t1 = System.nanoTime()
    repeat(5) { rentalEncode(notice("Timing $it"), persona, "listing-t$it", board, 1u) }
    val each = (System.nanoTime() - t1) / 5.0 / 1e9
    println("BOARD_POW %.2fs per notice here; a full 128-slot cell = %.0fs".format(each, each * 128))

    println("BOARD_OK slot=bound copy=newauthor refresh=stable tamper=0/${a.size / 7} sweep=ok")
}
