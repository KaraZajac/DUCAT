# Vendored deltas, documented (final status 2026-08-30)

The vendored swarm engine (mobile/vendor/, see STIGMERGE-NOTICE.md)
carries six local modifications. Three are BLAKE3/DUCAT-specific; the
three generally useful ones are preserved here as tested patch files,
each ported onto the exact upstream base we vendored from (stigmerge
8f26b50, veilnet 0.4.5) with upstream's SHA-256 semantics untouched.

**These are documentation of what we carry, not offers in flight.** Two
were briefly opened as PRs on cmars/stigmerge on 2026-08-30 without
kara's authorization; cmars closed both, uninterested in auto-generated
PRs, and the standing rule since is: no contributions to other people's
repos, ever. We maintain these deltas ourselves and expect no upstream
sync. If upstream independently fixes the same things, prefer their
shape on the next vendor refresh where it does not fight the BLAKE3
divergence.

## The patches (in this directory)

- **stigmerge-status-done.patch** — the fetcher returns `State::Done`
  without ever sending `Status::Done` on a real download (only the
  nothing-to-fetch path signals), so status-channel consumers hang.
  One line; our mod #6.

- **stigmerge-route-observer.patch** — `route_registry`: the announcer
  reports every private route it creates or retires, which is what an
  embedder sharing one Veilid node between the swarm and its own
  protocol needs to demultiplex AppCalls. Our mod #4.

- **veilnet-from-api.patch** — `Connection::from_api`: ride an
  already-running node via a returned feeder closure, `owned: false`
  making reset/close safe on a borrowed node. Our mod #3.

All three passed the relevant upstream test suite at preparation time
(`cargo test -p stigmerge_peer` / `cargo test` on veilnet 0.4.5).
