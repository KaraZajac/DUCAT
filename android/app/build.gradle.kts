plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.ducatproject.ducat"
    compileSdk = 35

    defaultConfig {
        // **Permanent.** An applicationId cannot be changed once published: a
        // different one is a different app, with no update path and no install
        // base. §18.7's AID is immutable for the same reason and by the same
        // logic — decide once, or decide never.
        applicationId = "org.ducatproject.ducat"
        minSdk = 26          // HCE needs 19; 26 is where Keystore and BLE settle down
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    // One APK per ABI. A universal build carries every architecture's copy of a
    // 12 MB native library, so a phone downloads three and uses one.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86_64")
            isUniversalApk = false
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Debug signing for now, deliberately. §11 requires a release to be
            // reproducibly built and signed by a key published independently of
            // the site; that is a pre-release task, not a pre-build one. It stops
            // being acceptable the moment an APK leaves this machine.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
}

dependencies {
    // UniFFI's generated bindings need JNA on Android.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    // Worth ~30 MB of dex, which is a real cost and not the deciding one: the
    // app is self-hosted rather than squeezed into a store limit, and picking a
    // less apt glyph to save download size would be optimising the wrong thing.
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    // QR encoding only. §16.9's card is ~1 KB, which is a dense but valid
    // symbol; the alternative was hand-rolling Reed-Solomon, which is not a
    // thing to hand-roll for a code someone scans to add a friend.
    implementation("com.google.zxing:core:3.5.3")
    // Veilid's protected store is not a file it manages itself: on Android it
    // reaches back through JNI for `androidx.security.crypto.MasterKey` and
    // `EncryptedSharedPreferences`. Without this on the classpath the keyring
    // fails, and because `allow_insecure_fallback` defaults to false the node
    // refuses to start at all — "Could not initialize the protected store",
    // which reads like a Veilid bug and is a missing dependency.
    //
    // The 1.1.0-alpha line specifically: 1.0.0 ships `MasterKeys` (plural,
    // static helpers) and the JNI code loads `MasterKey$Builder`.
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}
