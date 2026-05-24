# Mempool Downloader Timeout Cancel-Handle Retention Finding

Date: 2026-05-03

Status: submitted as GitHub Security Advisory private report
GHSA-89mr-m7gq-cxjm on 2026-05-07.

Scope: mempool transaction download and verification task lifecycle in
`zebrad/src/components/mempool/downloads.rs`, with follow-up reachability through
P2P transaction push/advertisement and mempool dependency verification.

## Summary

The mempool transaction downloader removes a transaction's cancel handle when a
download/verify task succeeds or returns a normal error, but not when the task
hits the downloader's outer timeout. After such a timeout, the active task has
left `pending`, but the transaction remains in `cancel_handles`.

That breaks the downloader's intended in-flight accounting: timed-out
transactions can accumulate stale dedup entries over time, future attempts for
the same `UnminedTxId` return `AlreadyQueued`, and full pushed transactions can
remain retained through the stored `Gossip::Tx`.

Triage: private maintainer heads-up candidate. The source bug, sequential
retention behavior, and full pushed-transaction retention behavior are
confirmed. Remote exploitability is plausible, but the most obvious
missing-mempool-output path is partially bounded by an inner 60-second mempool
lookup timeout, so practical severity depends on how easily remote peers can
push verification/state work past the 73-second outer timeout.

## Finding

`Downloads` tracks active transaction work in two structures:

- `pending`, a `FuturesUnordered` of spawned download/verify tasks;
- `cancel_handles`, keyed by `UnminedTxId`, storing the cancellation sender and
  the original `Gossip` request.

The cancel-handle map stores the original request:

- `zebrad/src/components/mempool/downloads.rs:177-179`

Admission rejects a transaction as already queued when that map contains the
txid, and rejects new work only when `pending.len()` reaches
`MAX_INBOUND_CONCURRENCY`:

- `zebrad/src/components/mempool/downloads.rs:276-323`

The spawned task wraps the whole download/verify future in an outer
`tokio::time::timeout(RATE_LIMIT_DELAY, fut)`:

- `zebrad/src/components/mempool/downloads.rs:412-450`

On success and normal error, `Downloads::poll_next()` removes the map entry. On
outer timeout, it returns `Err(elapsed)` without removing anything:

- `zebrad/src/components/mempool/downloads.rs:215-228`

The consumer cannot repair this because the timeout item does not include a
transaction ID. `Mempool::poll_ready()` logs that no specific transaction ID is
available:

- `zebrad/src/components/mempool.rs:663-671`

So after an outer timeout:

```text
pending.len() decreases,
cancel_handles[txid] remains,
same txid is permanently treated as AlreadyQueued,
unique later txids can still be admitted because pending is below the cap.
```

## Remote-Reachability Notes

The direct transaction path can store full transaction contents in the retained
request:

- a peer `tx` message becomes `Request::PushTransaction(transaction.clone())`
  (`zebra-network/src/peer/connection.rs:1278`);
- inbound forwards it to `mempool::Request::Queue(vec![transaction.into()])`
  (`zebrad/src/components/inbound.rs:526-530`);
- `Gossip::Tx` holds an `UnminedTx`
  (`zebra-node-services/src/mempool/gossip.rs:7-12`);
- `download_if_needed_and_verify()` clones and inserts that `Gossip` into
  `cancel_handles`
  (`zebrad/src/components/mempool/downloads.rs:456-460`).

The advertised-ID path is also externally reachable:

- a peer `inv` containing transaction inventory becomes
  `Request::AdvertiseTransactionIds(...)`
  (`zebra-network/src/peer/connection.rs:1278-1295`);
- inbound forwards those IDs to `mempool::Request::Queue(...)`
  (`zebrad/src/components/inbound.rs:534-540`);
- `Downloads` stores the corresponding `Gossip::Id` in the same
  `cancel_handles` map.

Default exposure is gated by mempool activation: when the mempool is disabled,
`Request::Queue` returns per-transaction `MempoolError::Disabled` responses and
does not create a downloader queue
(`zebrad/src/components/mempool.rs:965-1008`). The issue is reachable once Zebra
is close enough to the tip for the mempool to be enabled.

