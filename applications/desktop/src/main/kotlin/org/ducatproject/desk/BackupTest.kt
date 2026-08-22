package org.ducatproject.desk

import org.ducatproject.ducat.Ceremony
import org.ducatproject.ducat.ContactStore
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.Profile
import uniffi.ducat_mobile.RestoredBackup
import uniffi.ducat_mobile.exportBackup
import uniffi.ducat_mobile.importBackup

/**
 * What a backup has to carry, proven by carrying it. `./gradlew :desktop:backuptest`.
 *
 * Two rounds, because the two failures they cover are different shapes.
 *
 * **App state**, in memory only: `claimed_kis_v1` is a StringSet, the export
 * handed it to org.json which mangled it, and restore only understood Boolean
 * and String — so a restored device forgot which outputs were already matched
 * to a bill, silently.
 *
 * **Escrow shares**, through the real bridge: §4.3.3's shares live in their own
 * store, and the Android export passed `vec![]` for them while this app's
 * backup screen told people an open escrow needs a fresher bundle. On the
 * three-party rung a lost share costs the ability to take part; on the
 * two-party rung the escrow can never be released by anyone, so the deposit is
 * gone. That one goes through `exportBackup`/`importBackup` rather than a
 * hand-built record, because the bug was in the bridge and a hand-built
 * `RestoredBackup` would have passed the whole time it was broken.
 */
fun main() {
    appState()
    escrowShares()
}

private fun appState() {
    val srcDir = kotlin.io.path.createTempDirectory("ducat-bk-src").toFile()
    val dstDir = kotlin.io.path.createTempDirectory("ducat-bk-dst").toFile()
    val src = DeskContext(srcDir)
    val dst = DeskContext(dstDir)

    val kis = setOf("ki-aaaa", "ki-bbbb", "ki-cccc")
    // A card handed out and not yet claimed. Its writer secret is the only way
    // to answer the claim, and the poller only watches inboxes it can find in
    // here — so a restore without it leaves somebody holding a card that
    // claims into silence.
    val cards = """[{"inbox":"VLD0:abc","wsec":"c2Vjcg==","uri":"ducat:card","answered_by":null}]"""
    src.getSharedPreferences("ducat_contacts", 0).edit()
        .putStringSet("claimed_kis_v1", kis)
        .putBoolean("publish_address", true)
        .putString("receipts_v1", "[{\"r\":1}]")
        .putString("issued_cards", cards)
        // §16.11: which of a contact's one-time ids we already spent.
        // Lost, a restored device re-offers them and seals to keys the
        // other side has burned.
        .putString("usedtheirs_ab12", "3,7,11")
        .apply()

    val blob = ContactStore(src).backupAppState()

    val restored = RestoredBackup(
        spendKeyHex = "",
        restoreHeight = 0uL,
        personaSecret = ByteArray(0),
        displayName = null,
        publishPayto = false,
        profile = Profile(null, null, null, null, null, null, null, null),
        contacts = emptyList(),
        prekeySignedSecret = null,
        prekeyOneTime = emptyList(),
        prekeyNextId = 0uL,
        appState = blob,
        escrowCount = 0u,
        escrowShares = emptyList(),
    )
    ContactStore(dst).restoreFromBackup(restored)

    val got = dst.getSharedPreferences("ducat_contacts", 0)
        .getStringSet("claimed_kis_v1", emptySet()) ?: emptySet()
    val pub = dst.getSharedPreferences("ducat_contacts", 0)
        .getBoolean("publish_address", false)
    val rec = dst.getSharedPreferences("ducat_contacts", 0)
        .getString("receipts_v1", null)

    val crd = dst.getSharedPreferences("ducat_contacts", 0)
        .getString("issued_cards", null)

    check(got == kis) { "BACKUPTEST_FAIL claimed_kis: expected $kis got $got" }
    check(pub) { "BACKUPTEST_FAIL publish_address dropped" }
    check(rec == "[{\"r\":1}]") { "BACKUPTEST_FAIL receipts_v1: got $rec" }
    check(crd == cards) { "BACKUPTEST_FAIL issued_cards: got $crd" }
    val used = dst.getSharedPreferences("ducat_contacts", 0)
        .getString("usedtheirs_ab12", null)
    check(used == "3,7,11") { "BACKUPTEST_FAIL usedtheirs: got $used" }
    println("BACKUPTEST_OK claimed_kis=$got publish=$pub receipts=$rec cards=ok used=$used")
}

