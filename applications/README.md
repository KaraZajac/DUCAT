# The DUCAT clients

Every user-facing build lives here. Three of them, and the middle one is easy
to confuse with the third:

```
applications/
  android/      the phone (Kotlin/Compose) — Android APKs, three ABIs
  desk/         DUCAT desk — the shipped desktop client: a Tauri window over
                `ducat-app`, the crate in `app/` at the repo root, which sits
                on the same `mobile`/`core` the phone does. Linux, Windows,
                macOS
  desktop/      the earlier desk (Compose Desktop) — compiles the phone's own
                Kotlin against an Android shim. Still packages locally, but no
                release builds its installers any more; it is where the render
                test, the shim gates and the live field exercises run
  ios/          not yet; see below
```

`android/` and `desktop/` are the two modules of one Gradle project —
`settings.gradle.kts` includes exactly those two. **`desk/` is not a Gradle
module at all**: it is pnpm plus its own Cargo workspace (deliberately its own,
so a machine without webkit can still `cargo test --workspace` on everything
else), and it builds from its own directory.

```sh
cd applications
./gradlew :android:assembleDebug          # the phone
./gradlew :desktop:run                    # the Compose desk, from source
./gradlew :desktop:packageDistributionForCurrentOS   # a jpackage installer, for this OS

cd desk && pnpm install
pnpm tauri dev                            # the shipped desk, with hot reload
pnpm tauri build                          # its installers, for this OS
```

Read [`desk/README.md`](desk/README.md) for what the shipped desk does, what it
needs installed per platform, and how it is driven headlessly.

There is no `windows/`, `macos/` or `linux/` directory, and that absence is the
design: **a desk is one codebase, packaged per machine.** No packager crosses —
jpackage builds a `.deb` or `.rpm` only on Linux, an `.msi` only on Windows, a
`.dmg` only on a Mac, and `tauri build` is bound the same way — so the split
that matters is which *machine* runs the build, not which folder holds the
source. Three copies of one source tree would be three places for the same bug
to diverge.

`.github/workflows/release.yml` is where that happens. A tag push (or a manual
dispatch, which keeps the bundles as run artifacts instead of cutting a
release) starts two jobs: `apk` builds the phone's native library for all three
ABIs on the runner that has the NDK and assembles the APKs, and `desk` fans
`applications/desk` out over a four-way matrix — `ubuntu-22.04` (an older base
so the binary links against a glibc and webkit more desks have),
`windows-latest`, and `macos-latest` twice, the second cross-compiled to
`x86_64-apple-darwin` for Intel Macs. Every bundle is copied to a stable name
(`ducat-<slug>.deb`, `.rpm`, `.AppImage`, `.msi`, `-setup.exe`, `.dmg`) and
uploaded to the release the tag names.

There is **no** `desk.yml`. There was: from 2026-08-17 it jpackaged the
*Compose* module across four runners, which is the arrangement the paragraph
above used to describe. On 2026-09-05 it was replaced by `release.yml` and
`checks.yml`, in the same commit that made the Tauri desk the one that ships.

`.github/workflows/checks.yml` is the other half, on every push to master and
every pull request. Three jobs: `protocol` (the vectors, the second
implementation, the spec audit, `cargo test --workspace`), `clients` (the phone
builds and unit-tests, the Compose desk builds, every screen draws), and `desk`
(the Tauri client — its dictionaries re-generated from the phone's resources
with `--check`, `pnpm check` over the pages, and `cargo build --features
sound`). The `clients` job exists because the two Kotlin clients are not
independent: the Compose desk compiles the phone's own screens, so a phone-side
edit can break it silently.

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
- For `desk/` only, and none of it for the Gradle side: **Rust (stable), Node
  20+, pnpm**, plus the platform's webview development packages. CI pins pnpm
  10 and Node 22; `desk/README.md` lists the system packages per OS.

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

## How the Compose desk borrows the phone's brain — and its screens

**This section is about `desktop/`, not `desk/`.** The shipped Tauri desk
shares code with the phone one level lower down — through `ducat-app` and
`core`, in Rust — and has its own screens written in Svelte. What follows is
the other arrangement: a desk built out of the phone's own Kotlin.

`desktop/build.gradle.kts` compiles a named list of the phone's own source
files (`android/src/main/java/org/ducatproject/ducat/...`) against a small
Android shim in `desktop/src/main/kotlin/android/`. Mailbox, ContactStore,
Ceremony, the wallet and the chain rules are **one implementation on every
client** — editing them changes both, which is the point and also the hazard
worth remembering.

