# Mempool V5 same-effects pending amplification note

Date: 2026-05-04

Last updated: 2026-05-08

Disposition: public issue filed after explicit re-authorization on 2026-05-08.
Issue: https://github.com/ZcashFoundation/zebra/issues/10565

## Summary

Zebra's mempool storage suppresses already stored or rejected transactions using
the right identity for each rejection class, but the download/verify queue only
deduplicates in-flight work by exact `UnminedTxId`.

For V5 transactions, exact unmined identity is `WtxId`, which is the mined
transaction ID plus the authorization digest. This means multiple V5 transaction
variants with the same mined ID but different authorization digests can occupy
multiple inbound mempool download/verify slots before any one of them is inserted
or rejected.

This is not a consensus issue. It is bounded mempool availability hardening: the
main bound is `MAX_INBOUND_CONCURRENCY = 25`, but a peer can use distinct
same-effects V5 witnessed IDs to consume several of those slots at once.

## Finding

`Request::Queue` checks storage before each queue admission:

- `zebrad/src/components/mempool.rs:861-887`
- `zebrad/src/components/mempool/storage.rs:915-928`

That storage check suppresses a transaction if the same mined ID is already in
the mempool, or if a matching rejection cache entry exists. But it only checks
settled storage and rejection state; it does not record a same-mined-ID pending
reservation while the transaction is being downloaded or verified.

The downloader's in-flight deduplication is exact-ID based:

- `zebrad/src/components/mempool/downloads.rs:177-179` stores
  `cancel_handles: HashMap<UnminedTxId, ...>`.
- `zebrad/src/components/mempool/downloads.rs:276-307` rejects only when
  `cancel_handles.contains_key(&txid)` or when `pending.len()` reaches
  `MAX_INBOUND_CONCURRENCY`.
- `zebrad/src/components/mempool/downloads.rs:456-460` inserts the cancel handle
  under the exact requested `UnminedTxId`.

For V5, distinct exact IDs can share one mined ID:

- `zebra-chain/src/transaction/hash.rs:186-203` defines `WtxId` as `{ id,
  auth_digest }`.
- `zebra-chain/src/transaction/unmined.rs:93-108` uses
  `UnminedTxId::Witnessed(WtxId)` for V5 unmined transactions.
- `zebra-chain/src/transaction/unmined.rs:182-195` says `mined_id()` returns
  the V5 transaction's effects ID, omitting the authorization digest.

So, before any same-effects variant completes, this sequence is possible:

1. The peer advertises or pushes `WtxId(mined_id = M, auth_digest = A)`.
2. Storage has no settled transaction or rejection for that exact witnessed ID,
   so it queues.
3. The peer advertises or pushes `WtxId(mined_id = M, auth_digest = B)`.
4. `cancel_handles` does not contain the second exact ID, and no settled storage
   result exists yet, so it also queues.
5. The process repeats until the 25-task inbound queue cap is reached.

Once a same-effects variant is accepted, later queue attempts for that mined ID
are blocked by `Storage::should_download_or_verify()` because the verified set
is keyed by mined ID. But the already-admitted concurrent work still runs.

If a variant fails verifier consensus validation, Zebra records an exact-tip
rejection:

- `zebrad/src/components/mempool/storage.rs:56-65` defines
  `ExactTipRejectionError` for authorizing-data failures.
- `zebrad/src/components/mempool/storage.rs:182-187` documents that exact-tip
  rejections apply only to the exact `UnminedTxId`.
- `zebrad/src/components/mempool/storage.rs:843-869` stores
  `TransactionDownloadVerifyError::Invalid` as
  `ExactTipRejectionError::FailedVerification`.

That includes authorizing-data failures, but it is not limited to them. This is
conservative for same-effects policy because another authorization for the same
effects might be valid, and because exact-tip rejections are separate from
same-effects chain rejections. But it also means same-effects V5 variants that
differ only in authorization digest can continue to require verification work
unless there is an in-flight same-mined-ID throttle.

## Reachable paths

Two remote paths can feed this shape:

