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

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
    // The generated bindings speak JNA on a plain JVM.
    implementation("net.java.dev.jna:jna:5.14.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.1")
    implementation("org.json:json:20240303")
}

// Headless proof the stack stands: JVM, JNA, the Rust bridge, Veilid — no
// window involved. `./gradlew :desktop:smoke`.
tasks.register<JavaExec>("smoke") {
    classpath = sourceSets["main"].runtimeClasspath
    mainClass = "org.ducatproject.desk.SmokeKt"
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
