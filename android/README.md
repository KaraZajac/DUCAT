# The DUCAT client

```sh
cd android && ./gradlew :app:assembleDebug
```

Read [`DESIGN.md`](DESIGN.md) first — it settles the two decisions that constrain
everything else (the balance screen, and what "Send / Request" actually means).

## Toolchain

- **JDK 21.** AGP does not accept Java 25; `gradle.properties` pins
  `org.gradle.java.home`. If you have 21 elsewhere, point it there.
- **SDK 35**, build-tools 35, **NDK 27.2** (for the Rust core, once bridged).
- Gradle 8.11.1 via the wrapper — no system Gradle needed.

## What is real and what is a placeholder

Real, and verified present in the built APK:

- `applicationId = org.ducatproject.ducat` — **permanent**. It cannot change once
  published: a different one is a different app, with no update path and no
  install base.
- The NFC AID `F04455434154` (`0xF0` + `"DUCAT"`), declared in
  `res/xml/apduservice.xml` exactly as §18.7 pins it. Immutable for the same
  reason — it cannot be discovered at runtime, so a change is a simultaneous
  update of every client that exists.
- The `ducat:` URI scheme for §18.7's QR token mode.
- `allowBackup="false"`, because §4.3 makes backup an explicit,
  passphrase-protected export the user performs. Letting the OS sweep wallet keys
  into cloud backup would defeat the design.

Placeholders, marked as such in the source:

- **The home screen's numbers are a stub.** §17.2's capacity arithmetic lives in
  `core::float` and stays there — a Kotlin reimplementation would be a second
  thing to keep in step, which is exactly the drift §18.12 exists to catch. The
  stub is not "temporary arithmetic", because temporary arithmetic is how two
  implementations begin.
- **`DucatHostApduService` refuses every APDU** with `6A82`. The exchange carries
  a `TapPresent`, and a tap that half-works is worse than one that does not
  exist, so it declines cleanly until the bridge to `core` lands.
- Accounts, Activity and Menu are labelled placeholders.

## Signing

Debug key, deliberately, for now. §11 requires a release to be reproducibly built
and signed by a key published independently of the site — a pre-release task, not
a pre-build one. **It stops being acceptable the moment an APK leaves this
machine.**

## The bridge

`mobile/` wraps `core` with UniFFI and builds to a `.so` per ABI. **It adds no
logic** — every function forwards, because a wrapper is exactly where a quiet
second implementation appears: one rounding choice, one "+1 for safety", and the
app is answering a different question from the vectors. `cargo test -p
ducat-mobile` compares the bridge against `core` directly rather than against
expected constants, so it fails if the wrapper starts having opinions.

```sh
./mobile/build-android.sh     # rebuild all three ABIs, regenerate bindings
```

Run it after **any** change to `core/` or `mobile/`. Nothing checks that the
`.so` in `jniLibs` matches the current source: the app will happily load a stale
one and behave like an older protocol, which is §18.12's drift wearing different
clothes. `jniLibs/` is gitignored for the same reason — a committed binary is a
binary nobody rebuilds.

What crosses the bridge today: §17.2's capacity arithmetic, §15.5.1's
verification tiers, §17.8's capacity buckets. The home screen's *capacity* is
therefore real — computed by the same code the vectors and the harness run — while
the wallet figures around it are still placeholders, because those come from a
Monero wallet and that is the next piece.

## Verified, and not

The `.so` files are built, packaged for `arm64-v8a`, `armeabi-v7a` and `x86_64`,
and the bindings compile against them. **The bridge has not been executed on a
device or emulator.** Marshalling across JNI is the one part these tests cannot
reach, and claiming otherwise would be exactly the kind of untested assertion the
harness exists to catch elsewhere.

## Next

A Monero wallet behind the balance screen, and the tap flow carrying real
`TapPresent` bytes over the AID already declared.
