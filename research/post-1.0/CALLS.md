# Voice over the routes: measured, feasible (2026-08-30)

The question, after the swarm sustained ~3 Mbit/s through private routes:
could two DUCAT contacts hold a live audio call? Bandwidth was never the
issue — Opus speaks excellent mono voice at 24 kbit/s, under one percent
of what the swarm moved. Calls live or die on **latency, jitter, and
loss**, so those were measured rather than guessed.

## The measurement

`mobile/examples/pingtest.rs` — two processes on live Veilid, using the
app's own primitives (`node_app_call`: import the route blob, call,
await the reply — the exact path every mailbox send takes). The listener
publishes the same kind of route blob a contact card carries and echoes.

Two phases, run 2026-08-30 against the public network, default safety
(the same route settings the mailbox uses — no anonymity dial-down):

| phase | shape | result |
|---|---|---|
| first call | route import + first use | 375 ms |
| sequential | 29 × 200 B round trips | min 172 / **p50 187** / p90 291 / max 341 ms |
| voice cadence | 500 calls, 20 ms pacing (50 Hz), 160 B, 10 s | **0 lost**, p50 182 / p90 278 / max 306 ms, **jitter 65 ms** (RFC 3550 style) |

Both endpoints sat on one host; the hops between them were real Veilid
route hops (that is where private-route latency lives), but two phones on
different continents add propagation on top. Treat the numbers as a
same-region floor.

## What that means for a call

Mouth-to-ear budget at the measured medians:

    capture + Opus encode (20 ms frames)   ~25 ms
    one-way network (~RTT/2)               ~95 ms
    jitter buffer (absorbs the 65/p90)     ~120 ms
    decode + playout                       ~20 ms
    ------------------------------------------------
    ~260 ms   (p90-shaped worst spells ~350 ms)

ITU G.114 calls < 400 ms acceptable for conversation. **A full-duplex
call over default private routes lands inside that band** — the feel of a
good satellite call: perceptible turn-taking delay, no talking over each
other, no drops. Zero loss at exact voice cadence for ten straight
seconds is the striking result; Opus's PLC + inband FEC would cover the
loss that longer calls will eventually meet.

## Design sketch (a 1.4 candidate, none of it built)