A plausible remote timeout shape is a transaction with many transparent inputs
that are not found in the best chain. Mempool transaction verification performs
one best-chain UTXO lookup per transparent input, records missing outpoints as
potential mempool dependencies, then waits on
`mempool::Request::AwaitOutput(outpoint)`:

- `zebra-consensus/src/transaction.rs:700-714`
- `zebra-consensus/src/transaction.rs:736-750`

Important caveat: a single missing mempool output wait should normally return a
regular `TransparentInputNotFound` error before the downloader's outer timeout,
because the transaction verifier wraps `AwaitOutput` calls in
`MEMPOOL_OUTPUT_LOOKUP_TIMEOUT = 60s`
(`zebra-consensus/src/transaction.rs:64-71`), while the downloader's outer
timeout is `RATE_LIMIT_DELAY = 73s`
(`zebrad/src/components/mempool/crawler.rs:80-84`). That normal error path
removes the cancel handle. So the missing-output case becomes interesting when
earlier state lookup work, queueing, or verifier/state load consumes enough time
that the 73-second outer timeout wins first.

The broader timeout stack makes that condition plausible under load. The
mempool downloader wraps the transaction verifier service with
`TRANSACTION_VERIFY_TIMEOUT = BLOCK_VERIFY_TIMEOUT = 8 minutes`
(`zebrad/src/components/mempool/downloads.rs:75-80`,
`zebrad/src/components/sync.rs:173`), which is much longer than the downloader's
73-second outer timeout. The state service passed to the downloader is not
wrapped in a shorter `Timeout` at construction
(`zebrad/src/components/mempool.rs:364-367`). Therefore slow or backlogged
best-chain UTXO state lookups before the 60-second mempool dependency wait can
let the downloader's outer timeout win first, leaving the stale cancel handle.

This is plausible because the verifier does its best-chain UTXO checks
sequentially over transaction inputs before the mempool-output wait:

- quick checks run before state UTXO loading
  (`zebra-consensus/src/transaction.rs:404-439`);
- UTXOs are loaded after those checks
  (`zebra-consensus/src/transaction.rs:469-473`);
- each transparent input issues `UnspentBestChainUtxo` before missing inputs are
  collected for mempool dependency waits
  (`zebra-consensus/src/transaction.rs:694-715`);
- transactions can contain many transparent inputs under the 2 MB transaction
  limit, with `MIN_TRANSPARENT_INPUT_SIZE = 41`
  (`zebra-chain/src/transaction/serialize.rs:1136-1139`) and transaction
  deserialization capped by `MAX_BLOCK_BYTES`
  (`zebra-chain/src/transaction/serialize.rs:770-783`).

The normal silent-peer download path is less concerning because the network
service passed into the downloader is separately wrapped in
`TRANSACTION_DOWNLOAD_TIMEOUT = 20s`, which is shorter than the outer timeout:

- `zebrad/src/components/mempool.rs:364-367`
- `zebrad/src/components/sync.rs:139`

But the direct pushed-transaction path bypasses transaction download and can
retain a full `Gossip::Tx`. The serialized transaction is bounded by the 2 MB
block-size cap during deserialization, but the deserialized malicious
transaction can be materially larger in memory; the downloader comments estimate
that a deserialized transaction near the transparent-output extreme can take
about 9 MB (`zebrad/src/components/mempool/downloads.rs:88-99`).

## Existing Bounds and Reset Paths

The in-flight concurrency cap is real but does not cap stale timeout entries:

- new work is rejected while `pending.len() >= MAX_INBOUND_CONCURRENCY`
  (`zebrad/src/components/mempool/downloads.rs:296-306`);
- after an outer timeout the task leaves `pending`, so new unique txids can be
  admitted even though old `cancel_handles` entries remain
  (`zebrad/src/components/mempool/downloads.rs:215-228`);
- duplicate txids remain permanently `AlreadyQueued` until the downloader is
  dropped or reset (`zebrad/src/components/mempool/downloads.rs:283-294`).

