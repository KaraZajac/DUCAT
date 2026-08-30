# Upstream offers — prepared, NOT sent (status 2026-08-30)

The vendored swarm engine (mobile/vendor/, see STIGMERGE-NOTICE.md) carries
six local modifications. Three are BLAKE3/DUCAT-specific and stay ours; the
three generally useful ones are prepared here as patches, each ported onto
the exact upstream base we vendored from (stigmerge 8f26b50 = upstream HEAD
at preparation time; veilnet 0.4.5 = same) and passing the upstream test
suite with upstream's SHA-256 semantics untouched.

**Nothing goes upstream without kara's explicit go, per patch.** (Two of
these were briefly opened as PRs #440/#441 on cmars/stigmerge on
2026-08-30 without that authorization; both were closed the same day and
the fork branches deleted. GitHub keeps closed PRs visible — only GitHub
support can remove them entirely.)

## The patches (in this directory)

- **stigmerge-status-done.patch** — `fetcher: send Status::Done when the
  index completes` (our mod #6). Upstream signals `Done` only on the
  nothing-to-fetch path, so status-channel consumers hang on every real
  download. One line. `cargo test -p stigmerge_peer` passes.

- **stigmerge-route-observer.patch** — `share_announcer: report allocated
  routes to an optional observer` (our mod #4, `route_registry`). What an
  embedder sharing one Veilid node between the swarm and its own protocol
  needs to demultiplex AppCalls. No behaviour change without an observer
  installed. `cargo test -p stigmerge_peer` passes.

- **veilnet-from-api.patch** — `connection: from_api, for riding an
  already-running node` (our mod #3, vendor-notice comments stripped).
  `cargo test` passes on veilnet 0.4.5 with it applied. veilnet lives on
  Codeberg (https://codeberg.org/cmars/veilnet), not GitHub.

## To send one (kara's call, kara's hands)

    git clone https://github.com/cmars/stigmerge && cd stigmerge
    git am ../research/post-1.0/upstream/stigmerge-status-done.patch
    # fork, push, open the PR — or attach the patch to an issue

Same shape for veilnet on Codeberg. If cmars takes them in some reshaped
form, re-sync the vendored copies to match the upstream shape where it
does not fight the BLAKE3 divergence — fewer deltas to carry is the whole
point of offering them.
