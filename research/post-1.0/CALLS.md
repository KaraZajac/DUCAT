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