- **Signalling**: a call is offered and answered on the thread (two new
  kinds — offer carries a fresh route blob + chosen codec params; answer
  carries the callee's blob). Decline/busy/hang-up are ordinary protocol
  notices. The thread is already authenticated both ways; no extra
  handshake exists to invent.
- **Media**: Opus 24 kbit/s mono, 20 ms frames, over `app_message`
  (fire-and-forget — no reply leg, so per-frame cost is *below* the
  measured app_call RTTs), seq + timestamp in a 12-byte header, jitter
  buffer 100–150 ms, PLC on gaps. ~50 pps each way measured safe.
- **Route churn**: routes die mid-call (the swarm met this; see the
  90 s watchdog). A call layer pings every few seconds and re-offers a
  fresh blob in-band the moment quality degrades — same reflex, faster
  clock.
- **The knob we did not need**: Veilid can shorten safety routes for
  less latency at less anonymity. The measured numbers are with the
  default — ship calls with full cover and leave the dial alone.
- **Already true today**: half-duplex voice is live — the chat's voice
  memos are push-to-talk over the mailbox, store-and-forward. Calls are
  an upgrade, not a rescue.

## Honest unknowns

Ten seconds is not an hour: NAT rebinding, route rotation frequency
under sustained 50 pps, and phone CPU (Opus is cheap; Veilid crypto per
small message is the question) need a longer soak on real handsets. The
harness is kept precisely so the soak is one command when the time comes.


## 2026-08-31: Opus landed, CBR on principle

`unsafe-libopus` (xiph's libopus c2rust-translated, BSD-3-Clause, pure
cargo — no cmake, no C cross toolchain) lives in the mobile crate; the
codec is Rust statics beside the call lane, shared by phone and desk,
reset on `nodeCallClose`. 16 kHz mono, 20 ms, **hard CBR 24 kbit/s = 60
bytes every packet** (unit test pins it), DTX off: VBR voice leaks speech
through packet sizes even under encryption (Wright et al., phonotactic
reconstruction), so frame size must not depend on what is said. Wire
frame = 8B header + 60B packet = 68B — 27 kbit/s against PCM's 259.

Measured desk↔desk (750 frames each way, live veilid): clean-route runs
are 750/750 both directions, jitter 3.6/4.3 ms, encode ~0.9 ms/frame.
Route allocations vary: one run drew a route dropping 47% one way while
the other direction ran perfect — hence Opus PLC on gaps ≤5 (decoder
stays continuous, post-gap frames decode clean) and test verdicts at
≤15% loss / ≤2% off-tone. Engine sync rule that the loss hunt surfaced:
the ANSWERING side must hold transmit until it first hears the caller —
its answer travels by mailbox-seconds, and frames sent into that gap
overflow the far ring and then play back late by the whole gap. And
`nodeCallSend` must not block the mic thread: a blocking send paced by
route RTT capped capture at ~14 fps, which got blamed on the emulator's
microphone. Now fire-and-forget with a 32-in-flight cap, freshest wins.

Still open: mid-call route re-allocation when quality goes bad (the 47%
run would have deserved one); FEC once PLC has field time; a real
jitter buffer if handsets show reorder beyond the ±5 window.


## 2026-08-31 afternoon: the phone leg, six defects deep

Emulator→desk was delivering 2% while desk→emulator ran clean. The chain
of real causes, each found by probing the boundary:

1. **The load-shed capability list was the killer.** Shedding SGNL and
   DIAL (§ load-shedding, yesterday) crippled the NAT-shadowed phone's
   own outbound media — 100.0% delivery with them restored. Shipped list
   is now `[ROUT, TUNL, RLAY, DHTV]`: the expensive server roles stay
   shed, the traversal machinery stays on. Desks never noticed because
   they are publicly reachable.
2. **32 concurrent app-message sends thrashed route resolution** ("could
   not get remote private route", 58% of sends). One serial sender task
   with an 8-deep drop-oldest queue: zero failures, full 50 fps, and the
   capture thread still never blocks.
3. **Route blobs outgrow guesses**: a phone allocated one past the 1200
   cap the day after the cap was pinned (blobs embed hop peer-info).
   Cap now 4096, both implementations, edge vectors re-pinned (355).
4. **A ringing call polled the whole world**: Mailbox.poll sweeps every
   contact at 1–21 s of DHT reads each; the answer queued behind
   strangers past the whole window. Ringing now polls that one contact
   (Mailbox.pollContact) every 2 s.
5. **45 s is not two mailbox trips**: offer out ~25–45 s cold, answer
   back the same. Window now 90 s; the v2 design is answering back
   through the offer's own route (the door is already open) for an
   instant connect, mailbox kind-15 remaining the record.
6. **A stale answer opened a ghost call**: a redelivered old kind-15
   activated a new ring (70 s of tone into a dead route; the silence
   watchdog saw the far side's own late frames). Answers older than
   2× the ring window no longer activate.

Test-bench lesson bought with an afternoon: answerphone processes that
never answered kept looping for their full 30-minute deadline; several
shared one store and raced sends — per-process seq counters forked the
outbound chain, and the phone rightly refused the fork ("does not follow
the one before it"), then dead-lettered and self-healed. One store, one
process, always kill the last role before launching the next.

Definitive live run (clean thread): finger-dialed, UK ringback in the
earpiece, answered in-window, phone→desk **3513/3513 frames, 0 send
failures, 100.0% decoded**; desk→phone 2995 confirmed ≈ 2990 heard
(99.8%); both hang-up paths exercised. The emulator microphone myth is
dead: it was the blocking send, then the shed, all along.