- An unsolicited `tx` message becomes `Request::PushTransaction`, and inbound
  forwards it to `mempool::Request::Queue(vec![transaction.into()])`
  (`zebra-network/src/peer/connection.rs:1278` and
  `zebrad/src/components/inbound.rs:526-532`).
- An `inv` / crawler transaction ID path forwards `UnminedTxId`s to
  `Request::Queue` (`zebra-network/src/peer/connection.rs:1279-1295`,
  `zebrad/src/components/inbound.rs:534-540`, and
  `zebrad/src/components/mempool/crawler.rs:246-277`).

Normal P2P transaction responses do exact-match the requested ID before returning
available transactions to the downloader:

- `zebra-network/src/peer/connection.rs:190-239` only pushes a received
  transaction into the response if `pending_ids.remove(&transaction.id)`
  succeeds.

So this is not a "peer returns the wrong WTXID and leaves stale downloader
state" finding on the normal peer-service path. The issue is the absence of a
same-mined-ID pending reservation while exact witnessed IDs are in flight.

## Local proof

A focused downloader test now confirms the in-flight admission behavior:

- `zebrad/src/components/mempool/downloads.rs::same_mined_id_v5_wtxids_queue_separately_today`

The test constructs `MAX_INBOUND_CONCURRENCY` synthetic
`UnminedTxId::Witnessed(WtxId)` values with the same mined transaction ID and
different authorization digests. With the network, verifier, and state services
held pending, all same-effects witnessed IDs are admitted and occupy all inbound
download slots. Re-queueing the exact first WTXID returns `AlreadyQueued`, while
the next distinct same-effects WTXID returns `FullQueue`.

Command:

```sh
cargo test -p zebrad same_mined_id_v5_wtxids_queue_separately_today --lib
```

Result on 2026-05-07: passed.

## Duplicate Check

Read-only duplicate search performed on 2026-05-07:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra V5 WtxId same mined id pending mempool'
```

No issue hits were returned.

Independent cross-check:

- RepoPrompt builder chat `v5-queue-dedup-7CD08E` independently validated the
  in-flight exact-ID admission behavior and the bounded public-hardening
  severity. It also recommended correcting the rejection wording above so it
  matches `Storage::reject_if_needed()`: all verifier `Invalid` results are
  cached as exact-tip rejections today, not just authorizing-data failures.

## Impact

The impact is bounded but real:

- a single peer can consume up to 25 inbound mempool download/verify slots with
  same-effects V5 variants;
- invalid variants are cached by exact `WtxId` under exact-tip rejection
  semantics, so a different auth digest for the same mined ID can still be
  queued and verified;
- this can amplify verification work and delay unrelated transaction processing
  while the queue is full.

Important limits:

- the queue cap is small (`MAX_INBOUND_CONCURRENCY = 25`);
- successfully accepted same-effects variants block later variants by mined ID;
- rejection lists are memory-limited and tip-scoped;
- this does not make Zebra accept invalid transactions or blocks.

## Suggested fix direction

Keep exact `UnminedTxId` tracking for cancellation and protocol exactness, but
add a small same-effects pending guard:

- track in-flight mined IDs alongside exact `cancel_handles`;
- reject or defer additional V5 transactions with the same `mined_id()` while
  one same-effects candidate is pending;
- remove the mined-ID pending entry on every terminal task path, including
  success, normal error, cancellation, and timeout;
- preserve exact-tip rejection semantics for authorization failures, because a
  different authorization for the same effects might be valid.

Regression coverage should include:

- two V5 `UnminedTxId::Witnessed` values with the same mined ID but different
  auth digests;
- queueing the first succeeds;
- queueing the second while the first is pending returns a bounded/deferred
  result;
- after the first task terminates, the mined-ID pending entry is cleaned up.

## Disclosure triage

Public hardening / low-to-medium availability.

This is attacker-influenced and worth fixing, but it is bounded by the 25-task
queue limit and affects mempool liveness rather than consensus correctness,
funds, or persistent chain state.

Confidence: high on the downloader admission behavior after the local proof.
Confidence remains lower on practical exploitability because it depends on how
cheaply an attacker can produce many V5 transactions with the same effects ID and
distinct authorization digests that reach the verifier.
