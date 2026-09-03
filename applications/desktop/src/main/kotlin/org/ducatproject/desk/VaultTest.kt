package org.ducatproject.desk

import org.ducatproject.ducat.DeskVault
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.securePrefs
import java.io.File

/**
 * The desk's encryption at rest, checked the only way worth checking it:
 * by looking at the bytes on disk for the secret that is supposed to be gone.
 *
 * `./gradlew :desktop:vaulttest` — uses a throwaway directory, never a real
 * desk, because half of what it does is delete things.
 */
fun main() {
    val base = File(
        System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }
            ?: error("VAULT_FAIL set DUCAT_DESK_STATE (a throwaway directory)"),
    )
    base.deleteRecursively()
    base.mkdirs()

    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("VAULT ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    /** Every byte the desk has written, as one searchable blob. */
    fun onDisk(): String = base.walkTopDown()
        .filter { it.isFile }
        .joinToString("\n") { f ->
            f.name + ":" + runCatching { f.readBytes().decodeToString() }.getOrDefault("")
        }

    // 1. A desk with no vault behaves exactly as it did before: plaintext,
    //    and the point of the exercise is that we can see the secret.
    val context = DeskContext(base)
    val spend = "a".repeat(64)
    WalletStore(context).save("5Test", spend, 2_190_000uL, true)
    val persona = PersonaStore(context).secret()
    // A store that is deliberately plain, written before the vault exists —
    // exactly how ducat_locale, ducat_units and the rest are used.
    context.getSharedPreferences("ducat_locale", 0).edit().putString("lang", "fr").apply()
    check("plaintext desk still works", WalletStore(context).spendKeyHex() == spend)
    check("and its secret is visibly on disk", onDisk().contains(spend),
        "which is what the vault is for")

    // 2. Setting a passphrase encrypts what is already there.
    val made = DeskVault.create(base, "correct horse battery")
    check("vault created", made.isSuccess, made.exceptionOrNull()?.message ?: "")
    // The *sealed* stores' plaintext is gone. Not "no .json at all": the
    // deliberately-plain stores live in the same directory and must survive,
    // which is the bug the check below covers. The security property is the
    // two checks after this one — the spend key and the persona secret are
    // no longer readable — not the absence of every file.
    check(
        "the sealed store's plaintext is gone",
        !File(base, "prefs/ducat_contacts.json").isFile,
    )
    check("sealed store exists", base.walk().any { it.name.endsWith(".enc") })

    // A store that is deliberately NOT sealed must survive the vault being
    // created. DeskContext writes every store as prefs/<name>.json, so the
    // migration used to glob the lot — encrypting and deleting the plain
    // ones too, after which their own reader opened <name>.json, found
    // nothing, and read as empty: a desk reverting to the system language
    // with the operator's choice still on disk in a file nothing would ever
    // open again. Only names securePrefs has been asked for are its
    // business now.
    run {
        val plainStore = context.getSharedPreferences("ducat_locale", 0)
        check(plainStore.getString("lang", null) == "fr") {
            "VAULT_FAIL a deliberately-plain store was eaten by the vault: " +
                "lang=${plainStore.getString("lang", null)}"
        }
        println("VAULT ok   a deliberately-plain store is left alone")
    }
    val after = onDisk()
    check("spend key no longer on disk in the clear", !after.contains(spend))
    check("persona secret no longer on disk in the clear",
        !after.contains(java.util.Base64.getEncoder().encodeToString(persona)))

    // 3. The desk still reads its own data through the sealed store.
    val ctx2 = DeskContext(base)
    check("wallet reads back through the vault", WalletStore(ctx2).spendKeyHex() == spend)
    check("persona reads back through the vault",
        PersonaStore(ctx2).secret().contentEquals(persona))

    // 4. Writes keep working, and keep landing sealed.
    securePrefs(ctx2, "ducat_contacts").edit().putString("vault_probe", "hello-probe").apply()
    check("a write round-trips",
        securePrefs(ctx2, "ducat_contacts").getString("vault_probe", null) == "hello-probe")
    check("and does not appear in the clear", !onDisk().contains("hello-probe"))

    // 4b. The same write, read back the way a relaunch reads it.
    //
    //     Case 4 above passed for two years while nothing was being sealed at
    //     all. `securePrefs` caches by name, so it handed back the same
    //     instance, whose in-memory map had the value regardless of whether a
    //     byte ever reached the disk — and "not in the clear" is trivially
    //     true of something never written. It was testing memory.
    //
    //     Locking drops every cached store, which is what closing the app
    //     does. Only after that does reading prove durability.
    DeskVault.lock()
    check("relock/unlock for a cold read", DeskVault.unlock(base, "correct horse battery").isSuccess)
    check("a chained write survives a lock cycle",
        securePrefs(DeskContext(base), "ducat_contacts").getString("vault_probe", null) == "hello-probe",
        "the editor's puts must return the sealing wrapper, not the plain one")

    // 4c. A store nobody has written yet is empty, not a parse error. This
    //     threw `A JSONObject text must begin with '{'` — the scratch file
    //     exists from the moment it is made and holds nothing until there is
    //     something to decrypt into it.
    val virgin = runCatching {
        securePrefs(DeskContext(base), "ducat_never_written").getString("anything", null)
    }
    check("an unwritten store reads as empty", virgin.isSuccess && virgin.getOrNull() == null,
        virgin.exceptionOrNull()?.message ?: "")

    // 4e. Every store this app has, opened cold on a protected desk.
    //
    //     This is the shape of the bug a released macOS build actually shipped
    //     with: set a passphrase, launch, and the first store nobody had
    //     written yet threw a JSONException before any screen drew. It was not
    //     particular to the store that happened to be new — every one of them
    //     was one launch away from it. So name them all, and open them all.
    val stores = listOf(
        "ducat_catalogue", "ducat_ceremonies", "ducat_contacts", "ducat_desk_place",
        "ducat_enquiries", "ducat_listings", "ducat_locale", "ducat_orders",
        "ducat_pin", "ducat_rides", "ducat_units",
    )
    val cold = DeskContext(base)
    val broke = stores.mapNotNull { name ->
        runCatching { securePrefs(cold, name).getString("nothing_is_here", null) }
            .exceptionOrNull()?.let { "$name: ${it.message}" }
    }
    check("every store opens cold on a protected desk", broke.isEmpty(), broke.joinToString("; "))

    // 4d. The order a real first launch takes: vault first, wallet after. The
    //     migration path seals by a different route, so cases 2 and 3 above
    //     never exercised the editor — this is the one that mints money.
    val fresh = File(base, "firstrun").apply { mkdirs() }
    DeskVault.lock()
    check("a second desk takes a passphrase", DeskVault.create(fresh, "correct horse battery").isSuccess)
    WalletStore(DeskContext(fresh)).save("5FirstRun", "b".repeat(64), 2_190_001uL, true)
    DeskVault.lock()
    DeskVault.unlock(fresh, "correct horse battery")
    check("a wallet made under the vault is still there next launch",
        WalletStore(DeskContext(fresh)).address() == "5FirstRun",
        "otherwise every launch mints a new one and yesterday's coin is unspendable")
    DeskVault.lock()
    check("back to the main desk", DeskVault.unlock(base, "correct horse battery").isSuccess)

    // 5. A wrong passphrase is refused, and refusing costs nothing.
    DeskVault.lock()
    val wrong = DeskVault.unlock(base, "not the passphrase")
    check("wrong passphrase refused", wrong.isFailure, wrong.exceptionOrNull()?.message ?: "")
    check("refusal left the vault locked", !DeskVault.unlocked)
    val right = DeskVault.unlock(base, "correct horse battery")
    check("right passphrase opens it", right.isSuccess)
    check("data survived the wrong attempt",
        WalletStore(DeskContext(base)).spendKeyHex() == spend)

    // 6. A short passphrase is not a passphrase.
    val shortOne = DeskVault.create(File(base, "other").apply { mkdirs() }, "short")
    check("a short passphrase is refused", shortOne.isFailure)

    // 7. Locked means locked: a fresh process with no passphrase cannot read.
    DeskVault.lock()
    check("locking clears the key", !DeskVault.unlocked)
    check("and a locked desk is told to ask for one", !Unlock.tryQuiet(base))

    // 8. The dangerous one: a locked desk must refuse to read, not report
    //    an empty store. Empty means "no wallet", and no wallet means the
    //    desk mints a fresh one and takes payments into it while the real
    //    one sits sealed beside it.
    val refused = runCatching { securePrefs(DeskContext(base), "ducat_contacts").getString("x", null) }
    check("a locked desk refuses to read a store", refused.isFailure,
        refused.exceptionOrNull()?.message ?: "it answered instead")
    val wouldMint = runCatching { WalletStore(DeskContext(base)).address() }
    check("and therefore cannot be tricked into a second wallet", wouldMint.isFailure)

    println(if (failures == 0) "VAULTTEST OK" else "VAULTTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
