# veilid-core, vendored

This is veilid-core 0.5.7 as published on crates.io (MPL-2.0, see
LICENSE.md), carried here for one change:

- `src/rpc_processor/fanout/fanout_queue.rs`: `FanoutNodeStatus::transition`
  no longer clones a node's whole transition history — a boxed linked list
  with derived, recursive `Clone` and `Drop` — and bounds that history to
  the newest eight transitions plus the oldest. A node touched thousands of
  times over a long-lived fanout queue cost quadratic time and eventually
  the stack; the DUCAT desk, which runs for hours, died in
  `FanoutNodeStatus::clone` (SIGSEGV, 2026-09-05).

- `build.rs`: the x86_64-Android fix-up looked for `libclang_rt.builtins`
  under one hard-coded NDK version (28.2); it now globs every installed NDK,
  so a build box with only the project's pinned NDK (27.2) can build the
  emulator ABI.

The root workspace and the desk's own workspace point `veilid-core` here
through `[patch.crates-io]`. Bump the version in lockstep with upstream
and re-apply the change when moving to a newer veilid.
