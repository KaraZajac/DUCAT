#!/bin/bash
# Rebuild the native libraries and regenerate the Kotlin bindings.
#
# Run this after any change to core/ or mobile/. Nothing checks that the .so in
# jniLibs matches the current source — the app will happily load a stale one and
# behave like an older protocol, which is the drift §18.12 exists to catch,
# wearing different clothes.
set -e
cd "$(dirname "$0")/.."

# veilid-core pulls libsqlite3-sys, whose build script needs the NDK's C
# compiler — not just a linker. Without these it fails with an unhelpful
# "custom build command failed" and nothing points at the cause.
# veilid-core's build script requires these by name and panics without them —
# "ANDROID_HOME or ANDROID_SDK_ROOT not set", from a build script, which reads
# like a cargo problem rather than a missing variable.
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=${ANDROID_NDK:-$ANDROID_HOME/ndk/27.2.12479018}

NDK=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$NDK/aarch64-linux-android26-clang
export CXX_aarch64_linux_android=$NDK/aarch64-linux-android26-clang++
export AR_aarch64_linux_android=$NDK/llvm-ar
export CC_armv7_linux_androideabi=$NDK/armv7a-linux-androideabi26-clang
export CXX_armv7_linux_androideabi=$NDK/armv7a-linux-androideabi26-clang++
export AR_armv7_linux_androideabi=$NDK/llvm-ar
export CC_x86_64_linux_android=$NDK/x86_64-linux-android26-clang
export CXX_x86_64_linux_android=$NDK/x86_64-linux-android26-clang++
export AR_x86_64_linux_android=$NDK/llvm-ar
# x86_64 is emulator-only and veilid-core 0.5.7's build script hardcodes an NDK
# version in its glob for libclang_rt.builtins-x86_64-android.a. Set DUCAT_ABIS
# to include it once a matching NDK is installed; a real phone is ARM.
ABIS=${DUCAT_ABIS:-"aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a"}
for t in $ABIS; do
  target=${t%%:*}; abi=${t##*:}
  cargo build -p ducat-mobile --target "$target" --release
  cp "target/$target/release/libducat_mobile.so" "android/app/src/main/jniLibs/$abi/"
done
rm -rf /tmp/uniffi-out
# Bindings come from the **host debug** build, not from a shipped library.
#
# `--library` mode discovers the interface from the symbol table, and the release
# profile strips symbols to get the .so from 30 MB to 12 MB. Generating from a
# stripped library silently produces nothing, which then shows up as Kotlin
# unable to see a function that plainly exists.
cargo build -p ducat-mobile
cargo run -p ducat-mobile --bin uniffi-bindgen -- generate \
  --library target/debug/libducat_mobile.so \
  --language kotlin --out-dir /tmp/uniffi-out
rm -rf android/app/src/main/java/uniffi/ducat_mobile
cp -r /tmp/uniffi-out/uniffi/ducat_mobile android/app/src/main/java/uniffi/
# The desktop client compiles the same bindings against the host library
# (built by `cargo build --release -p ducat-mobile`, found via
# jna.library.path in desktop/build.gradle.kts).
rm -rf android/desktop/src/main/kotlin/uniffi/ducat_mobile
mkdir -p android/desktop/src/main/kotlin/uniffi
cp -r /tmp/uniffi-out/uniffi/ducat_mobile android/desktop/src/main/kotlin/uniffi/
echo "native libraries and bindings refreshed"
