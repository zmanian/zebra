# GetBlockTemplate Long-Poll Full-Mempool Poll Amplification Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Finding

`getblocktemplate` long polling re-runs the full state and mempool template
fetch for each waiting RPC request every five seconds. A caller with mining RPC
access can hold many matching long-poll requests open and make each waiter
periodically clone the full verified mempool transaction set and dependency map.

There is also a sharper mismatch subcase: if the state tip and the mempool's
last-seen tip are out of sync, a long-poll request immediately loops and repeats
the state plus full-mempool request without the five-second polling sleep.

This is not a consensus bug and does not imply invalid block acceptance. It is a
mining RPC availability hardening issue for deployments that expose
`getblocktemplate` to shared or weakly trusted callers.

## Preconditions

- JSON-RPC is enabled and reachable by the caller.
- The caller can authenticate if cookie auth is enabled.
- Mining RPC is enabled by configuring `mining.miner_address`, otherwise
  `getblocktemplate` returns before entering template construction.
- The caller has a current matching `longpollid`.
- The chain tip, template max-time state, and mempool transaction checksum do not
  change, so long-poll requests remain pending.
- Or, for the tighter mismatch subcase, the state tip and mempool last-seen tip
  remain briefly out of sync while a long-poll request is active.
- The server accepts enough concurrent connections or requests for the repeated
  polling cost to matter.

## Evidence

`getblocktemplate` loops while the client long-poll ID still matches the freshly
generated server ID:

- `zebra-rpc/src/methods.rs:2290-2345`

Each loop iteration fetches state chain info and then calls
`fetch_mempool_transactions()`:

- `zebra-rpc/src/methods.rs:2298-2322`
- `zebra-rpc/src/methods/types/get_block_template.rs:756-796`

`fetch_mempool_transactions()` sends `mempool::Request::FullTransactions`:

- `zebra-rpc/src/methods/types/get_block_template.rs:776-783`

The mempool service answers `FullTransactions` by cloning every verified
transaction and cloning the dependency map:

- `zebrad/src/components/mempool.rs:834-848`

If the server and client long-poll IDs match, the RPC sleeps for the mempool
poll interval before checking again:

- `zebra-rpc/src/methods.rs:2363-2372`
- `zebra-rpc/src/methods/types/get_block_template/constants.rs:10-20`

If the mempool response's `last_seen_tip_hash` differs from the state tip,
`fetch_mempool_transactions()` returns `None`, and the long-poll branch uses
`continue` before the sleep is constructed:

- `zebra-rpc/src/methods/types/get_block_template.rs:759-793`
- `zebra-rpc/src/methods.rs:2317-2325`

The RPC server builder does not add an application-level method timeout,
long-poll waiter cap, or shared long-poll fanout cache around this method. Zebra
uses the jsonrpsee server builder and sets middleware plus response-size limits:

- `zebra-rpc/src/server.rs:138-153`

## Impact

Expected impact is bounded but avoidable resource amplification:

- every matching waiter performs an independent full-mempool clone every five
  seconds;
- during state/mempool tip mismatch, each long-poll waiter can repeat the same
  clone path immediately until the snapshots align;
- the cost scales with `active_longpoll_waiters * mempool_size`;
- large mempools make each `FullTransactions` response more expensive;
- the same waiters also occupy long-running RPC futures and count against server
  connection/request capacity.

This composes with the existing `getblocktemplate` ZIP-317 quadratic selection
and large response-construction notes, but it is a separate polling shape: the
server can repeat the full-mempool clone for many waiting clients before any
template is returned.

The practical severity is low-to-medium. The path requires mining RPC access,
RPC is disabled by default, cookie auth is enabled by default when RPC is
enabled, and server-level connection limits bound the number of live waiters in
ordinary configurations. It is still worth hardening because a mining pool,
proxy, or copied Docker deployment can expose this surface to semi-trusted
clients.

## Suggested Fix

Prefer bounding and sharing long-poll work:

- Add a global and/or per-client cap for active `getblocktemplate` long-poll
  waiters.
- Add a short backoff or wait-on-change path before retrying after a
  state/mempool tip mismatch.
- Deduplicate matching long-poll requests so one background waiter performs the
  five-second state/mempool check and fans the result out to all clients.
- If a full fanout cache is too invasive, add a short-lived shared snapshot for
  `FullTransactions` results used by concurrent long-poll waiters.
- Add metrics for active GBT long-poll waiters and repeated
  `FullTransactions` polls.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "longpoll" "full mempool" "amplification"'
gh api repos/ZcashFoundation/zebra/issues/9301
gh api repos/ZcashFoundation/zebra/issues/9727
```

No exact issue hits were returned.

Closest broad overlap: #9301 ("DoS vulnerability in `getblocktemplate` RPC")
and #9727 ("Respond quickly to long-polled `getblocktemplate` RPC on new chain
tips") are open and cover general GBT DoS / long-poll responsiveness themes,
but not this exact repeated full-mempool polling shape.

Current-behavior proof added and rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc getblocktemplate_matching_longpollid_repeats_full_mempool_poll_today --lib
cargo test -p zebra-rpc getblocktemplate --lib
```

Result: passed.

The new test lives in `zebra-rpc/src/methods/tests/vectors.rs`. It constructs a
matching current `LongPollId` from the mocked tip, empty mempool, and `max_time`,
then starts a long-poll `getblocktemplate` request. The mocked services observe
and answer `ReadRequest::ChainInfo` plus `mempool::Request::FullTransactions`,
advance the virtual clock by `MEMPOOL_LONG_POLL_INTERVAL`, then observe the
same state and full-mempool requests again while the RPC remains pending. This
confirms the periodic full-mempool polling shape directly.

## Disclosure Posture

Treat as public hardening on current evidence.

This does not crash the process, bypass consensus, or mint funds. It requires
configured mining RPC access and is bounded by server-level connection/request
limits. It may deserve a private heads-up only if a specific deployment exposes
mining RPC broadly with large mempools and high allowed concurrency.

## Confidence

Confidence: medium-high.

The source-to-sink path is direct: matching long-poll RPC request to repeated
`FullTransactions` request to full verified-mempool clone. The remaining
uncertainty is practical exploitability under real mining deployments and the
effective server concurrency limit in each deployment.
