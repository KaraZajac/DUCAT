# Phase 4 on a real Android build — the phone↔desk walk

*2026-09-14, 09:24–10:30 UTC. Rows 4.1–4.3 of [research/post-1.0/TRUST.md](../post-1.0/TRUST.md) §8
walked on the x86_64 debug APK (`1.1.0.1053-b8a6b709`, built 03:28 from the tree
after `phase4-phone-trust.md`) on the `ducat` AVD, against headless desks from
`app/examples/mailbox.rs` (`checker`, `rater`) over the live network and the
live stagenet chain. Nothing was committed; no source was edited. Times are UTC;
the phone's clock shows them as 6:xx AM.*

**Result: every step passed.** The phone burned 0.01 XMR under its persona, the
poller signed the `BURN_PROOF` one second after the block was seen, a desk that
had never met the phone verified it against its own node in one second, the
desk's verdict came back as plain text and changed nothing on the phone, a
desk's rated receipt landed on the phone's record and the record went back and
was read, and all of it — burn, envelope, receipts, threads — survived an
uninstall and a restore from an encrypted backup. One probable bug (the
post-restore PIN dialog closing on its own), two nits, and one large
environmental finding: the emulator's default networking cannot carry a Veilid
node, and a rootless replacement for the sudo tap exists.

## The clock

| UTC | What |
|---|---|
| 09:24:48 | fresh install of `app-x86_64-debug.apk` (old `1.1.0.1024-0b49d730` uninstalled first) |
| 09:29:18 → 09:35:37 | onboarding, six steps, to Home |
| 09:37:01 | bank sends 0.015 XMR (`d3a93b27…59ef`, fee 0.000121); phone sees it at block 2207397 at 09:40:23; spendable by 10:00 |
| 09:37:01 | `checker` desk up — "node ready — 47 peers" in 9 s, `MB_CARD` printed |
| 09:25–09:54 | **phone node never attaches on SLIRP** (0 peers for 28 min, two poller restarts) |
| 09:47–09:54 | emulator relaunched inside a rootless `pasta` namespace with a real tap; app relaunched 09:54:11, **AttachedFull with 88 peers at 09:54:24** |
| 09:56:20 / 09:56:40 | card delivered by `ducat:` VIEW intent / "Add" tapped; phone-side claim done 09:56:45 |
| 10:01:19 | checker adopts the claim ("card (profile) answered by Phone Walk") — 4 min 39 s after Add |
| 10:01:40 / 10:02:15 | Burn tapped / PIN approved; `sent 1f4aa3ec752bc71a…` at 10:03:00 (45 s) |
| 10:04:31 / 10:04:32 | block 2207411 seen (91 s after the send) / envelope signed by the sweep |
| 10:04:42 | `rater <phone card>` desk up; claimed 10:05:18; receipt delivered 10:05:20; on the phone 10:05:26 |
| 10:12:27 / 10:12:30 | "Show my burn" (icon) / delivered to the checker |
| 10:12:54 / 10:12:56 | "Show my record" (icon) / delivered to the rater |
| 10:13:07 | rater reads the record: `MB_RECORD receipts=1 weighted=0 rating_x10=0` |
| 10:14:12 / 10:14:13 | checker receives the proof / verifies it: `MB_VERIFIED 0.010000 XMR … block 2207411 … after 1 tries` |
| 10:14:24 | "checked: burned 0.010000 XMR, since block 2207411" lands on the phone |
| 10:22:00 | backup exported (22,722 bytes) behind the PIN |
| 10:23:35 | uninstall + reinstall (the APK on disk was by then `1.1.0.1058-5d079cd7`, see below) |
| 10:25:30 / 10:25:34 | file picked / "Restored wallet" with the same address |
| 10:29:02 | Home; Burn screen shows "In block 2,207,411" and "Copy the proof" yields the identical envelope |

## Step 1 — onboarding to a usable wallet: PASS

The fresh install opens on **"Set up DUCAT — Step 1 of 6 — Create your
identity — A keypair on this phone. No email, no phone number, nobody to
register with — which is also why nobody can restore it for you."** with
"Create" and "I already have a backup". The six steps, each answering in
about two seconds:

