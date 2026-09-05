# DUCAT desk

DUCAT on a bigger screen: a Tauri window over `ducat-app`, the application
logic in Rust that the phone and the desk share.

## Build

Prerequisites, once per machine:

- **Fedora:** `sudo dnf install webkit2gtk4.1-devel libsoup3-devel javascriptcoregtk4.1-devel gtk3-devel librsvg2-devel openssl-devel`
- **Debian/Ubuntu:** `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libssl-dev build-essential`
- **macOS:** Xcode command line tools (`xcode-select --install`)
- **Windows:** Microsoft C++ Build Tools and WebView2 (present on Windows 10/11)
- Everywhere: Rust (stable), Node 20+, pnpm.

Then, from this directory:

    pnpm install
    pnpm tauri dev        # a window with hot reload
    pnpm tauri build      # installers under src-tauri/target/release/bundle

The Rust half also builds on its own, without the web toolchain:

    cargo build --manifest-path src-tauri/Cargo.toml

`src-tauri` is its own Cargo workspace on purpose: it needs webkit to
compile, and a box without webkit — CI, a phone build machine — must still
be able to `cargo test --workspace` on everything else.

## Where things are

- `../../app/` — `ducat-app`: stores, releases, sites; the logic, tested with `cargo test -p ducat-app`
- `src-tauri/src/lib.rs` — the commands, one thin call each into `ducat-app`
- `src/lib/*.svelte` — the screens; `src/lib/api.ts` types every command once

State lives under `$XDG_DATA_HOME/ducat` (Linux), `~/Library/Application Support/ducat`
(macOS) or `%APPDATA%\ducat` (Windows). `DUCAT_DESK_STATE=<dir>` names an
identity explicitly; two desks on one machine are two directories.
