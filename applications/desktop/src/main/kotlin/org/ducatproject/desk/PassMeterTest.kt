package org.ducatproject.desk

import uniffi.ducat_mobile.PassphraseStrength
import uniffi.ducat_mobile.passphraseStrength

/**
 * The strength meter, through the bridge the screen actually calls.
 * `./gradlew :desktop:passmeter`.
 *
 * The arithmetic is pinned in `core/src/backup.rs`. This exists because the
 * meter is not arithmetic to the person reading it — it is a word and a colour
 * next to the box holding the passphrase for their spend key, their persona and
 * every relationship they have, and it reaches them through a uniffi enum whose
 * four variants could be mapped in the wrong order without a single Rust test
 * noticing.
 *
 * The bug being pinned: the estimate scored a spaced phrase at eleven bits per
 * token and stopped there, so `1 2 3 4 5 6` — six digits, worth about twenty
 * bits and exhausted in an instant — came out at sixty-six and the screen said
 * Strong, in green. So did any word typed six times. Both are what somebody
 * does when a meter demands more words and they have none, which makes them the
 * two shapes it must not flatter.
 */
fun main() {
    // The grades this must not give. Nothing here is a passphrase; every one of
    // them was Strong.
    val notStrong = mapOf(
        "1 2 3 4 5 6" to "six digits, one apiece",
        "1 2 3 4 5 6 7 8" to "and pressing the space bar for more",
        "aaa aaa aaa aaa aaa aaa" to "one word, typed six times",
        "hunter2 hunter2 hunter2 hunter2" to "the classic, repeated",
        "horse Horse HORSE hOrSe horsE horse" to "shouting is not a seventh choice",
        "a b a b a b a b" to "two letters, alternating",
    )
    for ((p, why) in notStrong) {
        val s = passphraseStrength(p)
        check(s != PassphraseStrength.STRONG) { "PASSMETER_FAIL '$p' ($why) graded $s" }
    }

    // And the grades it must still give, or the meter has been fixed into
    // uselessness and people will learn to ignore it.
    val expected = mapOf(
        "correct horse battery staple thing pin" to PassphraseStrength.STRONG,
        "velvet anchor pumice drifting lantern rehearse" to PassphraseStrength.STRONG,
        "T7#kq2Lm!vZr9x@W" to PassphraseStrength.STRONG,
        "Tea4two!" to PassphraseStrength.FAIR,
        "hunter22" to PassphraseStrength.WEAK,
        "short" to PassphraseStrength.TOO_SHORT,
    )
    for ((p, want) in expected) {
        val got = passphraseStrength(p)
        check(got == want) { "PASSMETER_FAIL '$p' graded $got, expected $want" }
    }

    // All four variants are reachable across the bridge. An enum mapped one
    // position out would still pass everything above if the shift happened to
    // land inside a band nothing here tests.
    val seen = (notStrong.keys + expected.keys).map { passphraseStrength(it) }.toSet()
    check(seen == PassphraseStrength.entries.toSet()) {
        "PASSMETER_FAIL only reached $seen of ${PassphraseStrength.entries}"
    }

    println("PASSMETER_OK ${notStrong.size} flattering shapes refused, ${expected.size} grades held")
}
