# The route-keepalive bill (research, 2026-08-31)

What the +28% CPU on an idle record-writing phone actually buys, read
from veilid-core 0.5.7 rather than guessed.

## The machinery

`routing_table/routing_domains/public_internet/controller/private_route_management.rs`:

- veilid maintains **4 background safety routes** (minimum 2) per safe
  default length — these are what our sends hide behind.
- Every allocated route gets a **loopback keepalive each 10 s per
  ordering** (`SR_LOOPBACK_KEEPALIVE_INTERVAL`), *suppressed* when real
  traffic or a recent test already validated the route inside the
  window. An idle phone therefore pays the full cadence; a busy one
  pays less for the same privacy — the bill is highest exactly when
  the phone looks most idle.
- **Published routes are always in the must-test set** (`is_published()
  || never-validated ⇒ test`). Every reach-me code and outstanding
  card whose route sits in a DHT record is a standing test obligation
  until it is pruned.

All three numbers are `const`, not config: there is no knob an
embedder can turn in 0.5.7.

## Levers, in order of preference

1. **Hold fewer published routes** (app-side, available now). The
   sweep already prunes spent cards (§18.7); the remaining standing
   cost is long-lived reach-me/profile codes. Worth auditing how many
   routes a settled phone actually publishes — each one is a 10 s-cadence
   test forever.
2. **Ride the suppression** (free). Real traffic validates routes, so
   the push lane's chatter and any active call already displace
   keepalives one-for-one. The idle-phone bill is the whole bill.
3. **Upstream conversation** (kara's, if wanted): a configurable
   keepalive interval — 10 s is tuned for responsiveness on servers;
   a pocketed phone would take 30–60 s on background routes happily.
   Not our PR to open (2026-08-30 rule); an issue from kara's own
   hand, or nothing.
4. **Vendor patch** (last resort): the constants are local-cadence
   only — wire-compatible to change — but forking veilid-core for two
   numbers is maintenance debt out of proportion to half a core on an
   idle emulator. Declined for now.

Verdict: no code change. Lever 1 gets an audit when reach-me codes
next get attention; lever 2 is already how the app behaves since the
push work landed.
