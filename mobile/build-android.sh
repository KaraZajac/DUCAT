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
# All three, always. x86_64 is emulator-only, but the emulator is where this
# gets tested, and building a subset is worse than it sounds: the bindings
# below are regenerated every run, so any ABI left out keeps an older .so and
# the app dies on launch with "UniFFI API checksum mismatch" — a crash whose
# message points at the build system rather than at the skipped architecture.
ABIS=${DUCAT_ABIS:-"aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a x86_64-linux-android:x86_64"}
for t in $ABIS; do
  target=${t%%:*}; abi=${t##*:}
  cargo build -p ducat-mobile --target "$target" --release
  cp "target/$target/release/libducat_mobile.so" "applications/android/src/main/jniLibs/$abi/"
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
rm -rf applications/android/src/main/java/uniffi/ducat_mobile
cp -r /tmp/uniffi-out/uniffi/ducat_mobile applications/android/src/main/java/uniffi/
# The desktop client compiles the app's copy of these bindings directly
# (its source set includes app/src/main/java with a filter), against the
# host library built by `cargo build --release -p ducat-mobile`.

# An ABI directory that exists but was not rebuilt now holds a library older
# than the bindings just written, and the app dies on launch for whoever
# installs that one. Catch it here rather than in a logcat trace on a device.
built=" $(for t in $ABIS; do printf '%s ' "${t##*:}"; done)"
for dir in applications/android/src/main/jniLibs/*/; do
  abi=$(basename "$dir")
  case "$built" in
    *" $abi "*) ;;
    *) echo "STALE: jniLibs/$abi was not rebuilt — its library no longer matches" \
            "the bindings, and the app will crash on launch with a UniFFI" \
            "checksum mismatch. Rebuild without DUCAT_ABIS set." >&2; exit 1 ;;
  esac
done
echo "native libraries and bindings refreshed"
