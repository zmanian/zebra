# TrustedChainSync best-tip forwarding note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization and a fresh duplicate check.

## Summary

`TrustedChainSync::spawn()` creates a helper task that subscribes to the upstream
indexer's `ChainTipChange` stream and forwards matching blocks into the mirror's
`LatestChainTip` / `ChainTipChange` channels through a finalized-tip sender. But
the upstream indexer stream is driven by the node's best tip, not by finalized
tip changes. When the streamed hash is a normal non-finalized best tip, the
helper looks for that hash in the mirror's finalized `ZebraDb`, does not find it,
and exits permanently.

The helper task's `JoinHandle` is also discarded, while `spawn()` returns only
the separate non-finalized-state sync task handle. That means callers cannot
supervise, await, or abort the finalized-tip forwarding task through the handle
they receive.

This is not a default-node consensus issue and it is not the previously
documented trusted-sync block/hash validation boundary. It is an opt-in
trusted-mirror robustness issue: a normal or malicious upstream best-tip update
can stop the finalized-tip forwarding task, leaving the mirror dependent on the
separate non-finalized-state sync loop for all later tip updates.

## Evidence

- `zebrad/src/commands/start.rs:277-287` starts the indexer server with
  `latest_chain_tip.clone()`.
- `zebra-state/src/service/chain_tip.rs:303-305` documents `LatestChainTip` as
  the best non-finalized chain tip if available, otherwise the finalized tip.
- `zebra-rpc/src/indexer/methods.rs:36-53` implements `ChainTipChange` by
  awaiting `best_tip_changed()` and sending `best_tip_height_and_hash()`.
- `zebra-rpc/src/sync.rs:62-70` starts a background task in
  `TrustedChainSync::spawn()` and subscribes it to `chain_tip_change(Empty {})`.
- `zebra-rpc/src/sync.rs:62-120` discards that background task's `JoinHandle`,
  while `zebra-rpc/src/sync.rs:123-127` returns only the separate
  `syncer.sync()` task handle.
- `zebra-rpc/src/sync.rs:101-114` decodes the streamed hash, catches the mirror
  DB up with the primary, then calls `db.block(hash.into())`; if the block is not
  in finalized storage, the task returns.
- `zebra-rpc/src/sync.rs:111-112` explicitly says the task exits when the latest
  tip hash is not present in the DB and relies on `TrustedChainSync::sync()` to
  send non-finalized chain-tip updates.
- `zebra-rpc/src/sync.rs:136-212` runs the separate non-finalized-state stream
  sync loop.
- `zebra-rpc/src/sync.rs:258-277` updates mirror channels from the synced
  non-finalized best chain after successful non-finalized commits.

## Duplicate Check

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync BlockAndHash hash mismatch in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync initial_contextual_validity in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'trusted indexer validation boundary in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'TrustedChainSync best tip forwarding in:title,body' --state all --limit 100
```

Result: no hits returned.

Fresh duplicate check on 2026-05-09 reused the validation-boundary searches and
also found no hits for the exact local proof name. No dedicated public issue was
found for the best-tip-forwarder permanent-exit shape.

## Local Proof Status

Runtime proof-backed for the best-tip forwarding task exit as of 2026-05-09.

The new `zebra-rpc/src/sync.rs` test
`trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today` factors the
existing forwarding loop into a private helper, runs a minimal local indexer
gRPC server, creates a real primary/secondary state DB pair so
`spawn_try_catch_up_with_primary()` succeeds, verifies the test hash is absent
from the mirror's finalized storage, sends that hash over `ChainTipChange`, and
awaits the forwarding task. The task exits without panic, proving the current
`db.block(hash.into()) == None => return` branch is reachable in a runtime
setup.

Verification:

```sh
cargo test -p zebra-rpc trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today --lib
```

Result on 2026-05-09: passed.

The adjacent `block_and_hash_decode_accepts_mismatched_hash_today` test covers
the `BlockAndHash` hash/body trust-boundary sub-claim in the
validation-boundary note and passed again on 2026-05-09, but it is separate from
the helper-task permanent-exit shape described here.

## Impact

In a healthy deployment, the separate `NonFinalizedStateChange` stream should
usually compensate by importing the non-finalized block and publishing the
non-finalized best tip. That keeps this from being a strong availability finding.

The fragile part is that one ordinary non-finalized best-tip notification ends
the finalized-tip forwarding task for the lifetime of the mirror syncer. After
that point:

- finalized-tip-only updates are no longer forwarded by this task;
- aborting the `JoinHandle` returned by `TrustedChainSync::spawn()` does not
  directly abort this helper task while it is still alive;
- if the non-finalized stream is delayed, broken, or filtered while
  `ChainTipChange` remains active, mirror tip metadata can become stale;
- a malicious or misconfigured trusted upstream can trigger the permanent exit
  immediately by sending a best-tip hash that is not yet finalized in the mirror.

This does not let an untrusted network peer corrupt a normal Zebra full node. It
affects the optional standalone read-state/indexer mirror path and depends on
trusted infrastructure assumptions.

## Suggested fix direction

- If `ChainTipChange` is intended to represent best-tip updates, do not use it
  as a finalized-tip stream in `TrustedChainSync`; treat missing finalized DB
  blocks as non-finalized and keep the subscription alive.
- If `TrustedChainSync` needs finalized-tip updates specifically, add a distinct
  finalized-tip indexer stream or include a finalized/non-finalized marker in the
  current stream.
- Replace the permanent `return` on `db.block(hash.into()) == None` with a
  recoverable branch that logs, keeps listening, and leaves non-finalized update
  publication to the non-finalized stream.
- Supervise the finalized-tip helper under the same returned task handle as the
  non-finalized sync loop, or return both handles and make shutdown ownership
  explicit.
- Add a mirror-mode regression test where the upstream sends a non-finalized
  best-tip change before later finalized-only changes, and assert the mirror does
  not silently lose tip-change propagation.

## Disclosure triage

Public hardening. The affected path is an opt-in trusted read-state mirror, the
code comments indicate this exit was intentional, and the non-finalized sync loop
often masks the issue. It is still worth fixing or testing because it turns a
normal best-tip event into permanent loss of one of the mirror's two sync
channels.

Confidence: high for the permanent-exit behavior, medium for operational
impact. The code path is now runtime proof-backed; the practical impact depends
on how downstream deployments use
`init_read_state_with_syncer()` and whether they rely on `ChainTipChange` when
the non-finalized stream is unavailable or lagging.
