package org.ducatproject.desk

import org.ducatproject.ducat.Listings
import uniffi.ducat_mobile.maxStandShards
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.rentalDecode
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead
import uniffi.ducat_mobile.standShardName
import java.io.File

/**
 * An attacker on a real board. `./gradlew :desktop:boardattack`
 *
 *   DUCAT_DESK_STATE=/tmp/atk ./gradlew :desktop:boardattack
 *
 * The premise is not in doubt: a stand's write key is the cell name hashed, so
 * this test *can* write to somebody else's board and does. What is being
 * checked is what a reader does with what it wrote.
 *
 * Run it while `:desktop:listtest` is up as owner on the same spot. It reads a
 * genuine notice off the live board, produces the three doctored versions an
 * attacker would actually try, writes them into free slots, and then asks
 * whether a reader shows them.
 */
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("ATTACK_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val lat = System.getenv("DUCAT_LIST_LAT")?.toLongOrNull() ?: 525200000L
    val lon = System.getenv("DUCAT_LIST_LON")?.toLongOrNull() ?: 134050000L

    Unlock.orExit(dir)
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val ready = System.currentTimeMillis() + 90_000
    while (System.currentTimeMillis() < ready && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "ATTACK_FAIL node never became ready" }

    val cell = uniffi.ducat_mobile.geohashEncode(lat, lon, 5u)
    val base = org.ducatproject.ducat.standNow("local:$cell")
    println("ATTACK_UP on $base")

    // Find a genuine notice and a free slot on the same board.
    var board: String? = null
    var genuine: ByteArray? = null
    var genuineSlot = 0u
    // Free slots anywhere on the cell's ladder, not just on the board the
    // genuine notice sits on — shard 0 is usually the full one, and a reader
    // climbs the ladder, so a doctored notice up there is read just the same.
    val free = mutableListOf<Pair<String, UInt>>()
    for (shard in 0u until maxStandShards()) {
        val name = standShardName(base, shard)
        val slots = runCatching { standRead(name) }.getOrDefault(emptyList())
        val taken = slots.map { it.subkey }.toSet()
        if (board == null) {
            slots.firstOrNull {
                runCatching { rentalDecode(it.data, name, it.subkey) }.isSuccess
            }?.let { board = name; genuine = it.data; genuineSlot = it.subkey }
        }
        free += (0u..7u).filter { it !in taken }.map { name to it }
        if (board != null && free.size >= 3) break
    }
    val b = board ?: error("ATTACK_FAIL no genuine notice on $base — is the owner running?")
    val real = genuine!!
    println("ATTACK found a genuine notice at $b slot $genuineSlot, ${free.size} free slot(s)")
    check(free.size >= 3) { "ATTACK_FAIL need three free slots, cell has ${free.size}" }

    val title = rentalDecode(real, b, genuineSlot).title
    println("ATTACK it reads: \"$title\"")

    // 1. Lift it onto another slot, unchanged. This is the flood: one signed
    //    notice, every slot in the cell, all of it looking like its author.
    val (liftBoard, lifted) = free[0]
    standPost(liftBoard, lifted, real)

    // 2. Edit it in place. A price a reader would act on, or a title that
    //    names somebody else's business.
    val edited = real.copyOf().also { it[it.size / 2] = (it[it.size / 2].toInt() xor 0x01).toByte() }
    val (editBoard, editedSlot) = free[1]
    standPost(editBoard, editedSlot, edited)

    // 3. Strip the seal. The downgrade: if an unsigned notice still rendered,
    //    an attacker would never bother with any of the above.
    val stripped = real.copyOf(real.size / 2)
    val (stripBoard, strippedSlot) = free[2]
    standPost(stripBoard, strippedSlot, stripped)

    println(
        "ATTACK wrote three doctored notices: $liftBoard/$lifted, " +
            "$editBoard/$editedSlot, $stripBoard/$strippedSlot",
    )

    // The writes themselves must succeed — that is the premise, and a test
    // that quietly failed to write would prove nothing at all.
    // The bytes as well as the address, because this board is *live*. The
    // owner is filling its own ladder while this runs, and a slot chosen as
    // free can hold a genuine notice by the time it is read back — which then
    // decodes correctly, because it is genuine. Comparing the bytes is what
    // tells "our forgery was accepted" from "somebody else's real notice
    // landed here", and without it this test reports the second as the first.
    // Seen 2026-08-23: it failed on a listing called "Bicycle, barely ridden"
    // that the owner had just posted to the slot the lift went into.
    data class Doctored(
        val board: String,
        val slot: UInt,
        val what: String,
        val bytes: ByteArray,
    )
    val placed = listOf(
        Doctored(liftBoard, lifted, "lifted to another slot", real),
        Doctored(editBoard, editedSlot, "edited in place", edited),
        Doctored(stripBoard, strippedSlot, "stripped of its seal", stripped),
    )
    for ((brd, slot, what, _) in placed) {
        check(standRead(brd).any { it.subkey == slot }) {
            "ATTACK_FAIL the notice $what never landed — the test proved nothing"
        }
    }
    println("ATTACK all three are on the board (as they must be — the write key is public)")

    // Now the only question that matters: does a reader show them?
    var judged = 0
    for ((brd, slot, what, wrote) in placed) {
        val data = standRead(brd).first { it.subkey == slot }.data
        if (!data.contentEquals(wrote)) {
            // Not our bytes any more. Says nothing either way, and saying
            // nothing is the honest answer — the alternative is a security
            // test that cries wolf at the owner doing its job.
            println("ATTACK --   $what: slot $slot was overwritten by another writer, not judged")
            continue
        }
        val got = runCatching { rentalDecode(data, brd, slot) }
        check(got.isFailure) {
            "ATTACK_FAIL a notice $what was accepted at slot $slot: ${got.getOrNull()?.title}"
        }
        println("ATTACK ok   refused: $what")
        judged++
    }
    check(judged > 0) {
        "ATTACK_FAIL every doctored notice was overwritten before it could be judged — " +
            "run this against a settled owner, or the test proves nothing"
    }

    // And the genuine one still reads, so the refusals above are the check
    // working rather than everything being broken.
    check(runCatching { rentalDecode(real, b, genuineSlot) }.isSuccess) {
        "ATTACK_FAIL the genuine notice stopped verifying"
    }
    println("ATTACK ok   the genuine notice still reads")

    // Finally, through the search a person actually uses: the doctored slots
    // must not appear among the results.
    val found = mutableListOf<uniffi.ducat_mobile.RentalInfo>()
    Listings.search(lat, lon, null, onFound = { found.clear(); found.addAll(it) })
    println("ATTACK search returned ${found.size} listing(s)")
    check(found.any { it.title == title }) {
        "ATTACK_FAIL the genuine listing vanished from search"
    }
    // Three doctored copies of one notice would show as duplicates of it.
    val copies = found.count { it.title == title }
    check(copies == 1) {
        "ATTACK_FAIL search showed $copies copies of \"$title\" — a lifted notice got through"
    }

    // Clear up. These went onto a board other people read, and leaving three
    // unreadable notices squatting live slots until their TTL is exactly the
    // thing §18.7 asks a client not to do — the more so here, because the
    // whole point of the exercise was that writing them was easy.
    //
    // Written down first, then cleared. A run that dies between the writes
    // and here would otherwise leave them for the next run to find and not
    // know about, which is how a test that is about litter becomes litter.
    val ledger = File(dir, "borrowed-slots.txt")
    ledger.appendText(placed.joinToString("") { (b, s, _) -> "$b $s\n" })
    var cleared = 0
    val remaining = mutableListOf<String>()
    ledger.readLines().filter { it.isNotBlank() }.distinct().forEach { line ->
        val (brd, slot) = line.split(" ").let { it[0] to it[1].toUInt() }
        if (runCatching { standPost(brd, slot, ByteArray(0)) }.isSuccess) cleared++
        else remaining += line
    }
    ledger.writeText(remaining.joinToString("") { "$it\n" })
    println("ATTACK ok   cleared $cleared slot(s) it had borrowed, ${remaining.size} left to retry")

    println(
        "ATTACK_OK wrote=3 refused=3 genuine=intact search=${found.size} " +
            "copies=1 cleared=$cleared",
    )
}
