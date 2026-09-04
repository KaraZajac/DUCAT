package org.ducatproject.desk

import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.ui.barCells
import org.ducatproject.ducat.ui.cellFills

/**
 * What the swarm bar promises about a transfer.
 *
 * The bar exists because a byte count could not tell a stalled fetch from a
 * fresh one, and it is only worth more if its arithmetic is exact: pieces
 * arrive scattered, so a reader reads the *pattern*, and a cell that fills
 * early or a piece counted at two seams would quietly make that pattern a
 * fiction.
 *
 * `./gradlew :desktop:piecebartest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("BAR ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    fun progress(total: Int, have: Set<Int>): Swarm.Progress {
        val bytes = ByteArray((total + 7) / 8)
        for (i in have) bytes[i / 8] = (bytes[i / 8].toInt() or (1 shl (i % 8))).toByte()
        return Swarm.Progress(0, 0, false, have.size.toLong(), total.toLong(), bytes)
    }

    // The bit reader itself, since everything else rests on it.
    val scattered = progress(16, setOf(0, 5, 8, 15))
    check("reads the bits it was given",
        (0 until 16).filter { scattered.has(it) } == listOf(0, 5, 8, 15))
    check("no bit past the end", !scattered.has(16) && !scattered.has(9999))

    // Nothing, and everything.
    check("empty is empty", cellFills(progress(100, emptySet()), 100).all { it == 0f })
    check("full is full", cellFills(progress(100, (0 until 100).toSet()), 100).all { it == 1f })

    // The property that matters: no cell is full unless its whole run is.
    // 100 pieces over 40 cells means runs of 2 and 3.
    val one = cellFills(progress(100, setOf(0)), 100)
    check("one piece fills no cell", one.none { it >= 1f }, "max ${one.max()}")
    check("one piece does move a cell", one.any { it > 0f })

    // Seams: every piece counted exactly once. Fill pieces one at a time and
    // the total fill (weighted by run length) must climb by exactly one piece.
    var bad = 0
    for (total in listOf(7, 40, 41, 99, 100, 256, 1000)) {
        val cells = barCells(total)
        val have = mutableSetOf<Int>()
        for (i in 0 until total) {
            have.add(i)
            val fills = cellFills(progress(total, have), total)
            // Reconstruct the piece count from the cells' runs.
            var counted = 0.0
            for (c in 0 until cells) {
                val from = (c.toLong() * total / cells).toInt()
                val to = ((c + 1).toLong() * total / cells).toInt().coerceAtLeast(from + 1)
                val run = (minOf(to, total) - from).coerceAtLeast(1)
                counted += fills[c].toDouble() * run
            }
            if (Math.abs(counted - have.size) > 0.001) bad++
        }
        check("$total pieces: every piece lands in exactly one cell", bad == 0, "off on $bad")
        if (bad > 0) break
    }

    // Cell count never exceeds the pieces — a 3-piece share must not draw 40
    // cells, which would read as a large transfer barely begun.
    for (total in listOf(1, 2, 3, 39, 40, 41, 5000)) {
        check("$total pieces -> ${barCells(total)} cells",
            barCells(total) <= total && barCells(total) <= 40 && barCells(total) >= 1)
    }

    // A share with no index yet asks for nothing.
    check("no pieces, no cells", cellFills(progress(0, emptySet()), 0).isEmpty())

    if (failures > 0) {
        println("PIECEBARTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("PIECEBARTEST OK")
}