1. Create → 2. **"What should people call you?"** (name "Phone Walk"; the pay
   choice left on "Yes — easier to be paid"; Continue) → 3. **"Create your
   wallet — Monero keys, generated here and held here. This is the money."**
   → 4. **"Choose a PIN"** opens a dialog: "It is asked for before any money
   leaves this phone. Nothing else needs it, so pick something you can type at
   a counter." / PIN / "Type it again" / "There is no way to reset this from
   inside the app — anything that could reset it would be a way past it. If
   you forget it, restore this wallet from your backup." / Cancel / Set PIN
   (dark until both fields agree). Afterwards the card reads "Your PIN is set.
   It will be asked for before any payment." → Next → 5. **"How strangers
   trust each other here"** (the stakes paragraph; "Makes sense") → 6. **"Back
   it up — now, not later"**, which shows **"Your address"** —
   `58aTVQNmQkyUrVH5bvC1GkTZZQAka8ep4XnM24J4HrChgE7dsJjuA9jg7kH6CdbdmW8YrVNuuN9sR19PVDPyChVPSYqT9zK`
   — and a passphrase field. "walk" leaves Create backup dark under "At least
   8 characters"; "walkwalk" reads "Weak — this file is your whole wallet" and
   enables it. "Backup created — 327 bytes." then "It is on this phone, which
   is not a backup yet. Send it somewhere else…" with "Send it somewhere" /
   "Done".

"Done" lands straight on Home — there is no separate "Ready / Finish" screen in
this flow (the `onb_done_*` strings exist but were never shown). Home:
**"Ready to spend — USD 0.00 — 0.000000 XMR — ✓ Synced"** and the six tiles.
The address on the Accounts tab matches the one the backup step showed, under
"Top up — stagenet — Send Monero to this address from any wallet or exchange…".

Funding from the bank needed one detour: `wallet -- send` does not scan first,
and the bank's local view was stale from its 04:23 burn, so the first attempt
answered `WL_FAIL send: not enough unlocked — 0.002020 of 0.015169 XMR needed
with the fee`. `wallet -- sync` (spendable 0.026898) and the send went through:
`WL_SENT d3a93b27b5b7f121c2e06b67319ff028760b77e598fd75e129411cf15df159ef fee
0.000121 XMR accepted_by 3`. The phone logged `received 0.015000 XMR at block
2207397` three minutes later and Home read "Ready to spend USD 7.69 —
0.015000 XMR" with a new card, **"Running low on notes — 1 note — enough for
one payment, then a wait for change."** Accounts: Spendable 0.015000 XMR,
Notes 1, Bond none, ✓ Synced, and the new **"Costly identity"** card.

## The network detour, and why it matters for every emulator run

The phone's node never attached on the emulator as it was running. Status
page after fourteen minutes: **"attached: Attaching · route-capable: not yet —
no routes until this · peers: 0 live, 0 reliable · routing table: 0 answering,
0 not, of 2 known"**, with the Monero half fine ("http://xmr-lux.boldsuck.org:38081,
height 2,207,400, synced yes, 201 ms"). The poller logged `transport Attaching
— 0 peer(s)` at 09:25:56, `restarting the node — unattached for 185s`, then
`unattached for 593s`. ICMP, DNS (`bootstrap.veilid.net` resolves and pings)
and TCP all worked from the guest.

The emulator had been launched by hand without `-net-tap` (no `tap-ducat`
device existed on the host — the tap needs `sudo bash scripts/emulator-tap.sh`
once per host boot, and this session cannot sudo), so the guest was on QEMU's
user-mode networking. `scripts/emulator-tap.sh`'s header says SLIRP "cannot
carry a Veilid node — reads work, every DHT set dies in fanout"; the memory
notes say the same. **The mechanism, measured:** a UDP echo server on the host
and `nc -u` from the guest — a 100-byte datagram arrives and is echoed;
600, 1200, 1400, 2000 and 4000 bytes never arrive. The bootstrap exchange and
every DHT set are larger than that, DNS is smaller — hence "reads yes, writes
never" and "0 answering of 2 known".

**The fix without sudo** (`pasta` from the passt package is installed; user
namespaces and `/dev/net/tun` are open to users): run the emulator inside a
`pasta --config-net` user+network namespace, create the tap there, NAT the
guest out through pasta's uplink, and reach adb by *entering the namespace*
rather than forwarding ports (pasta's `-t` forwards to the namespace's main
address, and the emulator binds its console/adb ports on loopback only, so a
port forward never connects):

```
pasta --config-net --quiet -- bash ns-emulator.sh      # in the ns, as this user:
  ip tuntap add dev tap-ducat mode tap; ip addr add 10.0.2.2/24 dev tap-ducat
  ip addr add 10.0.2.3/32 dev tap-ducat; ip link set tap-ducat up
  sysctl -w net.ipv4.ip_forward=1
  nft add table ip nat; nft add chain ip nat post '{ type nat hook postrouting priority 100 ; }'
  nft add rule ip nat post ip saddr 10.0.2.0/24 oifname "$UPLINK" masquerade
  exec emulator -avd ducat -port 5554 -no-window -no-audio -gpu host -no-snapshot \
       -no-boot-anim -feature -Vulkan -net-tap tap-ducat
