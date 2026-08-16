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
    // Desk-side glue that lives inside the shared package (see DeskGlue.kt).
    "org/ducatproject/ducat/DeskGlue.kt",
    "org/ducatproject/ducat/ui/DeskHailOps.kt",
)

kotlin.sourceSets["main"].kotlin.apply {
    srcDir(rootProject.file("app/src/main/java"))
    include("org/ducatproject/desk/**", "uniffi/**", "android/**", "org/ducatproject/ducat/SecurePrefsDesk.kt")
    sharedLogic.forEach { include(it) }
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
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
tasks.register<JavaExec>("backuptest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.BackupTestKt"
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

// One real message from the desk's standing identity, to ring a phone's DHT
// watch — the poller battery-tier check. `./gradlew :desktop:ringtest`.
tasks.register<JavaExec>("ringtest") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.RingTestKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

// The desk driven blind over the live network; pairs with the Rust harness
// claiming the card it prints. `./gradlew :desktop:e2e`.
tasks.register<JavaExec>("e2e") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.E2eKt"
    jvmArgs("-Djna.library.path=${rootProject.projectDir}/../target/release")
}

compose.desktop {
    application {
        mainClass = "org.ducatproject.desk.MainKt"
        jvmArgs += "-Djna.library.path=${rootProject.projectDir}/../target/release"

        nativeDistributions {
            targetFormats(TargetFormat.Deb, TargetFormat.Msi, TargetFormat.Dmg)
            packageName = "ducat-desk"
            // Dmg insists MAJOR > 0; the protocol's own versioning lives in the spec.
            packageVersion = "1.0.0"
        }
    }
}
