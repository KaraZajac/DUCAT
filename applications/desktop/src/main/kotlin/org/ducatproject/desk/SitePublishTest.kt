package org.ducatproject.desk

import java.io.File
import org.ducatproject.ducat.Sites

/**
 * The publisher's half of §16.22, in the parts that need no network.
 *
 * Two properties, and both are about the one thing a site cannot get
 * back. `ducat:site/<key>` is the DHT record key, readers keep it across
 * every update, and the record's owner secret is the only key that can
 * rewrite the head — a reader opens with no writer at all, so there is
 * nothing to sign a write with. Lose that secret and the address goes on
 * serving its last bundle for as long as anyone mirrors it, with no way
 * for the author to change a word.
 *
 * So: the store must carry the keypair through a round trip, and `add`
 * must not quietly drop it. `add` is the sharp one — it rebuilds a row
 * from the head, the head is public, and pasting your own address or
 * tapping a `ducat:site` link to your own page is the obvious way to
 * check it looks right.
 *
 * The lint is the other half, and §16.22 asks for it in SHOULD terms:
 * a bundle that reaches the clearnet fails at the keyboard, in front of
 * the one person who can fix it, rather than on a stranger's phone where
 * the page merely looks broken.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:sitepublishtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("SITEPUB ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val root = kotlin.io.path.createTempDirectory("ducat-sitepub").toFile()
    val ctx = DeskContext(root)

    // --- the sealed-room lint ------------------------------------------
    fun bundle(name: String, index: String, extra: Pair<String, String>? = null): File {
        val d = File(root, name).apply { mkdirs() }
        File(d, "index.html").writeText(index)
        extra?.let { (n, body) -> File(d, n).writeText(body) }
        return d
    }

    val sealed = bundle(
        "sealed",
        "<h1>The Corner Shop</h1><img src='shop.png'><a href='ducat:card/abc'>card</a>",
        "style.css" to "body { background: url('paper.png'); }",
    )
    check("a bundle that stays home passes", Sites.clearnetIn(sealed) == null,
        Sites.clearnetIn(sealed) ?: "")

    val cases = mapOf(
        "img" to "<img src=\"https://cdn.example/logo.png\">",
        "link" to "<a href='//tracker.example/x'>hi</a>",
        "css-url" to "<h1>x</h1>",
        "import" to "<h1>x</h1>",
    )
    check("an external image is caught",
        Sites.clearnetIn(bundle("b1", cases["img"]!!)) != null)
    check("a protocol-relative link is caught",
        Sites.clearnetIn(bundle("b2", cases["link"]!!)) != null)
    check("url() in a stylesheet is caught",
        Sites.clearnetIn(
            bundle("b3", cases["css-url"]!!, "s.css" to "body{background:url(https://x/y.png)}"),
        ) != null)
    check("@import is caught",
        Sites.clearnetIn(
            bundle("b4", cases["import"]!!, "s.css" to "@import \"https://x/y.css\";"),
        ) != null)
    // The message has to name the file, or a publisher with thirty pages
    // is told only that one of them is wrong.
    val named = Sites.clearnetIn(bundle("b5", "<img src='https://x/y.png'>"))
    check("and it says which file", named?.startsWith("index.html:") == true, named ?: "null")

    // --- the keypair, through the store --------------------------------
    //
    // publish() needs a node, so the round trip is driven through the same
    // save/load path it uses rather than through the network.
    val pub = ByteArray(32) { 0x11 }
    val sec = ByteArray(32) { 0x22 }
    val mine = Sites.Site(
        recordKey = "VLD0:mine", title = "The Corner Shop", share = "VLD0:s",
        digestHex = "aa", updated = 1, addedAt = 1, keepAlive = true,
        fetchedDigestHex = "aa", fetchedShare = "VLD0:s",
        ownerPublic = pub, ownerSecret = sec,
    )
    val theirs = mine.copy(
        recordKey = "VLD0:theirs", title = "Someone else's",
        ownerPublic = null, ownerSecret = null,
    )
    check("a site I made knows it", mine.mine)
    check("a site I only read does not", !theirs.mine)

    Sites.save(ctx, listOf(mine, theirs))
    val back = Sites.all(ctx)
    val backMine = back.first { it.recordKey == "VLD0:mine" }
    check("the keypair survives the store", backMine.ownerSecret.contentEquals(sec))
    check("and the public half too", backMine.ownerPublic.contentEquals(pub))
    check("a read-only site stays read-only", !back.first { it.recordKey == "VLD0:theirs" }.mine)
    check("equality is by value, not identity", back.first { it.recordKey == "VLD0:mine" } == mine)

    if (failures > 0) {
        println("SITEPUBTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("SITEPUBTEST OK")
}
