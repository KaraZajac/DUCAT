# Offered upstream (2026-08-30)

The vendored swarm engine (mobile/vendor/, see STIGMERGE-NOTICE.md) carries
six local modifications. Three are BLAKE3/DUCAT-specific and stay ours; the
three generally useful ones have been offered back to cmars, ported onto the
exact upstream bases we vendored from (stigmerge 8f26b50 = their HEAD at
time of offer; veilnet 0.4.5 = same), each passing the upstream test suite
with upstream's own SHA-256 semantics untouched.

## Sent (GitHub, as KaraZajac)

- **stigmerge PR #440** — `fetcher: send Status::Done when the index
  completes`. The one-line completion-signal fix (our mod #6): upstream
  signals `Done` only on the nothing-to-fetch path, so status-channel
  consumers hang on every real download.
  https://github.com/cmars/stigmerge/pull/440

- **stigmerge PR #441** — `share_announcer: report allocated routes to an
  optional observer` (our mod #4, `route_registry`). What an embedder
  sharing one Veilid node between the swarm and its own protocol needs to
  demultiplex AppCalls. No behaviour change without an observer installed.
  https://github.com/cmars/stigmerge/pull/441

## Prepared, awaiting a Codeberg account to send (veilnet is not on GitHub)

- **veilnet-from-api.patch** (in this directory) — `connection: from_api,
  for riding an already-running node` (our mod #3, minus the vendor-notice
  comments). Branch also lives in the working clone's
  `feat/connection-from-api`. To send: either open a PR on
  https://codeberg.org/cmars/veilnet from a Codeberg fork, or email/attach
  the patch — `git am veilnet-from-api.patch` applies it on 0.4.5.
  `cargo test` passes on the result.

If cmars takes #440/#441 in some reshaped form, re-sync the vendored copies
to match the upstream shape where it does not fight the BLAKE3 divergence —
fewer deltas to carry is the whole point of offering them.
