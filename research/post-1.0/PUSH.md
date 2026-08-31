# Push: from poll-clock to doorbell (v2 design, pre-implementation)

*2026-08-31. Thinking before building, at kara's ask: can we push? is it
possible? do we want it?*

"Push" here is two different pushes, and they compose:

1. **Delivery push** — DHT watches on contact outbox records, so a
   written message rings our doorbell instead of waiting for the poll
   sweep. Fixes the whole app's arrival latency: call offers, hails,
   bills, settle notices, new issues, plain chat.
2. **Signaling push** — control frames on a call's already-open route,
   so the answer/decline/hang-up travel at route speed (~200 ms), not
   mailbox speed (25–45 s cold). Fixes the telephone specifically.

## Is it possible? (verified today, veilid-core 0.5.7 source)

- A watch question rides the **record's safety selection** — same
  anonymization as our reads (`storage_manager/watch_value.rs` takes
  `safety_selection` from the opened record).
- The watcher key is a **shared static anonymous key** when `None` is
  passed (our `node_dht_watch` passes `None`).
- The host's stored notification target for a question that arrived
  through a private route **is that private route**
  (`Destination::get_target` → `import_single_remote_route`);
  `get_target_node_ids()` for that arm is `None` — **the record host
  never learns the watcher's node id**. Notifications come back through
  the route.
- Therefore: **watch privacy ≈ poll privacy.** The host learns "some
  anonymous party is interested in this record, reachable via a route" —
  which periodic polling also reveals, just on a cadence.
- The machinery half-exists in-app: `node_dht_watch` +
  open-then-watch are exported and live position (§15.12) already uses
  them; node.rs has the `CHANGED` ring + `node_changed_keys()` +
  `node_wait_change`. Known lore holds: **a watch needs its record kept
  open; close cancels silently** ([[veilid-watch-needs-open-record]]).

## Do we want it?

**For:** ring-in-seconds instead of ring-in-minutes (today an incoming
offer aged past the 90 s window in transit and correctly filed as a
missed call — the poll clock IS the incoming-call latency); battery
(idle watches beat any hot-poll ladder — an empty-record probe costs up
to 21 s of DHT work, and a 10-contact hot loop never lets the radio
sleep); product (with the fg service holding the node, a backgrounded
phone can genuinely ring — the deferred full-screen-intent work becomes
worth doing); commerce (a hail that reaches a driver in seconds is a
different product than one that takes two minutes).

**Against / costs, eyes open:**
- **Watches are best-effort.** Hosts churn, routes rotate, restarts
  drop them, and death is silent. Design rule: *the poll remains the
  ground truth*; watches only shorten latency. Poll cadence relaxes
  (battery win) but never to zero.
- Keep-open set + renewal loop + re-arm-on-restart is real lifecycle
  code (bounded; Poller owns it).
- Per-record watcher caps in veilid are irrelevant for 1:1 outboxes
  (one watcher); publisher fan-out stays on poll/shelf by design.
- Unproven on the phone: watches under the load-shed capability list
  and emulator NAT. After today's SGNL/DIAL lesson, nothing ships
  without a probe. ([[ducat-1-0-freeze]] six-defects section.)

## Design sketch

**Watches (no wire change, no spec bump):**
- Watch set = top-K contacts by recent activity (start K=12) + the
  open thread + any contact with live state (unsettled tab, active
  ride, ringing call). Poller arms, renews, re-arms after restart;
  eviction by recency.
- On ValueChanged(key): map key → contact, `Mailbox.pollContact` just
  them (~1.1 s populated read), funnel as today → notification/ring
  fires seconds after the sender writes.
- Background sweep relaxes once watches are armed; sweep also re-arms
  dead watches (self-healing).

**Call control frames (spec dev5, §16.21 media channel):**
- Reserved sentinel `seq = 0xFFFFFFFF`; frame types ANSWER (callee's
  route blob + call id), DECLINE (call id), BYE (call id).
- Auth = possession: the offer route and call id exist only inside the
  sealed offer — the same trust base the media itself already stands
  on. Kind-15/kind-5 mailbox messages remain the canonical record; the
  control frame is the fast provisional signal and must agree.
- Replay is dead on arrival: routes are per-call and the ring demuxes
  by arrival route; the id must match the live call.
- Effect: connect ~200 ms after tapping Answer (today: up to another
  mailbox trip); decline stops the caller's ringback in ~RTT; hang-up
  becomes instant with the 10 s watchdog kept for crashes.

## Sequence (each stage proves before the next builds)

- **A. Probe** (desk↔desk, then emulator under shed): watch-arm →
  write → ValueChanged latency; renewal behavior over 30+ min; survival
  across the watched node restarting; behavior when the HOST restarts.
- **B. Watches** in Poller/Mailbox + the live matrix re-run (incoming
  call rings in seconds; issue/settle arrival timed).
- **C. Control frames**: spec dev5 + engine + harness; full telephone
  matrix again (target: tap-Answer → voice under a second).


## Stage A findings (2026-08-31, live)

- **One-shot arming dies quietly.** A desk watcher that armed once
  (`WT_ARMED true`) heard nothing for twenty minutes while fifteen
  writes landed — no error surfaced anywhere. The phone, which
  re-stamps every watch each poller pass, rang reliably the whole time.
  Policy: **re-arm every pass**; veilid treats it as desired-state and
  only renegotiates on change. Arming health is now narrated
  ("watches: N armed, M not") because a failed arm is otherwise
  indistinguishable from a working watch until a message is late.
- **Arms fail until the record is open** — first pass after launch reads
  "2 armed, 18 not", and the sweep's opens turn it into "20 armed,
  0 not" one pass later. Restart re-arm therefore works with no extra
  code: the sweep opens, the stamp arms.
- **The ring was never the bottleneck — the response was.** Watches were
  already firing before this work; the poller answered a ring with the
  full pass (wallet scan included), so ring→message equalled pass
  duration (85–105 s measured). The fast lane answers with a targeted
  `pollContact` instead: **ring→message 1.2 s measured** on the phone.
- **Changed-keys arrive with duplicates** (one per touched subkey) — the
  fast lane sets-dedupes before deciding it handled everything; when it
  did, the heavy pass waits for its heartbeat (the battery win).


## Stage B landed (2026-08-31 evening): the lane is a thread

The fast lane inside the sweep loop had a ceiling: it could not preempt
a running pass, so a ring during a slow sweep still waited for the
sweep (53 s measured). The lane is now its own thread — the process's
single `node_wait_change` consumer — and the sweep is a plain timer
(10 s chunks foreground, 3 min heartbeat pocketed), demoted to what it
always claimed to be: the correctness pass behind the push. Concurrent
readers of one contact are serialized in Mailbox with per-contact locks
and freshest-counters (unserialized they double-append; serialized but
stale they re-read).

Measured, writer ticking a real phone: **[5.2, 0.2, 0.5, 0.1, 0.4,
0.1, 0.4, 0.0] seconds** — first tick raced the boot sweep, everything
after arrived sub-second, foreground and background identical, the lane
answering each ring in 200–580 ms. The morning's baseline on this exact
path was 85–105 s. Watches held through a 20-minute quiet gap
(re-stamped each pass; the one-shot desk watcher stayed dead all run —
WT_STALL to the end).

Incoming calls now ride the same lane: the offer lands in ~1 s, the
thread bumps, the shell notices, the bell rings. Stage C (control
frames on the call route) is what remains of the telephone's v2.