Some events do clear the stale map:

- mempool disable/reset drops the `ActiveState::Enabled` downloader and calls
  `cancel_all()` through `PinnedDrop`
  (`zebrad/src/components/mempool.rs:376-385`,
  `zebrad/src/components/mempool/downloads.rs:545-558`);
- chain fork reset rebuilds the downloader, but it first clones pending and
  stored transaction requests for retry
  (`zebrad/src/components/mempool.rs:555-580`);
- chain growth only cancels txids whose mined ID appears in a new block
  (`zebrad/src/components/mempool.rs:677-685`).

These are situational cleanup paths, not a steady-state bound while a node stays
near the tip and attackers keep using fresh txids.

The queue-checker makes the retention operational even when peers stop sending
follow-up requests: it sends `CheckForVerifiedTransactions` every 5 seconds
(`zebrad/src/components/mempool/queue_checker.rs:22-29`,
`zebrad/src/components/mempool/queue_checker.rs:56-62`), and every mempool
request gives `Mempool::poll_ready()` a chance to drain completed downloader
tasks (`zebrad/src/components/mempool.rs:601-672`). So timed-out tasks are
harvested promptly, freeing `pending` slots while stale handles remain.

## Builder Cross-Check

RepoPrompt builder run: `mempool-timeout-audit-B4C4C3`.

Conclusion matched the local trace:

- confirmed source bug and stale-handle accumulation after an outer timeout;
- confirmed remote reachability to `Downloads` through peer `tx` and `inv`
  messages once the mempool is enabled;
- did not confirm a deterministic content-only remote trigger for the 73-second
  outer timeout;
- classified the issue as a private candidate rather than an eliminated
  hardening item or fully confirmed remote vulnerability.

## Local Backstop

Added paused-time proof tests:

- `zebrad/src/components/mempool/downloads.rs:595-645`
- `zebrad/src/components/mempool/downloads.rs:650-696`
- `zebrad/src/components/mempool/tests/vector.rs`

The first test stalls the downloader's services, advances Tokio time by
`RATE_LIMIT_DELAY`, polls timed-out tasks, and confirms:

- timed-out tasks are gone from `pending`;
- the original transaction request remains in `cancel_handles`;
- requeueing the same txid returns `MempoolError::AlreadyQueued`.
- a second unique txid can be queued after the first timeout;
- sequential timeout paths accumulate stale `cancel_handles`.

The second test queues the direct pushed-transaction path with `Gossip::Tx`,
stalls the services, advances through the outer timeout, and confirms:

- the timed-out task is gone from `pending`;
- `transaction_requests()` still returns the timed-out request;
- that retained request still has `tx().is_some()`, so the stale handle keeps
  the full `UnminedTx`, not merely its ID;
- later queueing of the same txid returns `MempoolError::AlreadyQueued`.

The service-level test
`mempool_timeout_retains_pushed_transaction_request_today` queues a direct
pushed transaction through the normal `Mempool` service, intercepts and holds
the verifier request, advances through the downloader timeout, then makes a
normal mempool request so `poll_ready()` drains the timed-out task. It confirms:

- the timed-out task is gone from `tx_downloads.in_flight()`;
- `tx_downloads.transaction_requests()` still contains the timed-out
  `Gossip::Tx`;
- the retained request still has `tx().is_some()`;
- later queueing of the same txid through `Mempool::call(Request::Queue(...))`
  returns `MempoolError::AlreadyQueued`.

Verification run:

```text
cargo test -p zebrad timed_out_downloads_accumulate_cancel_handles_today --lib
cargo test -p zebrad timed_out_pushed_transaction_retains_full_request_today --lib
cargo test -p zebrad mempool_timeout_retains_pushed_transaction_request_today --lib
```

Result:

```text
test components::mempool::downloads::tests::timed_out_downloads_accumulate_cancel_handles_today ... ok
test components::mempool::downloads::tests::timed_out_pushed_transaction_retains_full_request_today ... ok
test components::mempool::tests::vector::mempool_timeout_retains_pushed_transaction_request_today ... ok
```

