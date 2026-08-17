<p align="center">
  <img src="docs/mascot.png" width="220" alt="The DUCAT maneki-neko, holding a Monero coin">
</p>

# DUCAT

A peer-to-peer proximity commerce protocol: **Veilid** for transport, **Monero**
for settlement, **no operator in between**. There is no DUCAT server anywhere —
contacts, messages, bills and receipts are DHT records and end-to-end sealed
payloads, and every phone runs a full Veilid node, giving back the routing and
storage it takes.

**[`ducat-protocol.md`](ducat-protocol.md) is the specification** and the primary
artifact here. Everything else exists to keep it honest.

## Install

**Android** — phone browser, newest build, no release page to navigate:

    https://github.com/KaraZajac/DUCAT/releases/latest/download/app-arm64-v8a-debug.apk

Use `armeabi-v7a` only for phones older than about 2016; `x86_64` is for emulators.

**DUCAT Desk**, the desktop client — same protocol, a shopkeeper's window:

| | |
|---|---|
| Linux | [`.deb`](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-linux-x64.deb) · [`.rpm`](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-linux-x64.rpm) · [portable](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-linux-x64.tar.gz) |
| Windows | [`.msi`](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-windows-x64.msi) · [portable](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-windows-x64.zip) |
| macOS (Apple Silicon) | [`.dmg`](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-macos-arm64.dmg) · [portable](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-macos-arm64.tar.gz) |
| macOS (Intel) | [`.dmg`](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-macos-x64.dmg) · [portable](https://github.com/KaraZajac/DUCAT/releases/latest/download/ducat-desk-macos-x64.tar.gz) |

Each carries its own Rust library and JVM — nothing to install first. They are
unsigned, so both desktop OSes will warn on first open.

**Debug-signed, stagenet only.** Not for real money.

## What it does today

<p align="center">
  <img src="docs/screenshots/chat-tab.png" width="290" alt="A bar tab in chat: drink notices, an itemised bill, a receipt">
  &nbsp;&nbsp;
  <img src="docs/screenshots/pay.png" width="290" alt="The pay screen: fiat and XMR, fee breakdown, speed, request or send">
</p>

**Money between people.** A self-custodial stagenet Monero wallet with fiat
conversion throughout, a fee-aware Max, speed selection, and a transaction
history rebuilt from the chain — sends identified by key image, change never
shown as income, every row carrying the running balance so the history and the
balance screen can be checked against each other.

**Contacts without identifiers.** A persona is a keypair. Contact cards travel
by QR, `ducat:` link, or **NFC tap** (phone-to-phone HCE, plus NDEF stickers);
one scan opens a mailbox-based conversation that works while either side is
offline. Profiles — name, picture, pronouns, email, phone, Signal — are
optional, validated on the wire, and always presented as the *claim* they are.

**Chat that carries commerce.** End-to-end encryption with one-time prekeys and
forward secrecy (X3DH-shaped, the signed-prekey fallback surfaced with an open
lock, never hidden). Payment requests the recipient reviews but can never
one-tap pay; itemised bills whose lines **must** sum to their total or the
message is refused; receipts issued by the payee, pointing at the transaction
they acknowledge; pictures (sealed chunks in their own DHT records); reactions;
opt-in read receipts that ride the log head for free.

**Operating modes are whole apps.** Pick one in the drawer and the entire
scaffold hands over — its own tabs, nothing of the wallet's: a **point of
sale** (itemised or type-a-total register, one code, receipt on payment,
mempool sighting shown as *seen, never settled*), a **bar tab** (every drink
pinged to the customer, one bill at close, tip on top, pay from the bus home),
a **taxi** (fares, meter, and today's take — see below), and a **donation
box** (a standing address any Monero wallet can give to, its linkability cost
stated on screen). Almost none of this needed a new wire object — the spec's
proudest sentence.

**Ride-hailing with no dispatcher (§15.12).** A rider's phone turns a GPS fix
into a geocell — a public bulletin board *whose address is the place itself* —
and posts a hail: a claim-once card, a destination, an offer priced from the
real driving route. Drivers watch their cell and its neighbours (the net drawn
on a live map, its size their call), read the job — pickup distance, trip
length, what a rideshare would have paid them — and claim it; the DHT referees
the race, no matchmaker exists. Acceptance arrives with a face on it: name,
car, colour, plate, ETA. Fares run ~15% under a rideshare's rider price while
the driver keeps 100% minus a cent of network fee, which is what deleting the
platform's ~30% take makes arithmetically possible. Demonstrated end to end on
the live network: hail → claim → quote → ride → payment with tip → receipt.
One stated trade: address search, routing and map tiles query OpenStreetMap's
servers — the single place DUCAT sends location off-device; the boards
themselves never carry better than ~1.2 km.

**Escrow with no company behind it (§17.9).** Accepting a ride can bond its
fare: rider, driver and a mutually trusted arbiter contact run a distributed
key generation over the sealed thread — three devices derive one Monero
address, each holding only a share, any two able to sign, no dealer anywhere.
The fare sits where nobody can spend it alone. The happy path is two taps:
the driver proposes the FROST release, the rider consents. Anything else is a
settlement — either side proposes a split (one number: what goes back), the
other signs or counters, whoever signs ends it. A stranded party asks the
arbiter, whose co-signature *is* the ruling; a captured arbiter can at worst
pick between the named parties, never pay itself, and the desk arbiter keeps
the judgment human — requests print, approval is a line someone types. No
shared arbiter? The 2-of-2 rung holds both sides hostage honestly: fare plus
a rider margin, released only together. The same frame is a reservation —
rent and two deposits in one escrow, the host's acceptance *being* the
funding of their own stake. Chain-proven on stagenet in every shape,
including a two-input FROST release.

**Receipts are records, not messages.** Every receipt lives in its own store,
survives thread and contact deletion, rides the backup, and dresses the
Activity tab like a bank statement: counterparty, memo, itemisation, and a
pending section for requests not yet answered. The memo travels in the sealed
notice — never on the chain, because a public memo is a note stapled to a
banknote.

**And the app behaves like an app**: notifications that hide amounts from the
lock screen and deep-link into the thread they announce, unread badges, an
encrypted backup that carries the *people* (contacts, outbox keys, prekeys,
chain counters) as well as the money, a staleness nudge when contacts outgrow
the last export, redacted crash-surviving logs, and a torch on the scanner —
because codes get scanned across dark bars. It speaks twenty languages —
every screen reads from resources, plurals are composed rather than
concatenated, and Arabic and Farsi prove the mirror.

## The repository

```
ducat-protocol.md   the spec — draft 0.88, changelog first
core/               reference implementation (Rust)
vectors/            241 conformance vectors + schema — the published artifact
conformance/        three checkers: schema, second implementation, spec audit
harness/            end-to-end over real Veilid routes and real settlement
sim/                offline simulator and market scenarios
applications/       every client: android/ (Kotlin/Compose over a UniFFI
                    bridge) and desktop/, which compiles the same protocol
                    sources verbatim into DUCAT Desk for Linux/Windows/macOS
                    — it has played bar counter, driver, and standing escrow
                    arbiter. iOS gets a folder when it earns one.
mobile/             the Rust bridge: wallet, scanner, mailbox, node
research/           one-off measurements: Veilid throughput, Monero multisig,
                    FROSTLASS, wallet-layer probes. Evidence, not product.
```

## Checking it

```sh
cargo test --workspace                      # core, sim, harness
python3 conformance/validate_vectors.py     # every vector against schema.json
python3 conformance/ducat_check.py          # a second implementation runs them
python3 conformance/audit_spec.py           # the document against the code
```

The last one exists because prose drifts from code silently — a stale sentence
throws no exception. It has caught a normative section that was referenced three
times and never written, a field range the registry did not declare, and six
vector kinds the document never named.

## On the shoulders it stands on

DUCAT rides [Veilid](https://veilid.com), a network built by people who
explicitly refused to monetize one, and treats that as a **dependency, not a
coincidence** — the spec's stewardship section (§18.7) makes it normative: no
protocol fees, no node payment, ever; every client a full participant; records
minted for live purposes and forgotten when spent. The delivery model follows
[VeilidChat](https://veilid.com)'s published corrections rather than repeating
its mistakes, and says so in the changelog. Settlement is
[Monero](https://www.getmonero.org), scanned and spent by an embedded
[monero-oxide](https://github.com/monero-oxide/monero-oxide) wallet — no
`wallet-rpc` daemon on the phone.

## What is proven, and what is not

Demonstrated end to end on stagenet, over live private routes: `direct`, `fast/1`
and escrow settlement; card exchange and claim in both directions (as URIs —
never yet over NFC); ten attacks refused; the abandonment paths that leave a
single-sided receipt; the bar-tab flow phone-to-desktop, bill to receipt; and
two complete dispatched rides phone↔desktop — geocell hail to on-chain
settlement with tip and receipt, the second without a single lost message.
And the escrow arc: two phones and a desk arbiter built a 2-of-3 over the
mailbox and independently derived one address; a bonded hail ran fund →
fare-secured-by-own-scan → consent release → broadcast, driver paid. Every
release shape is a distinct transaction verified on-chain through the one
shipping engine: the plain sweep, the split, the arbiter ruling signed with
the funder absent, and a reservation's two-input release spending both
parties' stakes at once.

Not proven, and stated here rather than buried: **no external adversarial
review** (§2.5 is the project's own argument for why that matters), no
implementer who has never read `core/` (O21), NFC compile-verified but never
field-tested between two phones, and no measurement of the tap on a handset.
The latency figures in §8.7.2 are a desktop with an attached node. The
2-of-2 ride and the reservation banner are code-complete with their
cryptography proven, but their own two-phone UI passes are still owed; and a
co-signer today consents to a stated fee, not an itemised destination list —
honest consent waits on an upstream wallet API making payments readable.

## License

[BSD-3-Clause](LICENSE) — the same license as Monero. Note that §18.7's
obligations (no carriage fees, full participation) are conformance
requirements of the *protocol*, not terms of the license: build anything you
like from this code, but a client that monetizes carriage is not DUCAT.
