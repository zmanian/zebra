# Primitive Verifier Failure Taxonomy Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual of #1186/#10559. Do not post publicly without
explicit re-authorization.

Scope: follow-up on pass-5 workstream A7, covering proof/signature batch
verifier failure semantics in `zebra-consensus/src/primitives/`,
`tower-batch-control`, `tower-fallback`, and transaction error mapping.

## Summary

No attacker-reachable path was found where a malformed proof/signature, dropped
batch channel, worker failure, or verifier shutdown is reported as consensus
acceptance.

The reviewed primitive verifier stack generally fails closed:

- invalid batch verification returns an error to the item future,
- `tower_fallback` retries the same item through single-item verification,
- invalid single-item verification returns an error,
- `AsyncChecks::check()` rejects the transaction on the first failed async
  check.

The remaining issue is failure taxonomy and panic-hardening, not invalid
transaction acceptance. Several primitive verifier errors are boxed before they
reach `TransactionError::from(BoxError)`, and that downcast path only recognizes
`redjubjub::Error`, `ValidateContextError`, and an already-boxed
`TransactionError`. As a result, some consensus-invalid proof/signature failures
surface as `InternalDowncastError` instead of stable `TransactionError` variants.
Some watch-channel receive failures also still panic with "verifier was dropped
without flushing" rather than returning an infrastructure error.

There is also a diagnostics stability issue: `AsyncChecks` uses
`FuturesUnordered` and returns the first completed error. If an attacker submits
a transaction with multiple independent async failures, the top-level
`TransactionError` can depend on which verifier future completes first.

One extra observability caveat: the primary batch item futures emit
`invalid`/`verified` counters before `tower_fallback` retries a failed batch
item individually. In a mixed batch, a single invalid item can therefore cause
valid neighbors to be briefly counted as invalid by the primary batch path, even
though fallback then accepts those valid items. This is metrics drift, not
consensus rejection.

The same mixed-batch shape is also a bounded availability amplifier: one invalid
item can make unrelated valid items in the same global primitive batch pay the
cost of single-item fallback verification. Because the global verifier services
are shared by block, mempool, and RPC-submitted transaction verification paths,
this can add latency and CPU work to unrelated contemporaneous traffic. I did
not find a false-accept, false-reject, or hang path from that fallback behavior.

## Evidence

### Batch and fallback behavior

`tower-fallback` retries a request on the fallback service when the primary
batch service future returns `Err`:

- `tower-fallback/src/future.rs:86-98`
- `tower-fallback/src/future.rs:99-115`

For Ed25519, RedJubjub, RedPallas, and Halo2, the primary service is
`Batch<Verifier, Item>` and the fallback service is a single-item verifier:

- `zebra-consensus/src/primitives/ed25519.rs:69-95`
- `zebra-consensus/src/primitives/redjubjub.rs:64-90`
- `zebra-consensus/src/primitives/redpallas.rs:82-108`
- `zebra-consensus/src/primitives/halo2.rs:133-160`

This means a batch-level invalid result does not by itself accept or reject the
transaction. It causes per-item single verification, which is the expected
failure-localization behavior.

Sapling follows the same high-level shape through a `Fallback<Batch<Verifier,
Item>, verify_single>`:

- `zebra-consensus/src/primitives/sapling.rs:200-218`

Sprout Groth16 does not currently use batch verification in production; the
global service is already single-item verification:

- `zebra-consensus/src/primitives/groth16.rs:76-102`

### Worker/channel failures

The shared Rayon helper sends the closure result through a oneshot channel:

- `zebra-consensus/src/primitives.rs:37-49`

If the Rayon closure panics before sending, the receiver returns an error. The
`spawn_fifo_and_convert()` wrapper maps that to a boxed infrastructure string,
not success:

- `zebra-consensus/src/primitives.rs:22-34`

Ed25519/RedJubjub/RedPallas/Halo2 batch flushes convert a failed `spawn_fifo()`
receive into `None` on the watch channel:

- `zebra-consensus/src/primitives/ed25519.rs:150-168`
- `zebra-consensus/src/primitives/redjubjub.rs:145-162`
- `zebra-consensus/src/primitives/redpallas.rs:163-180`
- `zebra-consensus/src/primitives/halo2.rs:222-239`

The item futures treat `None` as an error, not acceptance:

- `zebra-consensus/src/primitives/ed25519.rs:193-210`
- `zebra-consensus/src/primitives/redjubjub.rs:188-206`
- `zebra-consensus/src/primitives/redpallas.rs:205-223`
- `zebra-consensus/src/primitives/halo2.rs:283-303`

