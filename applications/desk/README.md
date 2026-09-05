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

## What it does

Everything the phone does that a desk can, in the same protocol, from
the same identity:

| Page | What is there |
| --- | --- |
| Chat | Threads and groups; bills paid, declined, or taken back in place; replies that quote; pictures, files, and voice memos attached, small ones by record, big ones by swarm; a request, a standing bill, or a voice call from the header; a card for you or somebody's profile shared into the thread; reactions, taking a message back, disappearing messages, drafts — and in groups, reactions, quoted replies, and withdrawals named by (author, counter) |
| Wallet | Balance and sync, receive (address and code), send with a quote, history, the node, a rescan from any block |
| Till | A sale to whoever scans the code, running tabs, the catalogue priced in your currency; receipts go out when the chain agrees |
| Kiosk | Orders at a counter: a number, a `monero:` code any wallet can pay (the total carries six digits of noise so the payment is recognised), and a DUCAT card that turns the order into a bill with a receipt; ready and abandon |
| Activity | The ledger — every note in and send out with what it was for — and its CSV/JSON export |
| Library | The press (publications, issues on the shelf or the swarm, subscribers, subscribe-by-scan) and the reading room |
| Market | Browse a cell and its ring; list a thing with pictures, post it to this week's board |
| Files, Sites | A file at an address that cannot change; a site at an address that can |
| Me | Your name, your code, personas, a donation code, backup and restore |

The logic is `ducat-app`, tested with `cargo test -p ducat-app`; the
window is a thin set of commands over it. Several live-network
exercises ship as examples of that crate and were run against this
window while it was built:

    DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example mailbox -- host | guest <card> | customer <card> | reader <code> | party <name> [card...]
    DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example publish -- share <file> | site <folder> <title> | get <address>
    DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example wallet -- address | sync | send <addr> <xmr>
    DUCAT_DESK_STATE=<dir> cargo run -p ducat-app --example backup -- export <file> <passphrase> | import <file> <passphrase>

Voice calls use the machine's microphone and speaker. Built plainly, the
desk carries them through the sound server's own tools — `pw-record` and
`pw-play` (PipeWire), or `parec` and `pacat` (PulseAudio) — as child
processes moving raw 16 kHz PCM over pipes, which needs nothing to build
against. Built with `--features sound` it uses cpal instead (`alsa-lib-devel`
on Fedora, `libasound2-dev` on Debian; the native device stack on macOS
and Windows). A machine with neither connects calls without sound and
says so in the log. A debug desk started with `DUCAT_DESK_TONE_AUDIO=1`
calls with a test tone, which is how the call path is exercised headless.
Notices — a message, money, a call — go to the desktop's notification
tray when the window is not focused.

The Me page also keeps the ways to reach you away from DUCAT — email, phone,
Signal — with the switch that decides whether they ride a profile card at all.
Saving them rewrites your standing code's record in place, so a code already
printed hands out what you just saved; anyone who answered it earlier keeps
what they were given then. Anything that cannot be undone asks first, and the
chat list and the ledger each have a search box.

### The sealed room

A site opens in its own window, and that window is answered from the fetched
bundle and nothing else — the same room the phone builds. Every request goes
to a `ducat-site` protocol handler that reads the one directory registered
for that window's label; the response carries a policy of `default-src
'none'` with images, styles, fonts, and media from the bundle only and
`script-src 'none'`, so a page cannot run code, fetch, frame, or submit. The
window's navigation guard allows the bundle's own addresses, refuses
everything else, and turns a `ducat:` link into an event for the main
window: a site address is added and shown, a card lands in the chat's add
panel, and the room stays where it is. The room has no IPC: capabilities are
granted to the main window alone. "Show folder" still reveals the bundle in
the file manager for anyone who wants the files. The room's menu offers Back,
Forward, and First page (Alt+Left, Alt+Right, Alt+Home), run by the desk since
the page cannot run anything. A site that was fetched before opens from the
edition on hand when the node is offline. A `ducat:site/` address in a chat
message shows an Open button that adds the site and opens its room.

## Languages

The desk speaks the phone's nineteen languages. Every sentence a page
shows goes through `t("key")` (or `tp("key", n)` for a plural) in
`src/lib/i18n.svelte.ts`; the dictionaries under `src/lib/strings/` are
generated by `scripts/strings_from_android.py` from the phone's own
resources, so a key the phone has is translated the moment the desk uses
it. Sentences only the desk says live in `strings/values/strings_desk.xml`
(and `strings/values-<locale>/` for each language), prefixed `desk_` so
they can never shadow a phone key; `applications/check_strings.py --res
applications/desk/strings` holds them to the same rules as the phone's.
The language follows the system or is chosen on the Me page; Arabic and
Persian flip the direction.

To add a sentence: use a phone key if the phone already says it; otherwise
add a `desk_` key to `strings/values/strings_desk.xml`, translate it
(`scripts/write_desk_strings.py <locale> < strings.json` writes a
locale's file from JSON), and run `scripts/strings_from_android.py` to
regenerate the dictionaries. CI fails on a key no English file defines, a
stale dictionary, or a translation that disagrees.

Not on the desk (yet): rides in any seat — hail, taxi, drive, and the
bonded escrow behind them — need a phone's position. Cards from those
threads still land here and read as they should. 

## Keyboard

Ctrl+1 … Ctrl+9 and Ctrl+0 walk the sidebar in order (Cmd on a Mac).
Ctrl+K goes to the find box — the chat list's once it holds six or more
conversations, the ledger's on Activity. Enter sends a message, Shift+Enter
breaks a line, Escape drops a reply you were composing. Anything that cannot
be undone — forgetting a contact, clearing a chat, deleting a publication or a
listing, removing a file or a site, discarding a tab — asks first, in the
phone's own words.

## Driving the window

A debug build watches `DUCAT_DESK_DRIVE=<dir>` for `*.js` files and
evaluates each inside the page, in name order, deleting it; the page
can write back with `window.__TAURI__.core.invoke('drive_report',
{text})`, which lands in `<dir>/report.txt`. It exists because a
Wayland session ignores X11 pointer warps, so nothing outside the
window can click it; a walk of every screen after every change is how
the desk is tested. Under the drive, pages that would open a file
picker also take a typed path. Release builds compile none of this.

Under the drive, screens that would open a file dialog show a typed-path
input instead: `#fpath` (Library issue), `#ppath` (Market photo),
`#attpath` (Chat attachment), `#bpath` and `#ipath` (Me: backup export
and import). A drive script fills the input and dispatches `change`.

## Where things are

- `../../app/` — `ducat-app`: identity, contacts and the mailbox, wallet, tabs, publications, groups, listings and boards, the ledger, backup, releases, sites — the logic, tested with `cargo test -p ducat-app`
- `src-tauri/src/lib.rs` — the commands, one thin call each into `ducat-app`
- `src/lib/*.svelte` — the screens; `src/lib/api.ts` types every command once

State lives under `$XDG_DATA_HOME/ducat` (Linux), `~/Library/Application Support/ducat`
(macOS) or `%APPDATA%\ducat` (Windows). `DUCAT_DESK_STATE=<dir>` names an
identity explicitly; two desks on one machine are two directories.
On a fresh start with no state, the previous desk's directory
(`…/ducat-desk`) is adopted — copied, so nothing there is touched —
and its identity, contacts and wallet carry across; its string-kept
tables read here as structures.
