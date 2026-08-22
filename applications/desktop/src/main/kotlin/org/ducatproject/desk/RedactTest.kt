package org.ducatproject.desk

import org.ducatproject.ducat.DucatLog

/**
 * What the log is allowed to say. `./gradlew :desktop:redact`.
 *
 * This file exists to be sent to somebody. The Logs screen has a share button,
 * and the whole reason the on-disk tail is capped is so it stays "small enough
 * to share" — so every line in it should be read as though a stranger will.
 *
 * Redaction runs at add(), which is the only door in, and it has to hold in
 * both directions: a secret that gets through is sent to whoever the user
 * sends the log to, and an over-eager rule turns the diagnostics into
 * ellipses and the log stops being worth sharing at all.
 */
fun main() {
    val dir = kotlin.io.path.createTempDirectory("ducat-redact").toFile()
    val ctx = DeskContext(dir)
    DucatLog.clear()
    DucatLog.init(ctx)

    fun logged(msg: String): String {
        DucatLog.i("Redact", msg)
        return DucatLog.snapshot().last().message
    }

    // A stagenet subaddress, of exactly the shape Ceremony logs when a bond's
    // escrow is minted. Hand somebody this and they can watch the escrow.
    val addr = "7" + "A1b2C3d4E5f6G7h8J9kLmNpQrStUvWxYz".repeat(3).take(94)
    val out = logged("bond abcd done — escrow $addr")
    check(!out.contains(addr)) { "REDACT_FAIL a Monero address went into the log: $out" }
    check(out.contains("[95 b58]")) { "REDACT_FAIL address not marked as one: $out" }
    check(out.startsWith("bond abcd done — escrow ")) {
        "REDACT_FAIL the line around it was eaten: $out"
    }

    // Keys and key images: 64 hex, the existing rule, which must still fire —
    // and must fire *before* the address rule, since hex is a subset of base58.
    val key = "9f".repeat(32)
    check(!logged("spend $key").contains(key)) { "REDACT_FAIL a 64-hex key survived" }
    check(logged("spend $key").contains("[64 hex]")) { "REDACT_FAIL hex mislabelled" }

    // A card carries its own writer secret.
    check(!logged("scanned ducat:card/AAAA.BBBB.CCCC").contains("AAAA")) {
        "REDACT_FAIL a card link survived"
    }

    // And the other half. Ordinary diagnostics must come through intact, or
    // nobody can read what they are sent.
    for (plain in listOf(
        "transport AttachedFull — 151 peer(s)",
        "offline — messages wait for the network",
        "ride abcd: escrow holds 0.500000 XMR",
        "3 notes, so about 2 more payments",
        "aa12bb34… confirmed by http://node.monerodevs.org:38089",
        "attachments: reclaimed 12 KiB",
    )) {
        check(logged(plain) == plain) { "REDACT_FAIL mangled an ordinary line: ${logged(plain)}" }
    }

    // The near miss that matters: a short hash prefix is how half the app
    // already writes ids, and swallowing those would blind every log line.
    check(logged("aa12bb34cc56… deferring").contains("aa12bb34cc56")) {
        "REDACT_FAIL a short id prefix was redacted"
    }

    // The rule is shape, not prefix, and that is deliberate: any unbroken run
    // of ninety base58 characters goes, whatever it turns out to be. Nothing
    // a person writes looks like that, and the cost of being wrong runs one
    // way — a redacted diagnostic is inconvenient, a leaked address is
    // permanent. (This caught the logcap test's own filler, which is how the
    // property got written down.)
    val run = "x".repeat(120)
    check(!logged("odd $run").contains(run)) {
        "REDACT_FAIL a long unbroken base58 run was not redacted"
    }
    // But a run under the threshold, and anything with a space in it, stays.
    val short = "x".repeat(60)
    check(logged("odd $short").contains(short)) {
        "REDACT_FAIL a short run was redacted"
    }

    println("REDACT_OK address=hidden key=hidden card=hidden ordinary=intact shape=byrun")
}