NSPID=$(pgrep -f 'qemu-system-x86_64-headless -avd ducat')   # the emulator's pid
nsenter --preserve-credentials -U -n -t $NSPID adb -s emulator-5554 shell …
```

The guest keeps its baked 10.0.2.15 / gw 10.0.2.2 / dns 10.0.2.3 dialect
untouched (the design of `emulator-tap.sh` v2), plus the usual `svc wifi
disable` and the in-guest DNAT of 10.0.2.3:53 to 8.8.8.8. Thirteen seconds
after the app relaunched: `transport AttachedFull — 88 peer(s)`; Status
"attached: AttachedFull · route-capable: yes · peers: 84 live, 77 reliable ·
routing table: 84 answering, 24 not, of 110 known". The nat modules were
already loaded (a user namespace cannot load modules); `nft` worked as the
namespace's root. Two traps on the way: the first launch died with "Could not
initialize emulated framebuffer / Could not start renderer" because `export
$OLDENV` in zsh set `XAUTHORITY` to the whole unsplit string (the
[zsh-does-not-word-split] note, again); and `pgrep -f qemu…` matched the shell
running it, so the pid of a *host* process was recorded and adb was entered
into the wrong namespace ([ps-awk-self-match], again — pick the newest pid,
check `/proc/<pid>/ns/net`). Full script: the session scratchpad's
`walk/ns-emulator.sh` and `walk/adbn`.

**Handoff:** the `ducat` AVD is still running this way. A plain `adb devices`
on the host will list nothing — that is not the adb-server drop of the input
notes; enter the namespace as above, or kill that qemu and relaunch through
`scripts/emulator.sh 1` once a real tap exists. The phone on it holds the
restored state below (wallet `58aTVQ…9zK`, the burn, both threads).

## Step 2 — pairing the phone with a desk: PASS

`adb shell am start -a android.intent.action.VIEW -d '<ducat:card/…>'`
(the URI is base64url; nothing to escape) reaches the same road as a scan:
**"Add a contact? — This link opens a contact card for “Checker Desk”. Add
them? You can message and pay each other afterwards."**, a name field
pre-filled "Checker Desk", a "Call them" toggle, "Not now" / "Add". "Add"
opened the thread at once ("Messages are encrypted to keys that are deleted
after use. Once read, they cannot be recovered — not even by you."), the phone
logged `claimed: their outbox=VLD0:_FvYOIO…` five seconds later, and the chat
list showed "Checker Desk — No messages yet".

The desk side took longer than the kiosk walk's 85 s: `card (profile) answered
by Phone Walk` at 10:01:19, **4 min 39 s after Add** (the checker polls
`collect_claims` every 5 s; the phone had attached only two minutes before the
claim and was still filling its routing table — 84 of 110 known answering).
Not a defect, but the number to expect on a node that just came up.

## Step 3 — the burn on the phone: PASS

Accounts → **"Costly identity — Burn XMR under your name, so a stranger can
see that it cost you something. [Burn XMR]"**. The screen, in order: "A burn
sends XMR to an address whose keys nobody holds. The money is gone: nothing
comes back, and nobody can undo it." / "What you get is a proof that this
name, and no other, gave up that much. You show it to someone deciding whether
to deal with you, and their own node checks it. Nothing is published." /
**"Burning as Personal"** / amount `0.01` ("Amount in XMR") / **"Under USD
5.14 a burn proves nothing, and a reader will refuse it."** / purpose
`identity` ("What it is for — A short label: identity, a listing, an
arbiter.") / "This cannot be undone." / the button **"Burn USD 5.14"** /
"Where it goes — Both keys of this address are hashes of a published string,
so anyone can work it out for themselves and see that nobody chose a secret.
What arrives here can never be spent." / `5BKmKobctGAGwCKipy7G7gKQgbXpJzrVHC2Rnx3KpEbt7XewEkG3vAfY4cQoxTonXzKWo5oeqgXJ7aMNzgELcxp81RrV3de`
(the stagenet burn address of [ducat-burn-proofs]) / "Copy the address" /
**"Your burns — Nothing burned under this name yet."**

Because the price preference is on by default, the floor and the button speak
USD ("Burn USD 5.14") for what the Accounts tab itself calls test coins "worth
nothing"; the XMR figure appears only in the amount field. Cosmetic, noted.

Burn → the PIN gate: **"Enter your PIN — You are about to destroy money.
Nothing brings it back."** / PIN / Cancel / Approve. Approve at 10:02:15 →
the button disappears while the send runs → log `sending 0.010000 XMR using 1
note(s) to 5BKmKobctGAG…` → 45 s later `sent 1f4aa3ec752bc71a… fee 0.000122
XMR, accepted by 3 node(s)` and `Trust: burned 0.010000 XMR under f7a719a6…
(identity); waiting for its block` → the list reads **"USD 5.14 / identity /
Waiting for its block"** (10:03:05). The block came 91 s after the send:
`received 0.004878 XMR at block 2207411` (the change) at 10:04:31 and, one
second later, `burn 1f4aa3ec752b… is in block 2207411; its proof is ready to
show` — the poller's sweep signed the envelope on the first pass after the
block, no hourly wait. The list then reads **"USD 5.14 / identity / In block
2,207,411 / Copy the proof"** and the Accounts card **"Burned USD 5.14"**.
"Copy the proof" puts `ducat:burn/<666 hex>` (677 characters) on the
clipboard — verified by pasting it into the chat search field.

Timings: PIN → send accepted 45 s (the decoy download of
[monero-rpc-timeout-shape]); send → block 91 s; block → signed 1 s.

## Step 4 — show the burn, the desk checks it, the phone stays unmoved: PASS

The composer tray (the "+" beside the message box) has a second row:
**"Show my burn" · "Show my record"** — and no "Rate them", since no receipt
had ever settled in the thread. Tapping the *label* under the icon does
nothing (see nits); tapping the icon at 10:12:27 sent it: bubble **"A burn
proof" ✓ 6:12 AM**, chat-list preview "You: A burn proof", log `sealed seq 0
(803 B) … delivered seq 0 to Checker Desk` at 10:12:30.

Checker: `MB_PROOF_SEEN from Phone Walk (666 chars)` at 10:14:12 (its own log:
`received message seq 0 from Phone Walk` — 1 min 42 s after delivery, the
inbox poll of [dht-read-costs]) and, **one second later**, `Trust: verified a
burn of 10000000000 pXMR by f7a719a6… at block 2207411` → `MB_VERIFIED
0.010000 XMR by f7a719a6… at block 2207411 purpose=identity after 1 tries`,
`MB_BADGE burned 0.010000 XMR, since block 2207411`, `MB_OK checker verified
the burn after 2209s` (from its start). Its own node bore the out-proof out
for exactly the amount, a second node had the block: no `MB_NOT_YET` at all.

The reply reached the phone at 10:14:24 (`received seq 0 from Checker Desk`)
and is drawn as an ordinary incoming text bubble: **"checked: burned 0.010000
XMR, since block 2207411" — 6:14 AM**, grey, left. The header stays
**"Checker Desk"** with no badge line beneath it — the phone has verified
nothing of the desk's, and the desk's words are the desk's (screenshot
`walk/s23.png`).

## Step 5 — receipts phone↔desk: PASS (reverse direction); forward skipped

*Forward* (`rated` desk, phone rates it) is unreachable by design: "Rate them"
needs a kind-3 receipt in the thread and there is no deal with any desk here;
the tray shows no "Rate them" in either thread, before and after the restore.
Skipped, as the brief allowed.

*Reverse* (`rater <phone card>`): the phone's card was read from the QR hub
("My code" — "Phone Walk", the QR, "Copy link", "Share", "One person per code.
As soon as somebody takes this one, the next is made automatically — nothing
to do.") by decoding a screenshot with `cv2.QRCodeDetector` (410 characters).
The rater started 10:04:42, `MB_CLAIMED the rated` at 10:05:18, `MB_ATTEST_SENT
413 chars`, delivered 10:05:20. The phone logged `card (profile) answered by
Rater Desk` and cut its next code, then `received seq 0 from Rater Desk` and
**`Trust: a receipt from 2a7892e9…: 5 star(s)`** at 10:05:26 — six seconds
after the send, because its own card's record was already being watched.
Chat list: "Rater Desk — A rating for you, kept on your record"; thread bubble
**"A rating for you, kept on your record" 6:05 AM**.

"Show my record" (icon, 10:12:54) → bubble **"Your record, shown to them" ✓**,
log `sealed seq 0 (539 B) … delivered seq 0 to Rater Desk`. Rater, 11 s
later: `received message seq 0 from the rated`, `Trust: f7a719a6… showed a
record: 1 receipt(s) read`, **`MB_RECORD receipts=1 weighted=0 rating_x10=0
record_messages=1`**, `MB_OK rater read the record after 467s`. Weighted 0 is
right: the rater never verified the phone's burn, so the phone's own receipt
from it does not count for anything — the one-voice rule from the desk↔desk
run, now across clients.

## Step 6 — restore, then verify: PASS

Menu → Settings → **Backup — "One encrypted file holding your identity, your
keys and your settings. Export a fresh one whenever something important
changes."** Passphrase "walk": "At least 8 characters", Export and Import
both dark — the brief's passphrase is refused by the format's floor, so
**"walkwalk"** was used ("Weak — this file is your whole wallet"). Export →
**"Enter your PIN — A backup carries the spend key. Enter the PIN to export
or import one."** → Approve → `files/backups/ducat-backup.ducatbak`, 22,722
bytes, and the system share sheet ("Sharing 1 file — ducat-backup.ducatbak").
Pulled with `exec-out run-as … cat`, pushed to `/sdcard/Download/`.
(Screenshots of this screen are blank by design — it is FLAG_SECURE; the
uiautomator tree is the only way to read it.)

Uninstall, reinstall, grant, launch: **"Set up DUCAT — Step 1 of 6"** again.
"I already have a backup" → **"Step 1 of 2 — Restore from your backup —
Choose the file you saved and type the passphrase you set. Your identity,
your money and your people come back — nothing new is created."** →
passphrase, "Choose the file" → the picker (already on Downloads; the file
listed as "ducat-backup.ducatbak · 22.72 kB · 6:23 AM") → two seconds →
**"Restored wallet — Check this address is one you recognise before trusting
it."** with `58aTVQ…9zK` — the same address — and "Continue" → **"Step 2 of 2
— Choose a PIN"** → (see the probable bug) → "Your PIN is set…" → Next →
Home: **"Ready to spend — 0.004878 XMR — Synced"** at 10:29:02, the burn's
change already rescanned (`received 0.015000 XMR at block 2207397`, `received
0.004878 XMR at block 2207411` 35 s after the restore; the node was
AttachedFull with 73 peers 11 s after the start).

Accounts → Burn: **"Your burns — USD 5.13 / identity / In block 2,207,411 /
Copy the proof"**; the clipboard after "Copy the proof" is the *same* 677
characters, same head `ducat:burn/a201590105a9…`, same tail `…7f1ebeacfd0e`
— the envelope, its height and its txid rode the bundle (`burns_raw`) and came
back verifiable. Both threads came back too: Checker Desk with "A burn proof"
and "checked: burned 0.010000 XMR, since block 2207411", Rater Desk with "A
rating for you, kept on your record" and "Your record, shown to them"; the
tray shows **Show my burn enabled, Show my record enabled** (the received
receipt survived in `attestations_received_raw`), Rate them absent. The
restored phone also `republished every thread's one-time offer after a
restore` and re-pushed its trailing slot to each desk.

One caveat on the build: the APK at
`applications/android/build/outputs/apk/debug/` was rebuilt at 09:51 by the
concurrent vouching commit (`5d079cd7`), so steps 1–5 ran on
`1.1.0.1053-b8a6b709` and the reinstall in step 6 installed
`1.1.0.1058-5d079cd7`. The bundle written by the older build restored on the
newer one, which is the direction a real upgrade takes; a same-build restore is
not separately proven.

## Bugs and nits

1. **Probable bug — the post-restore "Choose a PIN" dialog closes on its
   own.** Twice, at 10:25:5x and 10:26:3x, the dialog (both fields present,
   confirmed by dump) vanished within seconds of typing into its first field,
   leaving the step card unchanged and no PIN set; the third attempt at
   10:27:25, with identical inputs and 1.5 s pauses, held through both fields
   and "Set PIN" and worked. The difference is what the poller was doing: the
   restore's own work ran exactly then — `republished every thread's one-time
   offer after a restore` 10:25:40, the wallet's two `received` lines 10:26:00,
   `re-pushed 1 trailing slot(s) to Checker Desk` 10:26:02, `lane: 1 record(s)
   rang` 10:26:18, `…to Rater Desk` 10:26:25 — and the log was quiet during
   the attempt that worked. `PinStep` holds the dialog's `asking` in a plain
   `remember`, so anything that takes the step out of composition and back
   (a store bump that re-derives the onboarding state in `MainActivity` is
   the candidate) resets it to closed. Confidence: medium — it never happened
   in the first-run onboarding, where nothing was being restored. Repro: back
   up a phone with two live threads, wipe, restore, and open the PIN dialog
   within a minute of "Restored wallet". The same `remember` pattern in
   `PinGate` would deserve a look ([compose-state-read-before-its-effect]).
2. **Nit — a fresh phone logs a wrong warning every ten seconds.** From
   09:25:35 until the wallet was created at 09:33:16: `W|DucatWallet|no spend
   key stored — this wallet predates wallet persistence`, 46 times. There was
   no wallet yet; the sentence describes a migration case and fires on a
   phone still in step 1 of setup.
3. **Nit — tray labels are not controls.** In the composer tray each item is a
   `Column { FilledTonalIconButton; Text(label) }` (`Chat.kt` `TrayItem`), so a
   tap on "Show my burn" the word does nothing while the icon above it sends.
   The same for Gallery, File, Money, Contact, Reservation, Location. A
   finger aims at the icon; a driver and a reader of the label do not.

Not bugs, recorded so the next walk does not chase them: the claim taking
4 min 39 s to reach the desk on a node two minutes old; the desk seeing the
proof 1 min 42 s after delivery (inbox poll) and the record 11 s after; the
Burn button vanishing for the 45 s of the send; the uiautomator tree omitting
the Export/Import row until the page was nudged (a harness artefact, the
buttons were there); `exec-out screencap` returning zero bytes on the Backup
screen (FLAG_SECURE, deliberate); and `wallet -- send` needing a `sync` first
when the bank has spent since its last scan.

## What is now proven, against §8

- **4.1** walk on phone and desk: done — the phone's burn, made, signed on the
  sweep, shown, and verified by a desk that had never met it.
- **4.2** thread part: the badge stayed empty where it should (the desk's
  verdict is the desk's), "Show my burn" and the receipt bubbles say what the
  memo said they would; the gate under Pay and the badge outside the thread
  were not exercised here.
- **4.3** phone↔desk receipts: the reverse direction end to end; the forward
  direction waits for a deal that produces a kind-3 receipt (the kiosk or
  rental walks).
- **3.3**'s "restored from a backup, still verifies": the envelope after the
  restore is byte-identical to the one the checker verified.

Artefacts: the session scratchpad's `walk/` — `checker.log`, `rater.log`,
their state dirs with `ducat.log`, `phone-prewipe.log`, `phone-restored.log`,
screenshots `s01`–`s24`, `restore.ducatbak` (passphrase "walkwalk"),
`ns-emulator.sh`, `adbn`, `ui.py`.