private fun escrowShares() {
    val srcDir = kotlin.io.path.createTempDirectory("ducat-esc-src").toFile()
    val dstDir = kotlin.io.path.createTempDirectory("ducat-esc-dst").toFile()
    val src = DeskContext(srcDir)
    val dst = DeskContext(dstDir)

    // Two open escrows and one already released. The released one must not
    // travel: spent key material has no recipient, and an entry in somebody's
    // escrow list that restores to nothing reads as recoverable and is not.
    val openA = """{"id":"aa11","stage":"done","keys":"deadbeef","scanFrom":2187000,"kind":1}"""
    val openB = """{"id":"bb22","stage":"release_pending","keys":"cafebabe","scanFrom":2187100}"""
    val closed = """{"id":"cc33","stage":"released","keys":"f00d","scanFrom":2186000}"""
    // No key package: the DKG never finished, so there is nothing to sign with.
    val halfBuilt = """{"id":"dd44","stage":"committed","scanFrom":2186500}"""
    src.getSharedPreferences("ducat_ceremonies", 0).edit()
        .putString("c_aa11", openA)
        .putString("c_bb22", openB)
        .putString("c_cc33", closed)
        .putString("c_dd44", halfBuilt)
        .apply()

    val shares = Ceremony.backupShares(src)
    check(shares.size == 2) {
        "BACKUPTEST_FAIL expected 2 open escrows, got ${shares.size} " +
            shares.map { it.escrowId.joinToString("") { b -> "%02x".format(b) } }
    }

    // Through the bridge, not around it.
    val spend = "1f".repeat(32)
    val persona = ByteArray(32) { 7 }
    val blob = exportBackup(
        BackupInput(
            spendKeyHex = spend,
            restoreHeight = 2187000uL,
            displayName = "esc",
            publishPayto = false,
            profile = Profile(null, null, null, null, null, null, null, null),
            contacts = emptyList(),
            prekeySignedSecret = null,
            prekeyOneTime = emptyList(),
            prekeyNextId = 1uL,
            appState = null,
            escrowShares = shares,
        ),
        "correcthorsebattery",
        persona,
    )
    val back = importBackup(blob, "correcthorsebattery")
    check(back.escrowCount.toInt() == 2) { "BACKUPTEST_FAIL escrowCount=${back.escrowCount}" }
    check(back.escrowShares.size == 2) { "BACKUPTEST_FAIL escrowShares=${back.escrowShares.size}" }

    Ceremony.restoreShares(dst, back.escrowShares)
    val p = dst.getSharedPreferences("ducat_ceremonies", 0)
    check(p.getString("c_aa11", null) != null) { "BACKUPTEST_FAIL aa11 did not restore" }
    check(p.getString("c_bb22", null) != null) { "BACKUPTEST_FAIL bb22 did not restore" }
    check(p.getString("c_cc33", null) == null) { "BACKUPTEST_FAIL a released escrow travelled" }
    check(p.getString("c_dd44", null) == null) { "BACKUPTEST_FAIL a keyless ceremony travelled" }

    val a = org.json.JSONObject(p.getString("c_aa11", "{}")!!)
    check(a.optString("keys") == "deadbeef") { "BACKUPTEST_FAIL key package lost: $a" }
    check(a.optLong("scanFrom") == 2187000L) { "BACKUPTEST_FAIL scanFrom lost: $a" }

    // A restore must never roll a live escrow backwards: the record on disk has
    // been carried forward by this device's own participation and the bundle's
    // is a photograph. Stage rewound here is a ceremony that stalls, or a
    // funding mark that lets a second payment go.
    dst.getSharedPreferences("ducat_ceremonies", 0).edit()
        .putString("c_bb22", """{"id":"bb22","stage":"released","keys":"cafebabe"}""")
        .apply()
    Ceremony.restoreShares(dst, back.escrowShares)
    val live = org.json.JSONObject(p.getString("c_bb22", "{}")!!)
    check(live.optString("stage") == "released") {
        "BACKUPTEST_FAIL a restore rewound a live escrow to ${live.optString("stage")}"
    }

    println("BACKUPTEST_OK escrows: carried=${back.escrowShares.size} released_skipped keyless_skipped live_preserved")
}
