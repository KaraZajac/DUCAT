import org.jetbrains.compose.desktop.application.dsl.TargetFormat

// The desktop client: the same Rust underneath, the same Compose idiom on
// top, on Linux/Windows/Mac. The uniffi bindings are the identical file the
// Android app compiles (copied in by mobile/build-android.sh); the native
// library is the host build of ducat-mobile, found via jna.library.path.
plugins {
    id("org.jetbrains.kotlin.jvm")
    id("org.jetbrains.compose")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The phone's protocol brain, compiled verbatim: these files touch Android
// only through the shims in src/main/kotlin/android/. Grown one file at a
// time, deliberately — anything ui/ or screen-shaped stays on the phone.
val sharedLogic = listOf(
    "org/ducatproject/ducat/Mailbox.kt",
    "org/ducatproject/ducat/Ceremony.kt",
    "org/ducatproject/ducat/ContactStore.kt",
    "org/ducatproject/ducat/MyProfile.kt",
    "org/ducatproject/ducat/DucatLog.kt",
    "org/ducatproject/ducat/RideStore.kt",
    "org/ducatproject/ducat/Wallet2.kt",
    // The screens themselves, now that R and stringResource resolve here.
    // Each one that crosses is a capability the desk stops lacking and a
    // wording the two clients cannot disagree about.
    "org/ducatproject/ducat/Ledger.kt",
    "org/ducatproject/ducat/Amounts.kt",
    "org/ducatproject/ducat/Locales.kt",
    "org/ducatproject/ducat/Modes.kt",
    "org/ducatproject/ducat/Units.kt",
    "org/ducatproject/ducat/ui/Theme.kt",
    "org/ducatproject/ducat/ui/Type.kt",
    "org/ducatproject/ducat/ui/Catppuccin.kt",
    "org/ducatproject/ducat/ui/Activity.kt",
    "org/ducatproject/ducat/ui/TxDetail.kt",
    "org/ducatproject/ducat/Wallet.kt",
    "org/ducatproject/ducat/Tabs.kt",
    "org/ducatproject/ducat/SecondOpinion.kt",
    "org/ducatproject/ducat/SafeImage.kt",
    "org/ducatproject/ducat/Posters.kt",
    "org/ducatproject/ducat/ui/Qr.kt",
    "org/ducatproject/ducat/ui/Balance.kt",
    "org/ducatproject/ducat/ui/Monero.kt",
    "org/ducatproject/ducat/ui/SyncStatus.kt",
    "org/ducatproject/ducat/ui/Accounts.kt",
    "org/ducatproject/ducat/ui/Network.kt",
    "org/ducatproject/ducat/ui/Logs.kt",
    "org/ducatproject/ducat/ui/Diagnostics.kt",
    "org/ducatproject/ducat/ui/Donate.kt",
    "org/ducatproject/ducat/ui/Pos.kt",
    "org/ducatproject/ducat/ui/BarTab.kt",
    "org/ducatproject/ducat/ui/Profile.kt",
    "org/ducatproject/ducat/ui/ChatList.kt",
    "org/ducatproject/ducat/ui/Contacts.kt",
    "org/ducatproject/ducat/ui/Pay.kt",
    "org/ducatproject/ducat/ui/Ceremony.kt",
    "org/ducatproject/ducat/ui/RentSearch.kt",
    "org/ducatproject/ducat/ui/HomeTiles.kt",
    "org/ducatproject/ducat/ui/ClaimErrors.kt",
    "org/ducatproject/ducat/ui/NameGate.kt",
    "org/ducatproject/ducat/Rings.kt",
    "org/ducatproject/ducat/ui/Renting.kt",
    "org/ducatproject/ducat/ui/Onboarding.kt",
    "org/ducatproject/ducat/Geo.kt",
    "org/ducatproject/ducat/Orders.kt",
    "org/ducatproject/ducat/ui/Kiosk.kt",
    "org/ducatproject/ducat/Pin.kt",
    // The tap on the wire. The phone's Tap.kt, which holds the antenna, is
    // replaced by TapDesk.kt below.
    "org/ducatproject/ducat/nfc/TapWire.kt",
    // The platform-free half only; DeviceLockAndroid is BiometricPrompt
    // and fragments, and stays on the phone.
    "org/ducatproject/ducat/DeviceLock.kt",
    "org/ducatproject/ducat/ui/PinGate.kt",
    "org/ducatproject/ducat/Catalogue.kt",
    "org/ducatproject/ducat/ui/Items.kt",
    "org/ducatproject/ducat/Hailing.kt",
    "org/ducatproject/ducat/Listings.kt",
    "org/ducatproject/ducat/Enquiries.kt",
    "org/ducatproject/ducat/Stakes.kt",
    "org/ducatproject/ducat/Places.kt",
    // §15.12's per-country fare table, which Places.kt prices from.
    "org/ducatproject/ducat/FareRates.kt",
    "org/ducatproject/ducat/ui/Drawer.kt",
    "org/ducatproject/ducat/ui/Shells.kt",
    "org/ducatproject/ducat/ui/Hail.kt",
    "org/ducatproject/ducat/ui/Taxi.kt",
    "org/ducatproject/ducat/ui/LocationShared.kt",
    "org/ducatproject/ducat/ui/BackupSettings.kt",
    "org/ducatproject/ducat/ui/MyProfileEditor.kt",
    "org/ducatproject/ducat/ui/Chat.kt",
    "org/ducatproject/ducat/ui/QrHub.kt",
    // Desk-side glue that lives inside the shared package — these two match
    // files in *this* module's tree (like SecurePrefsDesk below), not app/.
    "org/ducatproject/ducat/DeskGlue.kt",
)

// The phone's resources, made available to the phone's screens compiled here.
//
// R is generated from the same res/values XML the APK is built from, so an
// id cannot mean one string on the phone and another on the desk; the tables
// are emitted per locale and read at runtime by android/Resources.kt. This
// is what lets a screen come across verbatim, translations and all, instead
// of being retyped into a second implementation that drifts.
val deskResDir = layout.buildDirectory.dir("generated/deskres")
val generateDeskRes = tasks.register("generateDeskRes") {
    val resRoot = rootProject.file("android/src/main/res")
    inputs.dir(resRoot)
    outputs.dir(deskResDir)
    doLast {
        val out = deskResDir.get().asFile
        val src = File(out, "kotlin/org/ducatproject/ducat").apply { mkdirs() }
        val tables = File(out, "resources/deskres").apply { mkdirs() }
        val drawables = File(tables, "drawable").apply { mkdirs() }

        fun textOf(node: org.w3c.dom.Node): String {
            // Android's escaping: \' \" \n, and CDATA-free markup is rare
            // enough here that the child text is the whole value.
            val raw = StringBuilder()
            val kids = node.childNodes
            for (i in 0 until kids.length) raw.append(kids.item(i).textContent)
            return raw.toString()
                .replace("\\'", "'").replace("\\\"", "\"").replace("\\n", "\n")
        }

        val builder = javax.xml.parsers.DocumentBuilderFactory.newInstance()
            .newDocumentBuilder()
        // values/ is the base; values-xx/ are the translations.
        val dirs = resRoot.listFiles { f: File -> f.isDirectory && f.name.startsWith("values") }
            ?.sortedBy { it.name } ?: emptyList()
        // Ids come from the base locale, sorted, so they are stable across
        // builds and machines — a resource id that moves is a mistranslation.
        val strings = sortedSetOf<String>()
        val plurals = sortedSetOf<String>()
        val arrays = sortedSetOf<String>()
        val perLocale = linkedMapOf<String, Triple<MutableMap<String, String>, MutableMap<String, MutableMap<String, String>>, MutableMap<String, MutableList<String>>>>()

        for (dir in dirs) {
            val tag = if (dir.name == "values") "en" else dir.name.removePrefix("values-")
            val s = linkedMapOf<String, String>()
            val p = linkedMapOf<String, MutableMap<String, String>>()
            val a = linkedMapOf<String, MutableList<String>>()
            dir.listFiles { f: File -> f.name.endsWith(".xml") }?.sortedBy { it.name }?.forEach { f ->
                val doc = runCatching { builder.parse(f) }.getOrNull() ?: return@forEach
                val ss = doc.getElementsByTagName("string")
                for (i in 0 until ss.length) {
                    val e = ss.item(i) as org.w3c.dom.Element
                    val name = e.getAttribute("name")
                    if (name.isNotEmpty()) s[name] = textOf(e)
                }
                val ps = doc.getElementsByTagName("plurals")
                for (i in 0 until ps.length) {
                    val e = ps.item(i) as org.w3c.dom.Element
                    val name = e.getAttribute("name")
                    if (name.isEmpty()) continue
                    val items = e.getElementsByTagName("item")
                    val q = linkedMapOf<String, String>()
                    for (j in 0 until items.length) {
                        val it2 = items.item(j) as org.w3c.dom.Element
                        q[it2.getAttribute("quantity")] = textOf(it2)
                    }
                    p[name] = q
                }
                val az = doc.getElementsByTagName("string-array")
                for (i in 0 until az.length) {
                    val e = az.item(i) as org.w3c.dom.Element
                    val name = e.getAttribute("name")
                    if (name.isEmpty()) continue
                    val items = e.getElementsByTagName("item")
                    a[name] = (0 until items.length)
                        .map { textOf(items.item(it)) }.toMutableList()
                }
            }
            if (tag == "en") { strings += s.keys; plurals += p.keys; arrays += a.keys }
            perLocale[tag] = Triple(s, p, a)
        }

        val stringId = strings.withIndex().associate { (i, n) -> n to i + 1 }
        val pluralId = plurals.withIndex().associate { (i, n) -> n to i + 100_000 }
        val arrayId = arrays.withIndex().associate { (i, n) -> n to i + 150_000 }

        // The raster drawables the screens name, by id, copied in.
        val drawableNames = listOf("ducat_cat" to "drawable-nodpi/ducat_cat.png")
        val mipmapNames = listOf("ic_launcher" to "mipmap-xxxhdpi/ic_launcher.png")
        var nextArt = 200_000
        val artIds = linkedMapOf<String, Int>()
        (drawableNames + mipmapNames).forEach { (name, rel) ->
            val id = nextArt++
            artIds[name] = id
            val f = File(resRoot, rel)
            if (f.isFile) f.copyTo(File(drawables, "$id.png"), overwrite = true)
        }

        // JSON string escaping, including the control characters Android's
        // own \n unescaping puts back into the text.
        fun esc(s: String) = buildString {
            for (c in s) when {
                c == '\\' -> append("\\\\")
                c == '"' -> append("\\\"")
                c == '\n' -> append("\\n")
                c == '\r' -> append("\\r")
                c == '\t' -> append("\\t")
                c < ' ' -> append("\\u%04x".format(c.code))
                else -> append(c)
            }
        }
        File(src, "R.kt").writeText(buildString {
            appendLine("// Generated by :desktop:generateDeskRes — do not edit.")
            appendLine("// Ids are the phone's resource names, sorted; the strings behind them")
            appendLine("// are read at runtime from /deskres/<locale>.json (android/Resources.kt).")
            appendLine("package org.ducatproject.ducat")
            appendLine()
            appendLine("object R {")
            appendLine("    object string {")
            stringId.forEach { (n, i) -> appendLine("        const val $n = $i") }
            appendLine("    }")
            appendLine("    object plurals {")
            pluralId.forEach { (n, i) -> appendLine("        const val $n = $i") }
            appendLine("    }")
            appendLine("    object array {")
            arrayId.forEach { (n, i) -> appendLine("        const val $n = $i") }
            appendLine("    }")
            appendLine("    object drawable {")
            drawableNames.forEach { (n, _) -> appendLine("        const val $n = ${artIds[n]}") }
            // Vector-only drawables still need to resolve; they draw nothing.
            appendLine("        const val ic_ducat_mono = 299998")
            appendLine("        const val ic_ducat_coin = 299999")
            appendLine("    }")
            appendLine("    object mipmap {")
            mipmapNames.forEach { (n, _) -> appendLine("        const val $n = ${artIds[n]}") }
            appendLine("    }")
            appendLine("}")
        })

        perLocale.forEach { (tag, triple) ->
            val (s, p, a) = triple
            val json = StringBuilder("{\"strings\":{")
            json.append(
                s.entries.mapNotNull { (n, v) ->
                    stringId[n]?.let { "\"$it\":\"${esc(v)}\"" }
                }.joinToString(","),
            )
            json.append("},\"plurals\":{")
            json.append(
                p.entries.mapNotNull { (n, q) ->
                    pluralId[n]?.let { id ->
                        "\"$id\":{" + q.entries.joinToString(",") { (k, v) -> "\"$k\":\"${esc(v)}\"" } + "}"
                    }
                }.joinToString(","),
            )
            json.append("},\"arrays\":{")
            json.append(
                a.entries.mapNotNull { (n, items) ->
                    arrayId[n]?.let { id ->
                        "\"$id\":[" + items.joinToString(",") { "\"${esc(it)}\"" } + "]"
                    }
                }.joinToString(","),
            )
            json.append("}}")
            File(tables, "$tag.json").writeText(json.toString())
        }
        File(tables, "index.json").writeText(
            "{\"locales\":[" + perLocale.keys.joinToString(",") { "\"$it\"" } + "]}",
        )
        logger.lifecycle(
            "deskres: ${strings.size} strings, ${plurals.size} plurals, " +
                "${arrays.size} arrays, ${perLocale.size} locales",
        )
    }
}
tasks.matching { it.name == "compileKotlin" || it.name == "processResources" }
    .configureEach { dependsOn(generateDeskRes) }

sourceSets["main"].resources.srcDir(deskResDir.map { it.dir("resources") })

kotlin.sourceSets["main"].kotlin.apply {
    srcDir(deskResDir.map { it.dir("kotlin") })
    srcDir(rootProject.file("android/src/main/java"))
    include(
        "org/ducatproject/desk/**", "uniffi/**", "android/**",
        "org/ducatproject/ducat/SecurePrefsDesk.kt",
        // Generated by generateDeskRes, and outside sharedLogic because it
        // has no counterpart in the phone's tree — AGP generates the phone's.
        "org/ducatproject/ducat/R.kt",
        // The desk's half of the phone's PlatformWindow.kt. Distinct file
        // name because the include patterns cover both source trees: two
        // files at one path would be two declarations of one function.
        "org/ducatproject/ducat/ui/PlatformWindowDesk.kt",
        "org/ducatproject/ducat/nfc/TapDesk.kt",
        "org/ducatproject/ducat/ui/ScannerDesk.kt",
        "org/ducatproject/ducat/ui/LocationDesk.kt",
        "org/ducatproject/ducat/ui/RouteMapDesk.kt",
        "org/ducatproject/ducat/DeskWindowHandle.kt",
        "org/ducatproject/ducat/nfc/TapDesk.kt",
    )
    sharedLogic.forEach { include(it) }
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
    // The screens name icons from the extended set (Receipt, ArrowUpward,
    // …); the phone gets them from the same artifact.
    implementation(compose.materialIconsExtended)
    // Skia directly, for the off-screen render test: ImageComposeScene hands
    // back a skia Image, and reading its pixels is how "did it draw" is asked.
    implementation("org.jetbrains.skiko:skiko-awt:0.8.18")
    // The generated bindings speak JNA on a plain JVM.
    implementation("net.java.dev.jna:jna:5.14.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.1")
    implementation("org.json:json:20240303")
    // QR for the on-screen card — the same pure-Java encoder the phone ships.
    implementation("com.google.zxing:core:3.5.3")
}

// Headless proof the stack stands: JVM, JNA, the Rust bridge, Veilid — no
// window involved. `./gradlew :desktop:smoke`.
tasks.register<JavaExec>("smoke") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.SmokeKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// A backup app-state round-trip, offline: proves claimed_kis_v1 (a
// StringSet) survives export/restore. `./gradlew :desktop:backuptest`.
// Proves the escrow sweep takes what is over and leaves what is funded.
// `./gradlew :desktop:escrowsweep`.
// Everybody built the same wallet, or nobody funds it.
// `./gradlew :desktop:escrowagree`.
tasks.register<JavaExec>("escrowagree") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.EscrowAgreeTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Money that arrived must not be erased by a concurrent backfill.
// `./gradlew :desktop:walletrace`.
tasks.register<JavaExec>("walletrace") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.WalletRaceTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The round-0 invite frame reads the way it was written.
// `./gradlew :desktop:inviteframe`.
tasks.register<JavaExec>("inviteframe") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.InviteFrameTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Boards rotate, so a poisoned one is abandoned rather than kept.
// `./gradlew :desktop:generation`.
tasks.register<JavaExec>("generation") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.GenerationTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// A card is answered once, and the DHT sequence proves it.
// `./gradlew :desktop:claimonce`.
tasks.register<JavaExec>("claimonce") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ClaimOnceTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Only a payment to the address a bill named can close that bill.
// `./gradlew :desktop:tabminor`.
tasks.register<JavaExec>("tabminor") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.TabMinorTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The signed prekey's term, and what still opens after it ends.
// `./gradlew :desktop:prekeyrotate`.
tasks.register<JavaExec>("prekeyrotate") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.PrekeyRotateTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// A co-signer reads where the money goes out of the payload, not out of the
// note beside it. `./gradlew :desktop:releaseread`.
tasks.register<JavaExec>("releaseread") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ReleaseReadTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// One junk write must not end a conversation. `./gradlew :desktop:wedge`.
tasks.register<JavaExec>("wedge") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.WedgeTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Whether a message has actually left the phone. `./gradlew :desktop:delivery`.
tasks.register<JavaExec>("delivery") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.DeliveryTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// An attacker on a real board: write doctored notices to somebody else's
// live cell and check a reader drops them. `./gradlew :desktop:boardattack`.
tasks.register<JavaExec>("boardattack") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.BoardAttackTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// What a signature and a proof of work buy on a board nobody owns.
// `./gradlew :desktop:boardnotice`.
tasks.register<JavaExec>("boardnotice") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.BoardNoticeTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// What the shareable log is allowed to say. `./gradlew :desktop:redact`.
tasks.register<JavaExec>("redact") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.RedactTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The log file's ceiling, which was documented but only applied at startup.
// `./gradlew :desktop:logcap`.
tasks.register<JavaExec>("logcap") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.LogCapTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// How precisely a search tells OpenStreetMap where somebody is standing.
// `./gradlew :desktop:geoprivacy`.
tasks.register<JavaExec>("geoprivacy") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.GeoPrivacyTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The attachment directory that only ever grew, and the room check that stops
// it growing. `./gradlew :desktop:attsweep`.
tasks.register<JavaExec>("attsweep") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.AttachmentSweepTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// A PIN cooldown measured on a clock the phone's owner cannot set.
// `./gradlew :desktop:pinlockout`.
tasks.register<JavaExec>("pinlockout") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.PinLockoutTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The arithmetic that stops a 416 KiB PNG decoding to 1.6 GB.
// `./gradlew :desktop:safeimage`.
tasks.register<JavaExec>("safeimage") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.SafeImageTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// What an unsigned card may do to a contact's payment address: establish one,
// confirm one, but never replace one. `./gradlew :desktop:cardaddress`.
tasks.register<JavaExec>("cardaddress") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.CardAddressTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Two contacts a person cannot tell apart: the confusable-name fold and the
// store query that drives the warning. `./gradlew :desktop:confusable`.
tasks.register<JavaExec>("confusable") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ConfusableTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// What a second node's answer is allowed to do to a sale: corroborated
// settles, unreachable still settles, unknown defers and eventually says so.
// `./gradlew :desktop:secondopinion`.
tasks.register<JavaExec>("secondopinion") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.SecondOpinionTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("escrowsweep") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.EscrowSweepTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("backuptest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.BackupTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// §4.3's strength meter, through the bridge the screen calls: the shapes it
// used to grade Strong, and the ones it still must. `./gradlew :desktop:passmeter`.
tasks.register<JavaExec>("passmeter") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.PassMeterTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Proves §16.9's profile scope offline: reach-me identifiers ride only a
// "profile" handshake, the car only a driving one, and the purpose survives the
// wire. `./gradlew :desktop:profilescope`.
tasks.register<JavaExec>("profilescope") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ProfileScopeTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The standing ride-escrow arbiter (§15.12): joins 2-of-3 builds, never
// signs a release. `./gradlew :desktop:arbiter [--args="--issue"]`.
tasks.register<JavaExec>("arbiter") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ArbiterKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// One real message from the desk's standing identity, to ring a phone's DHT
// watch — the poller battery-tier check. `./gradlew :desktop:ringtest`.
tasks.register<JavaExec>("ringtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.RingTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The till driven blind against a real phone: card → claim → greeting →
// bill → payment watched onto the chain → receipt. `./gradlew :desktop:tilltest`.
tasks.register<JavaExec>("tilltest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.TillTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The bonded ride end to end between two headless desks, over the live
// network and live stagenet. Two processes, two roles — see RideTest.kt.
// `DUCAT_RIDE_ROLE=… DUCAT_DESK_STATE=… ./gradlew :desktop:ridetest`.
// The counter, both sides, over the live network: card → claim → itemised
// bill → stagenet payment → notice → chain → receipt.
//   DUCAT_KIOSK_ROLE=shop     ./gradlew :desktop:kiosktest
//   DUCAT_KIOSK_ROLE=customer DUCAT_KIOSK_CARD=<uri> ./gradlew :desktop:kiosktest
// Two writers, one ceremony record. `./gradlew :desktop:escrowracetest`
// The tap's two halves against each other, without a radio.
// `./gradlew :desktop:taptest`
// §15.12's fare suggestion, country by country. `./gradlew :desktop:faretest`
tasks.register<JavaExec>("faretest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.FareTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("taptest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.TapTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("escrowracetest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.EscrowRaceTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("kiosktest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.KioskTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("ridetest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.RideTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Lock an existing desk (or sweep one already locked).
// `DUCAT_DESK_STATE=… DUCAT_DESK_PASSPHRASE=… ./gradlew :desktop:vaultset`.
// A desk's standing address, and what is in it. Creates the wallet if the
// state has none, so a fresh directory becomes a fundable test bank.
// `DUCAT_DESK_STATE=… [DUCAT_WALLET_SCAN=1] ./gradlew :desktop:wallet`
tasks.register<JavaExec>("wallet") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.DeskWalletKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Sweep a desk wallet somewhere useful: stagenet money left in an old role's
// state is money a faucet has to be asked for twice.
// `DUCAT_DESK_STATE=… DUCAT_PAY_TO=… DUCAT_PAY_XMR=all ./gradlew :desktop:payout`
tasks.register<JavaExec>("payout") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.PayOutKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("vaultset") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.VaultSetKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Renting discovery over the live network: one desk posts, another finds.
// `DUCAT_LIST_ROLE=… DUCAT_DESK_STATE=… ./gradlew :desktop:listtest`.
tasks.register<JavaExec>("listtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ListTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The stake arithmetic the whole trust argument rests on.
// `./gradlew :desktop:staketest`.
tasks.register<JavaExec>("staketest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.StakeTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Encryption at rest: the secret is gone from the bytes, or it is not.
// `DUCAT_DESK_STATE=<throwaway> ./gradlew :desktop:vaulttest`.
tasks.register<JavaExec>("vaulttest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.VaultTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The counter's logic — saved menu, kiosk orders, the PIN — run rather than
// rendered. `DUCAT_DESK_STATE=<throwaway> ./gradlew :desktop:countertest`.
tasks.register<JavaExec>("countertest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.CounterTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Every hosted screen rendered off-screen, to prove it draws something.
// `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:rendertest`.
// A hail from the kerb to the driver's map. `./gradlew :desktop:hailtest`
tasks.register<JavaExec>("hailtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.HailTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Does a watched board actually ring? `./gradlew :desktop:watchtest`
tasks.register<JavaExec>("watchtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.WatchTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

tasks.register<JavaExec>("rendertest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.RenderTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The shim layer that hosts the phone's screens, checked headlessly.
// `./gradlew :desktop:shimtest`.
tasks.register<JavaExec>("shimtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ShimTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The resource bridge without a window: ids, languages, plurals.
// `./gradlew :desktop:restest`.
tasks.register<JavaExec>("restest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.ResTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// Read-only: scan a desk wallet forward, print what it holds.
// `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:tillcheck`.
tasks.register<JavaExec>("tillcheck") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.TillCheckKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The desk driven blind over the live network; pairs with the Rust harness
// claiming the card it prints. `./gradlew :desktop:e2e`.
tasks.register<JavaExec>("e2e") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.E2eKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// A packaged desk must carry its own Rust: the host ducat_mobile library is
// copied into the compose resources tree, which jpackage ships beside the
// app and names at runtime via compose.application.resources.dir — main()
// points JNA there when the property exists. Dev runs keep ../target/release.
// Name and destination follow the machine doing the building, so the same
// file works on this desk and on each of CI's three OSes.
val hostOs = org.gradle.internal.os.OperatingSystem.current()
val nativeLibName = when {
    hostOs.isWindows -> "ducat_mobile.dll"
    hostOs.isMacOsX -> "libducat_mobile.dylib"
    else -> "libducat_mobile.so"
}
val resourcesArchDir = when {
    hostOs.isWindows -> "windows-x64"
    hostOs.isMacOsX ->
        if (System.getProperty("os.arch") in listOf("aarch64", "arm64")) "macos-arm64"
        else "macos-x64"
    else -> "linux-x64"
}
val prepareNativeLib = tasks.register<Copy>("prepareNativeLib") {
    from(rootProject.file("../target/release/$nativeLibName"))
    into(layout.projectDirectory.dir("resources/$resourcesArchDir"))
}
// The plugin's own resource sync (prepareAppResources) is what actually
// reads the directory, so the copy must precede *it*, not just the
// package/distributable tasks that sit above it in the graph.
tasks.matching {
    it.name.startsWith("package") || it.name.startsWith("prepareAppResources") ||
        it.name == "createDistributable" || it.name == "runDistributable" ||
        it.name == "createReleaseDistributable"
}.configureEach { dependsOn(prepareNativeLib) }

compose.desktop {
    application {
        mainClass = "org.ducatproject.desk.MainKt"
        jvmArgs += "-Djna.library.path=${rootProject.projectDir}/../target/release"

        nativeDistributions {
            targetFormats(TargetFormat.Deb, TargetFormat.Rpm, TargetFormat.Msi, TargetFormat.Dmg)
            packageName = "ducat-desk"
            description = "DUCAT Desk — peer-to-peer proximity commerce, no operator"
            // Dmg insists MAJOR > 0; the protocol's own versioning lives in the spec.
            packageVersion = "1.0.0"
            // JNA reaches for sun.misc.Unsafe; jlink strips it unless asked.
            modules("jdk.unsupported")
            appResourcesRootDir.set(layout.projectDirectory.dir("resources"))
            // icons/ holds the phone's own artwork in each platform's format
            // (see icons/make-icons.py, which regenerates all of them). An
            // installer with a default Java icon looks like something that
            // escaped a build server rather than a thing anyone meant.
            linux {
                iconFile.set(project.file("icons/ducat.png"))
                menuGroup = "Network"
            }
            windows {
                iconFile.set(project.file("icons/ducat.ico"))
                menuGroup = "DUCAT"
                // Fixed for the life of the product: it is how Windows knows
                // a new .msi replaces this app instead of installing beside it.
                upgradeUuid = "6f3c1d5a-9b24-4a17-8f0e-2c7d5b91ae43"
            }
            macOS {
                iconFile.set(project.file("icons/ducat.icns"))
                bundleID = "org.ducatproject.desk"
                dockName = "DUCAT Desk"
            }
        }
    }
}