Sapling's `spawn_blocking()` flush explicitly sends `None` when the blocking
task joins with an error, then returns the join error to the batch worker:

- `zebra-consensus/src/primitives/sapling.rs:141-165`

The transaction verifier waits on all async checks and returns on the first
error:

- `zebra-consensus/src/transaction.rs:1209-1222`

So these failure modes fail closed, though not always with good taxonomy.

### Metrics emitted before fallback

The primary batch item futures increment success/failure counters before
returning their result to `tower_fallback`:

- `zebra-consensus/src/primitives/ed25519.rs:193-210`
- `zebra-consensus/src/primitives/redjubjub.rs:188-206`
- `zebra-consensus/src/primitives/redpallas.rs:205-223`
- `zebra-consensus/src/primitives/halo2.rs:283-303`
- `zebra-consensus/src/primitives/sapling.rs:116-132`

For batch verifiers that return one result for the whole batch, a mixed
valid/invalid batch can make every item future on the failed primary batch emit
an invalid metric. The fallback retry still verifies each cloned request
individually:

- `tower-fallback/src/service.rs:53-56`
- `tower-fallback/src/future.rs:86-115`

So the behavioral safety property holds, but invalid counters can overstate the
number of invalid proofs or signatures when attackers deliberately mix invalid
items into otherwise valid contemporaneous verification traffic.

### Fallback CPU and latency amplification

The global primitive services are shared by block and mempool transaction
verification. The block verifier sends transactions into the transaction
verifier and fails the block on any transaction error, while mempool/RPC
submission converges through the same transaction verifier path:

- `zebra-consensus/src/block.rs`
- `zebra-consensus/src/transaction.rs:1045-1084`
- `zebra-consensus/src/transaction.rs:1091-1150`
- `zebra-consensus/src/transaction.rs:1155-1182`
- `zebrad/src/components/mempool/downloads.rs`

When an invalid primitive item causes a primary batch failure, fallback protects
correctness by retrying each item individually. That also means all valid items
that happened to share the failed batch do extra cryptographic work. The impact
is bounded by batch size and normal verifier concurrency, but it is a real
latency/CPU amplification surface for invalid mempool or RPC-submitted
transactions.

The mempool download stream also treats any spawned verifier task panic as a
process panic:

- `zebrad/src/components/mempool/downloads.rs:215-217`

I did not find an ordinary malformed transaction path that triggers the
primitive watch-channel panic branches, so this is panic-containment hardening
rather than a confirmed crash exploit. It is still worth changing those
receiver panics and the mempool `expect` into ordinary infrastructure errors.

### Error taxonomy gap

The transaction verifier converts boxed primitive errors through
`TransactionError::from(BoxError)`:

- `zebra-consensus/src/error.rs:234-257`

That conversion currently recognizes:

- `zebra_chain::primitives::redjubjub::Error`,
- `ValidateContextError`,
- boxed `TransactionError`.

It does not recognize several errors that can come back from async primitive
checks, including:

- `zebra_chain::primitives::ed25519::Error`,
- `zebra_chain::primitives::reddsa::Error` for RedPallas,
- `zebra_script::Error` from script verification,
- Sapling bundle/proof/signature errors returned as strings,
- Halo2 proof errors returned as strings.

Existing tests already document some of this behavior:

- script failures are currently expected as `InternalDowncastError` in
  `zebra-consensus/src/transaction/tests.rs:1829-1835` and
  `zebra-consensus/src/transaction/tests.rs:2500-2506`,
- an invalid Sprout JoinSplit signature is expected as
  `InternalDowncastError` in
  `zebra-consensus/src/transaction/tests.rs:2607-2618`,
- the same test has a TODO showing the desired stable
  `TransactionError::Ed25519(...)` shape.

This is not acceptance of invalid transactions, but it can make consensus-invalid
input look like an internal verifier failure, reduce mempool misbehavior scoring,
and make logs/RPC-facing diagnostics less precise.

### Error-order stability gap

`AsyncChecks::check()` waits on a `FuturesUnordered` and immediately returns the
first failed check:

- `zebra-consensus/src/transaction.rs:1209-1222`

That is correct for fail-closed validation, but it means transactions with
multiple independent async failures can produce race-dependent top-level errors.
The modified JoinSplit test already accounts for this by looping around
proof/signature corruption and accepting whichever async failure wins first:

- `zebra-consensus/src/transaction/tests.rs:2607-2631`

