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
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}
