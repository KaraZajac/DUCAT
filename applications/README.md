# The DUCAT clients

Every user-facing build lives here. One Gradle project, two modules, and the
platforms each of them reaches:

```
applications/
  android/      the phone (Kotlin/Compose) — Android APKs, three ABIs
  desktop/      DUCAT Desk (Compose Desktop) — Linux, Windows, macOS
  ios/          not yet; see below
```

```sh
cd applications
./gradlew :android:assembleDebug          # the phone
./gradlew :desktop:run                    # the desk, from source
./gradlew :desktop:packageDistributionForCurrentOS   # an installer, for this OS
```

There is no `windows/`, `macos/` or `linux/` directory, and that absence is
the design: **the desk is one codebase**. jpackage can only build a `.deb`
or `.rpm` on Linux, an `.msi` on Windows, a `.dmg` on macOS — so the split
that matters is which *machine* runs the build, not which folder holds the
source. `.github/workflows/desk.yml` does exactly that: a tag push fans the
same module out to four runners and attaches every installer to the release.
Three copies of one source tree would be three places for the same bug to
diverge.

**iOS** gets a folder when there is something to put in it. Nothing
forecloses it — uniffi generates Swift bindings natively and the Rust stack
compiles for iOS, so the protocol layer is free; the UI and App Store review
are the cost.

Read [`DESIGN.md`](DESIGN.md) first — it settles the two decisions that
constrain everything else (the balance screen, and what "Send / Request"
actually means).

## Toolchain

- **JDK 21.** AGP does not accept Java 25. This repository no longer pins
  `org.gradle.java.home` — it named one developer's path and broke every CI
  runner. Set `JAVA_HOME`, or pin it in your own `~/.gradle/gradle.properties`.
- **SDK 35**, build-tools 35, **NDK 27.2** for the Rust core.
- Gradle 8.11.1 via the wrapper — no system Gradle needed.

## Permanent facts about the Android build

These cannot change once published, and the source says so where they live:

- `applicationId = org.ducatproject.ducat`. A different one is a different
  app, with no update path and no install base.
- The NFC AID `F04455434154` (`0xF0` + `"DUCAT"`), declared in
  `res/xml/apduservice.xml` exactly as §18.7 pins it — it cannot be
  discovered at runtime, so a change is a simultaneous update of every
  client that exists.
- The `ducat:` URI scheme for §18.7's QR token mode.
- `allowBackup="false"`, because §4.3 makes backup an explicit,
  passphrase-protected export the user performs. Letting the OS sweep wallet
  keys into cloud backup would defeat the design.

The Gradle module is `:android`, but its artifacts are still named
`app-<abi>-debug.apk`: the published install URL
(`/releases/latest/download/app-arm64-v8a-debug.apk`) is in the README and in
people's browsers, and a rename would quietly break it.

## Signing

Debug key, deliberately, for now. §11 requires a release to be reproducibly
built and signed by a key published independently of the site — a pre-release
task, not a pre-build one. **It stops being acceptable the moment these
builds are meant for real money.**

## The bridge

`mobile/` wraps `core` with UniFFI: a `.so` per Android ABI, and the host
library the desk loads through JNA. **It adds no logic** — every function
forwards, because a wrapper is exactly where a quiet second implementation
appears: one rounding choice, one "+1 for safety", and the app is answering a
different question from the vectors. `cargo test -p ducat-mobile` compares
the bridge against `core` directly rather than against expected constants, so
it fails if the wrapper starts having opinions.

```sh
./mobile/build-android.sh     # rebuild all three ABIs, regenerate bindings
cargo build --release -p ducat-mobile   # the host library, for the desk
```

Run the first after **any** change to `core/` or `mobile/`. Nothing checks
that the `.so` in `jniLibs` matches the current source: the app will happily
load a stale one and behave like an older protocol, which is §18.12's drift
wearing different clothes. `jniLibs/` is gitignored for the same reason — a
committed binary is a binary nobody rebuilds.

## How the desk borrows the phone's brain

`desktop/build.gradle.kts` compiles a named list of the phone's own source
files (`android/src/main/java/org/ducatproject/ducat/...`) against a small
Android shim in `desktop/src/main/kotlin/android/`. Mailbox, ContactStore,
Ceremony, the wallet and the chain rules are therefore **one implementation
on every screen** — editing them changes both clients, which is the point and
also the hazard worth remembering. Anything screen-shaped stays on its own
side.

Headless gates, all runnable without a window:

```sh
cd applications
./gradlew :desktop:smoke         # the stack reaches the live network
./gradlew :desktop:backuptest    # backup app-state round-trip
./gradlew :desktop:profilescope  # §16.9 profile scoping, offline
./gradlew :desktop:tilltest      # the whole till story against a real phone
./gradlew :desktop:tillcheck     # read-only: what has this till been paid?
./gradlew :desktop:arbiter       # the standing escrow arbiter (§15.12)
```