This does not create acceptance, but it makes peer scoring and incident
diagnostics less stable. A future hardening pass could preserve the current
short-circuit behavior while tests assert an allowed typed-error set rather than
one exact winner.

## Eliminated Hypotheses

### Invalid batch result accepted

Eliminated. Batch invalidity is represented as `Err` or `false`, then
`tower_fallback` retries individual verification. The transaction is only
accepted if individual verification succeeds.

### Dropped worker response accepted

Eliminated. A dropped oneshot/watch sender is represented as `Err` or `None`,
and item futures map that to an error.

### Cancelling one transaction check poisons unrelated queued checks

No acceptance path found. `AsyncChecks::check()` returns early and drops
remaining local futures on the first error, but the underlying batch still sends
its result to any remaining receivers. Dropping one receiver does not make the
shared watch sender publish success.

### One invalid item rejects unrelated valid items

Eliminated for current production verifiers. Ed25519, RedJubjub, RedPallas,
Sapling, and Halo2 all wrap their primary batch services in `tower_fallback`.
The fallback service receives a clone of the original request and re-runs
single-item verification after a primary batch error, so unrelated valid items
are not finally rejected just because they shared a failed primary batch.

### Groth16 batch fallback bypass

Eliminated for current production code. `JOINSPLIT_VERIFIER` is single-item
verification, not an active batch verifier.

## Hardening Recommendations

1. Add stable `TransactionError` variants or downcasts for every primitive error
   type that crosses `AsyncChecks` through `BoxError`.
2. Keep verifier infrastructure failures distinct from consensus-invalid proofs
   and signatures. A worker panic or dropped channel should not become
   `Groth16("...")`, `InternalDowncastError("InvalidSignature")`, or another
   consensus-shaped error.
3. Replace watch-channel receive panics in Ed25519, RedJubjub, RedPallas, and
   Halo2 with explicit infrastructure errors:
   - `zebra-consensus/src/primitives/ed25519.rs:210`
   - `zebra-consensus/src/primitives/redjubjub.rs:206`
   - `zebra-consensus/src/primitives/redpallas.rs:223`
   - `zebra-consensus/src/primitives/halo2.rs:303`
4. Add tests that force fallback paths and verify exact error taxonomy for:
   - invalid Ed25519 JoinSplit signature,
   - invalid RedJubjub Sapling binding/spend signature,
   - invalid RedPallas Orchard binding/spend signature,
   - invalid Halo2 proof,
   - simulated worker response-channel failure.
5. Move primitive verifier validity counters after fallback, or add distinct
   primary-batch failure metrics so invalid counters reflect final per-item
   verification outcomes.
6. Consider per-source batching isolation, rate limiting, or metrics for
   fallback storms so invalid mempool/RPC traffic cannot silently add latency to
   unrelated block verification work.
7. Convert mempool verifier-task join panics into structured verifier
   infrastructure errors instead of crashing the download stream.

## Disclosure Triage

Public hardening.

This does not appear to warrant private disclosure on its own because no invalid
transaction acceptance path was found. It is still worth fixing because it sits
on consensus-critical verification plumbing and affects peer scoring,
observability, and future incident diagnosis.

## Verification

Focused test run:

```sh
cargo test -p zebra-consensus v4_with_modified_joinsplit_is_rejected --lib
```

Result on 2026-05-09: passed. This confirms the representative modified
JoinSplit path is rejected; the remaining note is about taxonomy, panic
containment, metrics, and latency, not invalid acceptance.

## Duplicate Check

Read-only duplicate searches refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra primitive verifier InternalDowncastError fallback batch metrics'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra verifier was dropped without flushing primitive'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra transaction verifier error taxonomy InternalDowncastError'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "InternalDowncastError" "TransactionError"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "batch" "fallback" "invalid" "metrics" "verifier"'
gh api repos/ZcashFoundation/zebra/issues/1186
gh api repos/ZcashFoundation/zebra/issues/10559
```

Closest hits:

- #1186 is an older closed broad cleanup issue for verification error type
  deduplication and removing `InternalDowncastError`.
- #10559 publicly covers the sharper mempool consequence where infrastructure
  errors can be cached as exact-tip rejections.
- No exact public tracker was found for primitive batch fallback metrics drift,
  watch-channel panic containment, or the full async primitive error taxonomy.

## Confidence

Confidence: medium-high for "no invalid acceptance found in this surface";
medium for the full hardening scope, because synthetic worker-panic tests were
not added in this pass.