Since 0.88 that extends to the *screens*. `generateDeskRes` reads the phone's
own `res/values` XML, emits `R.kt` with stable sorted ids, and writes one JSON
table per locale; `android/Resources.kt` serves them at runtime with per-string
English fallback and real CLDR plural classes. So `stringResource(R.string.…)`
resolves here, and the phone's till, bar tab, chat, wallet, activity, profile
editor, backup and settings run on the desk **as the same source**, in all
twenty languages, rather than as a second implementation that drifts.

Six phone files stay phone-side, each for a reason no shim can fix:

| File | Why | The desk's answer |
|---|---|---|
| `Scanner.kt` | camera | `ScannerDesk.kt` — paste the code |
| `NfcReader.kt` | NFC radio | none; the QR is how a desk is tapped |
| `HailMap.kt` | osmdroid is an Android view | `RouteMapDesk.kt` — Compose-drawn route and driver net, no tile server |
| `Location.kt` | GPS | `LocationDesk.kt` — a position typed once |
| `PlatformWindow.kt` | Android inset flags | `PlatformWindowDesk.kt` |
| `Onboarding.kt` | a phone's first-run flow | the desk sets a passphrase, then requires a backup |

**The rule for anything new:** content crosses, window chrome stays home. When
a screen needs a platform's own behaviour, split it the way `PlatformWindow`
and `Locales`/`Localization` are split — a shared half and a named per-platform
half — rather than forking the screen.

Headless gates, all `desktop/`'s and all runnable without a window:

```sh
cd applications
python3 check_strings.py         # placeholders, scripts, plural classes
./gradlew :desktop:staketest     # the stake arithmetic users are promised
./gradlew :desktop:smoke         # the stack reaches the live network
./gradlew :desktop:backuptest    # backup app-state round-trip
./gradlew :desktop:profilescope  # §16.9 profile scoping, offline
./gradlew :desktop:tilltest      # the whole till story against a real phone
./gradlew :desktop:tillcheck     # read-only: what has this till been paid?
./gradlew :desktop:arbiter       # the standing escrow arbiter (§15.12)
./gradlew :desktop:restest       # resource bridge: ids, languages, plurals
./gradlew :desktop:shimtest      # the shim layer: every id, avatar encoder, clipboard
DUCAT_DESK_STATE=/tmp/v ./gradlew :desktop:vaulttest     # encryption at rest
DUCAT_DESK_STATE=/tmp/r ./gradlew :desktop:rendertest   # every screen, drawn off-screen
```

`rendertest` is the one that earns its keep: it renders each hosted screen
through `ImageComposeScene` with no display attached and fails if the result is
a blank rectangle. Compiling is not drawing — it caught two rooms that crashed
on first composition.


## The Compose desk's keys at rest

Also `desktop/`'s: the vault is `VaultSet.kt` and `VaultTest.kt` under
`desktop/src/main/kotlin/org/ducatproject/desk/`, and nothing in `desk/` or
`app/` reads `DUCAT_DESK_PASSPHRASE`. It matters because the standing headless
roles — arbiters, tills — are `desktop/` processes.

The phone keeps its spend key, persona secret and prekeys in
EncryptedSharedPreferences, whose master key lives in the Android Keystore and
never touches disk. A desktop has no such box, so the desk derives its key from
a passphrase: Argon2id with §4.3's reviewed parameters, domain-separated from
the backup's key, and XChaCha20-Poly1305 per store file.

```sh
# lock a desk that predates the vault (arbiters, tills)
DUCAT_DESK_STATE=~/ducat-arbiter DUCAT_DESK_PASSPHRASE='…' ./gradlew :desktop:vaultset
# headless tools take the same variable
DUCAT_DESK_STATE=~/ducat-arbiter DUCAT_DESK_PASSPHRASE='…' ./gradlew :desktop:arbiter
```

What it buys: a stolen disk, a synced home directory, a laptop backup, another
user on the machine — none of those yield the keys. What it does not buy:
anything against code running as the operator while the desk is open, because
the key is in memory then, as it must be.

**A locked desk refuses to read a store rather than reporting an empty one.**
That distinction is the whole game: empty means "no wallet", no wallet means
the desk mints a fresh one, and a till would then take payments into a wallet
nobody can restore while the real one sits sealed beside it.
