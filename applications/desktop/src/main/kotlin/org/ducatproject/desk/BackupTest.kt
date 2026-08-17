package org.ducatproject.desk

import org.ducatproject.ducat.ContactStore
import uniffi.ducat_mobile.Profile
import uniffi.ducat_mobile.RestoredBackup

/**
 * Round-trips app-state through backup, asserting the StringSet survives.
 *
 * claimed_kis_v1 is a StringSet; the backup exported it as something org.json
 * mangled and restore only handled Boolean/String, so it was silently dropped
 * — a restored device forgot which outputs were already matched to a bill.
 * This proves the fix: seed a set, export, restore into a fresh device, read
 * it back. `./gradlew :desktop:backuptest`.
 */
fun main() {
    val srcDir = kotlin.io.path.createTempDirectory("ducat-bk-src").toFile()
    val dstDir = kotlin.io.path.createTempDirectory("ducat-bk-dst").toFile()
    val src = DeskContext(srcDir)
    val dst = DeskContext(dstDir)

    val kis = setOf("ki-aaaa", "ki-bbbb", "ki-cccc")
    src.getSharedPreferences("ducat_contacts", 0).edit()
        .putStringSet("claimed_kis_v1", kis)
        .putBoolean("publish_address", true)
        .putString("receipts_v1", "[{\"r\":1}]")
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
    )
    ContactStore(dst).restoreFromBackup(restored)

    val got = dst.getSharedPreferences("ducat_contacts", 0)
        .getStringSet("claimed_kis_v1", emptySet()) ?: emptySet()
    val pub = dst.getSharedPreferences("ducat_contacts", 0)
        .getBoolean("publish_address", false)
    val rec = dst.getSharedPreferences("ducat_contacts", 0)
        .getString("receipts_v1", null)

    check(got == kis) { "BACKUPTEST_FAIL claimed_kis: expected $kis got $got" }
    check(pub) { "BACKUPTEST_FAIL publish_address dropped" }
    check(rec == "[{\"r\":1}]") { "BACKUPTEST_FAIL receipts_v1: got $rec" }
    println("BACKUPTEST_OK claimed_kis=$got publish=$pub receipts=$rec")
}
