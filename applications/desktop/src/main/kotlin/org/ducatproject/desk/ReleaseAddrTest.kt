package org.ducatproject.desk

import org.ducatproject.ducat.Releases

/**
 * The release address, which is the whole of a release's identity.
 *
 * `ducat:file/<share-key>:<digest>` and a share key carries colons of its
 * own — `VLD0:<key>:<secret>` — so the obvious split, on the first colon,
 * hands back a truncated key that still looks like a key and resolves to
 * nothing. The digest is what makes the object immutable and what lets a
 * reader check what they were handed, so a parser that quietly accepts a
 * malformed one is worse than one that refuses.
 *
 * `./gradlew :desktop:releaseaddrtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("ADDR ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val key = "VLD0:JRLsL7DGWZF56faYNxCMnHifsKxpG_YQQwPWhtnBoKw:XHRpyzz_a0YglByPJW07WIE"
    val digest = "734414d4729b2eeab097f8cda30299107d74de58c8107688482e798c3669e61f"

    val uri = Releases.uriOf(key, digest)
    check("round trips", Releases.parse(uri) == (key to digest), uri)
    check("the key keeps its own colons", Releases.parse(uri)?.first == key)
    check("prefixed", uri.startsWith("ducat:file/"))

    // Whitespace from a paste.
    check("tolerates a pasted newline", Releases.parse("  $uri\n") == (key to digest))

    // Case: a digest is hex and compares lowercase, so the same file pasted
    // in caps must not become a second release.
    check("digest case-folds", Releases.parse(Releases.uriOf(key, digest.uppercase()))?.second == digest)

    // Refusals. Each of these has a plausible shape and none of them names
    // a fetchable thing.
    val bad = listOf(
        "" to "empty",
        "ducat:site/VLD0:abc" to "a site address",
        "ducat:file/" to "prefix only",
        "ducat:file/$key" to "no digest",
        "ducat:file/:$digest" to "no key",
        "ducat:file/$key:" to "trailing colon",
        "ducat:file/$key:${digest.dropLast(1)}" to "digest one short",
        "ducat:file/$key:${digest}0" to "digest one long",
        "ducat:file/$key:${digest.dropLast(1)}z" to "digest not hex",
        "https://example.com/$key:$digest" to "not a ducat uri",
    )
    for ((s, why) in bad) check("refuses $why", Releases.parse(s) == null, s.take(40))

    if (failures > 0) {
        println("RELEASEADDRTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("RELEASEADDRTEST OK")
}
