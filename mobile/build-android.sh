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
NDK=${ANDROID_NDK:-$HOME/Android/Sdk/ndk/27.2.12479018}/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$NDK/aarch64-linux-android26-clang
export CXX_aarch64_linux_android=$NDK/aarch64-linux-android26-clang++
export AR_aarch64_linux_android=$NDK/llvm-ar
export CC_armv7_linux_androideabi=$NDK/armv7a-linux-androideabi26-clang
export CXX_armv7_linux_androideabi=$NDK/armv7a-linux-androideabi26-clang++
export AR_armv7_linux_androideabi=$NDK/llvm-ar
export CC_x86_64_linux_android=$NDK/x86_64-linux-android26-clang
export CXX_x86_64_linux_android=$NDK/x86_64-linux-android26-clang++
export AR_x86_64_linux_android=$NDK/llvm-ar
for t in aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a x86_64-linux-android:x86_64; do
  target=${t%%:*}; abi=${t##*:}
  cargo build -p ducat-mobile --target "$target" --release
  cp "target/$target/release/libducat_mobile.so" "android/app/src/main/jniLibs/$abi/"
done
rm -rf /tmp/uniffi-out
cargo run -p ducat-mobile --bin uniffi-bindgen -- generate \
  --library target/x86_64-linux-android/release/libducat_mobile.so \
  --language kotlin --out-dir /tmp/uniffi-out
rm -rf android/app/src/main/java/uniffi/ducat_mobile
cp -r /tmp/uniffi-out/uniffi/ducat_mobile android/app/src/main/java/uniffi/
echo "native libraries and bindings refreshed"