I repeated the original focused verification on 2026-05-04 against the current
checkout, added the direct pushed-transaction retention proof, and both
downloader-focused commands passed.

Follow-up on 2026-05-07 added and verified the service-level mempool proof.

## Impact

Potential severity: medium availability, pending remote exploitability
validation.

If a remote peer can repeatedly induce the outer timeout with unique
transactions, the downloader's `MAX_INBOUND_CONCURRENCY = 25` cap only limits
currently running tasks. It does not cap stale retained `cancel_handles` after
timeout. A peer could accumulate:

- one retained map entry per timed-out txid;
- one retained `Gossip::Id` for downloaded-id paths, or a retained full
  `Gossip::Tx` for direct pushed transactions;
- permanent `AlreadyQueued` treatment for the same txid until mempool reset or
  downloader drop.

The invalid or incomplete transaction is not accepted. This is resource
retention and mempool availability degradation, not consensus divergence.

## Suggested Fix Direction

- Change the internal downloader task result so the timeout path carries the
  `UnminedTxId`, for example `Err((txid, Elapsed))` or a small local enum.
- In `Downloads::poll_next()`, remove `cancel_handles[txid]` on every terminal
  path: success, normal error, cancellation, and outer timeout.
- Update `Mempool::poll_ready()` to log the timed-out txid while preserving the
  current policy: timeout is not transaction invalidity, should not enter
  rejection caches, and should not be peer misbehavior by itself.
- Add a post-fix regression where the same txid can be requeued after timeout
  and `transaction_requests()` no longer contains the timed-out request.

## Disclosure Triage

Private maintainer heads-up candidate.

The source-level bug and local proof are high confidence. The reason to keep it
private initially is the plausible remote P2P memory-retention shape using
direct pushed transactions or dependency waits. If follow-up validation shows
the outer timeout cannot be induced by remote peers in a practical default-node
scenario, this can be handled publicly as robustness hardening.

## Confidence

Confidence: high for stale `cancel_handles` after outer timeout.

Confidence: high that the direct pushed-transaction path retains full
transaction contents when the outer timeout wins. This is proven by the
`Gossip::Tx` timeout test and by the source path where
`download_if_needed_and_verify()` clones the original `Gossip` into
`cancel_handles`.

Confidence: medium for default-network exploitability. Direct pushed
transactions and transaction-ID advertisements reach the downloader once the
mempool is enabled. The simple missing-output wait is likely to clean up via its
60-second timeout, but large transparent-input transactions can force many
sequential best-chain UTXO lookups before that wait, and broader verifier/state
load can still make the 73-second outer timeout win. A full adversarial repro
should measure how quickly unique near-2 MB direct-pushed transactions with many
distinct missing or script-failing inputs can accumulate stale entries on a
near-tip mainnet node.

Follow-up validation on 2026-05-04:

- `UnspentBestChainUtxo` requests from mempool verification are redirected to
  `ReadStateService`, then handled on Tokio's blocking pool
  (`zebra-state/src/service.rs:1247-1264`,
  `zebra-state/src/service.rs:1458-1460`,
  `zebra-state/src/service.rs:1725`,
  `zebra-state/src/request.rs:1542-1555`).
- `zebrad` builds Tokio's default multi-thread runtime and does not configure a
  local, mempool-specific blocking-pool bound
  (`zebrad/src/components/tokio.rs:40-43`).
- The content-only missing-output path still usually returns a regular
  `TransparentInputNotFound` before the downloader timeout because
  `MEMPOOL_OUTPUT_LOOKUP_TIMEOUT` is 60 seconds, while `RATE_LIMIT_DELAY` is 73
  seconds (`zebra-consensus/src/transaction.rs:64-71`,
  `zebrad/src/components/mempool/crawler.rs:80-84`).
- Therefore the most defensible phrasing is not "single transaction always
  triggers memory retention." It is "if remotely supplied mempool work can push
  state/verifier progress past the 73-second outer timeout, Zebra leaks stale
  dedup/cancel entries; direct pushed transactions make each stale entry retain
  the full transaction."
