package org.ducatproject.desk

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import java.io.File
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.ui.PieceBar

/**
 * Pictures of the swarm bar, for a human to look at.
 *
 * A one-piece test page draws one cell, which says nothing about the idea;
 * these are the sizes and shapes a real publication actually arrives in.
 * Rendered off-screen so no phone, network or seeder is needed.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:piecebarshots`.
 */
@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    val dir = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("SHOTS_FAIL set DUCAT_DESK_STATE"),
    ).apply { mkdirs() }
    val out = File(dir, "piecebar").apply { mkdirs() }
    val context = DeskContext(dir)
    android.res.DeskRes.setLocale("en")

    fun progress(total: Int, have: Set<Int>) = Swarm.Progress(
        position = 0, length = 0, done = false,
        piecesDone = have.size.toLong(), piecesTotal = total.toLong(),
        pieces = ByteArray((total + 7) / 8).also { b ->
            for (i in have) b[i / 8] = (b[i / 8].toInt() or (1 shl (i % 8))).toByte()
        },
    )

    // A swarm does not hand out pieces in order: the lease manager pops them
    // from a HashSet filtered by what each peer holds. These are drawn the
    // way that actually looks — clumps where a peer was generous, gaps where
    // nobody has answered yet.
    val rng = java.util.Random(20260903)
    fun scattered(total: Int, fraction: Double): Set<Int> {
        val want = (total * fraction).toInt()
        val have = mutableSetOf<Int>()
        while (have.size < want) {
            // Runs, not confetti: a peer that answers tends to hand over a
            // neighbourhood before it goes quiet.
            val start = rng.nextInt(total)
            val run = 1 + rng.nextInt(6)
            for (i in start until minOf(start + run, total)) {
                if (have.size < want) have.add(i)
            }
        }
        return have
    }

    val cases = listOf(
        Triple("01-just-started", 240, 0.02),
        Triple("02-a-few-peers", 240, 0.18),
        Triple("03-half-way", 240, 0.5),
        Triple("04-nearly-there", 240, 0.93),
        Triple("05-done", 240, 1.0),
        Triple("06-small-issue-12-pieces", 12, 0.42),
        Triple("07-big-site-5000-pieces", 5000, 0.31),
    )

    for ((name, total, frac) in cases) {
        val p = progress(total, if (frac >= 1.0) (0 until total).toSet() else scattered(total, frac))
        val scene = ImageComposeScene(width = 900, height = 150, density = Density(2f))
        scene.setContent {
            CompositionLocalProvider(LocalContext provides context) {
                MaterialTheme(colorScheme = lightColorScheme()) {
                    Surface(Modifier.fillMaxSize()) {
                        Column(
                            Modifier.fillMaxWidth().padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text(
                                "$total pieces · ${(frac * 100).toInt()}% here",
                                style = MaterialTheme.typography.labelMedium,
                            )
                            PieceBar(p)
                        }
                    }
                }
            }
        }
        val img = scene.render()
        val bytes = img.encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)?.bytes
        if (bytes == null) {
            println("SHOTS_FAIL could not encode $name"); kotlin.system.exitProcess(1)
        }
        File(out, "$name.png").writeBytes(bytes)
        println("  wrote $name.png")
    }
    println("PIECEBARSHOTS OK — $out")
}
