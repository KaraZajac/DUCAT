package org.ducatproject.desk

import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactNaming
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.withoutDisplayHazards

/**
 * Two contacts a person cannot tell apart. `./gradlew :desktop:confusable`.
 *
 * Half of every name in the contact list was chosen by the contact, so calling
 * yourself what somebody's regular bar calls itself costs nothing and needs no
 * access to anything. The defence is only worth having if it survives the
 * obvious dodge — the attacker does not have to reuse the same *string*, only
 * the same *picture* — so most of what follows is spelling `Sam` in ways that
 * are not `Sam`.
 */
fun main() {
    val skel = ContactNaming::skeleton

    // The tables are hand-paired, and zip() silently truncates to the shorter
    // side, so a miscount would quietly drop the tail of the alphabet rather
    // than fail. Check the fold end to end instead of trusting the count.
    val disguises = mapOf(
        "Sam" to "the plain one",
        "Ѕam" to "Cyrillic Ѕ (U+0405)",
        "Sаm" to "Cyrillic а (U+0430)",
        "SAM" to "shouting",
        "​Sam" to "leading zero-width space",
        "Sa‍m" to "zero-width joiner inside",
        "‮Sam" to "right-to-left override",
        "Sam " to "trailing space",
        "Ｓａｍ" to "fullwidth",
        "⁦Sam⁩" to "wrapped in bidi isolates",
    )
    for ((s, why) in disguises) {
        check(skel(s) == "sam") {
            "CONFUSABLE_FAIL $why: ${skel(s)} did not fold onto sam"
        }
    }

    // Runs of whitespace collapse, so padding a name out cannot make a new
    // one. Not folded onto "sam" — that would be a different word.
    check(skel("Sam   Smith") == "sam smith") { "CONFUSABLE_FAIL whitespace did not collapse" }
    check(skel("\u00a0Sam\u2009Smith\u00a0") == "sam smith") {
        "CONFUSABLE_FAIL exotic spaces were not treated as spaces"
    }

    // Greek, which the second table covers.
    check(skel("ΡΑΥΡΑL") == skel("PAYPAL")) { "CONFUSABLE_FAIL Greek capitals did not fold" }
    check(skel("Καty") == skel("Katy")) { "CONFUSABLE_FAIL mixed Greek did not fold" }

    // And the other half: names that really are different must stay different,
    // or the warning fires on every honest contact list and is ignored.
    val distinct = listOf("Sam" to "Sara", "Sam" to "Samir", "Sam" to "Sán", "Jo" to "Jon")
    for ((a, b) in distinct) {
        check(skel(a) != skel(b)) { "CONFUSABLE_FAIL '$a' and '$b' were folded together" }
    }

    // Now the store. Two contacts, one picture.
    val dir = kotlin.io.path.createTempDirectory("ducat-confuse").toFile()
    val ctx = DeskContext(dir)
    val store = ContactStore(ctx)
    fun put(hex: String, asserted: String?, pet: String? = null) = store.add(
        Contact(
            personaHex = hex, petname = pet, assertedName = asserted,
            myOutbox = "o-$hex", theirOutbox = "t-$hex",
        ),
    )

    put("aa".repeat(32), "Sam")           // the bar
    put("bb".repeat(32), "Ѕam")           // the impostor, Cyrillic Ѕ
    put("cc".repeat(32), "Jordan")        // uninvolved
    put("dd".repeat(32), null)            // never named — not a claim to anything

    val amb = store.ambiguous()
    check("aa".repeat(32) in amb && "bb".repeat(32) in amb) {
        "CONFUSABLE_FAIL the lookalike pair was not flagged: $amb"
    }
    check("cc".repeat(32) !in amb) { "CONFUSABLE_FAIL a uniquely named contact was flagged" }
    check(amb.size == 2) { "CONFUSABLE_FAIL expected exactly the pair, got ${amb.size}" }

    // Two unnamed contacts both read "Unnamed contact", which is true of both
    // and asserted by neither. Flagging them would put a warning on the one
    // case where nobody is claiming to be anybody.
    put("ee".repeat(32), null)
    check(store.ambiguous().size == 2) {
        "CONFUSABLE_FAIL unnamed contacts were treated as a name collision"
    }

    // The outbound stripper and the wire's inbound rule have to agree on
    // exactly which characters they are about. If the stripper misses one, the
    // message leaves here and vanishes at the far end after the slot is spent;
    // if it takes one the wire allows, we quietly mangle honest writing.
    //
    // They agree by construction — withoutDisplayHazards crosses the bridge to
    // the same table wire.rs refuses with — so this is checking the bridge
    // reaches it, character by character, from the language that calls it.
    val hazards = ((0x202A..0x202E) + (0x2066..0x2069) + (0x206A..0x206F) +
        (0x0000..0x0008) + (0x000B..0x000C) + (0x000E..0x001F) + listOf(0x007F) +
        (0x0080..0x009F)).map { it.toChar() }
    for (h in hazards) {
        check(withoutDisplayHazards("a${h}b") == "ab") {
            "CONFUSABLE_FAIL U+%04X survived the outbound strip".format(h.code)
        }
    }
    // And the ones the wire keeps must not be stripped, or Arabic and Hebrew
    // lose their typography on the way out.
    for (keep in listOf('\u200E', '\u200F', '\u061C', '\n', '\t', ' ')) {
        check(withoutDisplayHazards("a${keep}b") == "a${keep}b") {
            "CONFUSABLE_FAIL U+%04X was stripped but the wire allows it".format(keep.code)
        }
    }
    // Ordinary writing crosses the bridge unchanged. Multi-byte characters
    // included: the strip runs over chars, and a rule that counted bytes would
    // cut a codepoint in half here rather than anywhere a Latin test looks.
    for (plain in listOf(
        "Sunny room near the park",
        "غرفة مشمسة قرب الحديقة",
        "חדר שמשי ליד הפארק",
        "公園近くの日当たりの良い部屋",
        "Chambre — 25 m², 2 pers.",
        "🚗 Fiat Panda",
    )) {
        check(withoutDisplayHazards(plain) == plain) {
            "CONFUSABLE_FAIL honest text came back changed: '$plain' -> " +
                "'${withoutDisplayHazards(plain)}'"
        }
    }

    println(
        "CONFUSABLE_OK folds=${disguises.size} distinct=${distinct.size} " +
            "store=pair-flagged hazards=${hazards.size}",
    )
}
