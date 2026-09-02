package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Sites

/**
 * One site's pages must never be served from another's directory.
 *
 * §16.22's room renders whatever is in the bundle directory for the
 * address the reader opened, and the only thing standing between two
 * publishers was `recordKey.hashCode()` — thirty-two bits of a hash
 * designed for hash tables, not for telling strangers apart. Two
 * addresses that collide share a directory, and `fetchBundle` returns a
 * cached one whenever the *site's own* fetched digest matches its head:
 * so the wrong bundle renders under the right address, sealed room and
 * all, with nothing on screen to say so.
 *
 * The collision below is the textbook Java one ("Aa"/"BB", and any
 * prefix of it), which is here to show the size of the thing being relied
 * on. A real attacker cannot type a record key — they would grind
 * keypairs for a 32-bit match — but that is a cost, not a wall, and
 * substitution is the attack this room exists to prevent.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:sitedirtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("SITEDIR ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val ctx = DeskContext(kotlin.io.path.createTempDirectory("ducat-sitedir").toFile())

    // Two addresses that the old scheme could not tell apart at all.
    val one = "VLD0:siteAa"
    val two = "VLD0:siteBB"
    check(
        "the old scheme really did collide on these",
        one.hashCode() == two.hashCode(),
        "${one.hashCode()} vs ${two.hashCode()}",
    )

    val dirOne = Sites.bundleDir(ctx, one)
    val dirTwo = Sites.bundleDir(ctx, two)
    check("colliding addresses now get their own directory", dirOne != dirTwo)
    check("and the name is a digest, not a number", dirOne.parentFile.name.length == 64)
    check("stable across calls", Sites.bundleDir(ctx, one) == dirOne)

    // The substitution the shared directory allowed: write one site's page
    // and read it back through the other's address.
    dirOne.mkdirs()
    File(dirOne, "index.html").writeText("<h1>the first publisher</h1>")
    check(
        "the other address does not see it",
        !File(dirTwo, "index.html").isFile,
    )

    // Orphans: bundles no saved site claims — what the old naming leaves
    // behind, and anything a removal missed.
    val root = File(ctx.filesDir, "sites")
    File(root, "2768502571/current").mkdirs()
    File(root, "2768502571/current/index.html").writeText("left over")
    val before = root.listFiles()?.size ?: 0
    check("the leftovers are there to sweep", before >= 2, "$before dirs")
    val swept = Sites.sweepOrphans(ctx)
    check("both unclaimed bundles go", swept == before, "swept $swept of $before")
    check("nothing is left", (root.listFiles()?.size ?: 0) == 0)
    check("a second sweep takes nothing", Sites.sweepOrphans(ctx) == 0)

    if (failures > 0) {
        println("SITEDIRTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("SITEDIRTEST OK")
}
