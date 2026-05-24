# Post-v4.4.0 Security Audit Continuation

Date: 2026-05-03

Scope: continuation after the pass-5 findings document, focused on avoiding
duplicate work and checking fresh-but-adjacent panic and availability leads.

## Private Maintainer Heads-Up Candidates

### Mempool downloader timeout cancel-handle retention

Status: private maintainer heads-up candidate pending exploitability
validation.

Confidence: high on the stale downloader state, medium on practical default-node
exploitability.

The mempool transaction downloader uses `pending.len()` to cap active work at
`MAX_INBOUND_CONCURRENCY`, and `cancel_handles` to deduplicate active
`UnminedTxId`s. On normal success and normal error, `Downloads::poll_next()`
removes the txid from `cancel_handles`, but on the outer timeout branch it
returns `Err(elapsed)` without removing the map entry
(`zebrad/src/components/mempool/downloads.rs:215-228`). The spawned task hits
that timeout through `tokio::time::timeout(RATE_LIMIT_DELAY, fut)`
(`zebrad/src/components/mempool/downloads.rs:412-450`).

The consumer cannot clean it up because the timeout item carries no txid.
`Mempool::poll_ready()` explicitly says there is no specific transaction ID on
timeout and only logs/metrics the timeout
(`zebrad/src/components/mempool.rs:663-671`). After the timeout, the task is no
longer in `pending`, but the original `Gossip` request remains in
`cancel_handles`, so the same txid is permanently `AlreadyQueued` while unique
later txids can still be admitted under the active-task cap.

This is especially concerning for direct pushed transactions because the stored
`Gossip::Tx` can retain full transaction contents: peer `tx` messages become
`Request::PushTransaction(transaction.clone())`
(`zebra-network/src/peer/connection.rs:1278`), inbound forwards them to the
mempool queue (`zebrad/src/components/inbound.rs:526-530`), and
`download_if_needed_and_verify()` clones the `Gossip` into `cancel_handles`
(`zebrad/src/components/mempool/downloads.rs:456-460`). A plausible remote
timeout shape is a transaction spending transparent outpoints absent from the
best chain, causing mempool verification to wait on
`mempool::Request::AwaitOutput(outpoint)` while looking for mempool
dependencies (`zebra-consensus/src/transaction.rs:700-750`).

Local proof:

```text
cargo test -p zebrad timed_out_download_keeps_cancel_handle_today --lib
```

The paused-time test shows the timeout path leaves `pending` empty, keeps the
request visible through `transaction_requests()`, and makes requeueing the same
txid return `MempoolError::AlreadyQueued`
(`zebrad/src/components/mempool/downloads.rs:590-622`).

Suggested fix direction:

- Carry `UnminedTxId` in the timeout result, or use a local terminal-result enum.
- Remove `cancel_handles[txid]` in `Downloads::poll_next()` for success,
  normal error, cancellation, and timeout.
- Preserve current policy that timeout is not transaction invalidity: do not add
  timeout to rejection caches and do not score the peer solely for timeout.
- Add a post-fix regression proving the txid can be requeued after timeout and
  stale `transaction_requests()` entries are gone.

Detailed note:
`docs/analysis/mempool-downloader-timeout-cancel-handle-retention-finding.md`.

## Public Hardening Findings

### RPC solution-rate full-chain scan

Status: public availability hardening.

Confidence: medium-high on the behavior, medium-low on security severity.

`getnetworksolps` and `getnetworkhashps` accept an optional `num_blocks` as an
`i32`. Positive values are converted directly to `usize` and sent to the state
service:

- `zebra-rpc/src/methods.rs:2682-2703`

The state service passes that value to `read::difficulty::solution_rate()`:

- `zebra-state/src/service.rs:1617-1652`

`solution_rate()` walks ancestor headers and caps only by actual chain length:

- `zebra-state/src/service/read/difficulty.rs:92-100`

So a reachable RPC client can request a very large positive `num_blocks`, up to
`i32::MAX`, and force Zebra to scan from the selected start hash back through as
much of the available chain as exists. This is bounded by chain height and the
RPC service's normal timeout/concurrency controls, so it is not unbounded memory
growth and not consensus relevant. It is still a cheap CPU/storage-read
amplification path for exposed RPC configurations.

Suggested fix direction:

- Cap `num_blocks` at a small operational maximum, or at least at the existing
  default/window size used by zcashd-compatible callers.
- Return an invalid-parameter error when the caller requests an excessive
  window.
- Add regression coverage for the existing snapshot TODO around excessive
  `num_blocks`.

Reproducer sketch:

- Call `getnetworksolps` with `num_blocks = i32::MAX` on a synced test state.
- Instrument or mock `read::difficulty::solution_rate()` / ancestor iteration
  and assert the request is capped before the state scan.

### P2P transaction `getdata` lookup amplification

Status: public P2P/mempool hardening.

Confidence: high on current behavior, medium-low on security severity.

Inbound `getdata` messages are bounded by the protocol inventory preallocation
limit:

- `zebra-network/src/protocol/external/inv.rs:182-210`

But the bound is still large: up to 50,000 inventory items per received
message. If the request contains any transaction inventory items and no block
items, the connection converts the full transaction-id set into an internal
`TransactionsById` request:

- `zebra-network/src/peer/connection.rs:1313-1331`

The inbound service forwards the whole set to the mempool service and then
computes `missing` by iterating over the original request set:

- `zebrad/src/components/inbound.rs:465-504`

The mempool service performs exact lookups for every requested ID:

- `zebrad/src/components/mempool.rs:780-787`
- `zebrad/src/components/mempool/storage.rs:699-713`

This lets one peer turn a single valid `getdata` message into tens of thousands
of hash-set/mempool lookups and a large `notfound` response. The path is bounded
by message size, timeout/load-shed layers, and connection limits, so it is a
hardening issue rather than an unbounded DoS. It is still worth tightening
because transaction-serving peers do not need to honor huge arbitrary lookup
sets from a single remote peer.

Suggested fix direction:

- Add an inbound transaction `getdata` lookup cap before calling the mempool.
- Prefer a much smaller transaction-response cap than the raw inventory-message
  deserialization limit.
- Consider penalizing or disconnecting peers that repeatedly ask for large
  mostly-missing transaction sets.

Reproducer sketch:

- Mock an inbound peer sending a `getdata` with the maximum permitted number of
  transaction inventory items.
- Assert that the mempool receives at most the configured cap and the response
  is capped or the peer is disconnected/scored.

### P2P transaction `inv` queue amplification

Status: public P2P/mempool hardening.

Confidence: high on current behavior, medium-low on practical severity.

A peer can send a protocol-valid `inv` with up to 50,000 inventory entries:

- `zebra-network/src/protocol/external/inv.rs:182-210`

If any entries are transaction inventory, the connection maps them into
`AdvertiseTransactionIds`:

- `zebra-network/src/peer/connection.rs:1279-1295`
- `zebra-network/src/peer/connection.rs:1815-1822`

The inbound service forwards the whole set to the mempool queue and ignores the
per-entry response:

- `zebrad/src/components/inbound.rs:534-541`

The mempool queue path still iterates every entry, creates oneshot response
bookkeeping, invokes duplicate/download checks, and collects one result per
input:

- `zebrad/src/components/mempool.rs:862-883`

The downloader itself caps active inbound downloads at 25:

- `zebrad/src/components/mempool/downloads.rs:82-105`
- `zebrad/src/components/mempool/downloads.rs:276-307`

So a large transaction `inv` can force avoidable per-entry CPU/allocation work
before most entries are rejected as already queued or over the queue cap. This
is bounded by the protocol inventory cap and inbound timeout/load-shed layers,
so it is public hardening rather than private disclosure.

Suggested fix direction:

- Cap transaction IDs from one inbound `inv` before forwarding to the mempool
  queue.
- Use the downloader concurrency cap, or a small multiple, as the first
  hardening bound.
- Add a fire-and-forget queue path for inbound advertisements so ignored
  responses do not allocate one response channel per advertised ID.

Detailed note: `docs/analysis/p2p-transaction-inv-queue-amplification-note.md`.

### P2P `mempool` request full-enumeration amplification

Status: public P2P/mempool hardening.

Confidence: high on current behavior, medium-low on practical severity.

Inbound `mempool` messages are mapped to `MempoolTransactionIds`:

- `zebra-network/src/peer/connection.rs:1362`

The inbound service asks the mempool for all transaction IDs:

- `zebrad/src/components/inbound.rs:547-554`
- `zebra-node-services/src/mempool.rs:37-39`

The mempool service collects every local mempool transaction ID into a
`HashSet`:

- `zebrad/src/components/mempool.rs:770-778`

Only after that does the connection layer truncate the outbound `inv` response
to `MAX_TX_INV_IN_SENT_MESSAGE = 25_000`:

- `zebra-network/src/peer/connection.rs:1537-1559`
- `zebra-network/src/protocol/external/inv.rs:192-201`

This means a constant-size unauthenticated P2P request can force work
proportional to the local mempool size, even when Zebra will not send the whole
set. Default mempool limits, inbound timeouts, and load shedding bound the
impact.

Suggested fix direction:

- Add a bounded mempool request variant such as
  `TransactionIdsLimited { max: usize }`.
- Have the mempool service stop after `max` IDs instead of collecting all IDs.
- Keep the existing connection-layer truncation as defense in depth.

Detailed note: `docs/analysis/p2p-mempool-request-enumeration-note.md`.

### P2P `getaddr` empty-cache rescan amplification

Status: public P2P availability and privacy hardening.

Confidence: medium-high on behavior, medium-low on practical severity.

The direct address-book clone/shuffle concern is mostly eliminated. Zebra keeps
one cached partial `getaddr` response in the inbound service:

- `zebrad/src/components/inbound.rs:397-409`
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:40-70`

The cache refreshes only every 10 minutes, and fresh responses are capped to
`min(MAX_ADDRS_IN_MESSAGE, address_book_len / ADDR_RESPONSE_LIMIT_DENOMINATOR)`:

- `zebrad/src/components/inbound/cached_peer_addr_response.rs:16`
- `zebra-network/src/address_book.rs:274-280`
- `zebra-network/src/constants.rs:301-313`

The empty-result path is weaker. `try_refresh()` advances `refresh_time` only
when the refreshed peer list is non-empty; empty refreshes and lock-contention
expiry branches do not advance the refresh deadline:

- `zebrad/src/components/inbound/cached_peer_addr_response.rs:66-90`

So a stale or isolated node with many retained address-book entries but no
currently gossipable peers can be driven into a full fresh-response attempt on
every inbound `getaddr`. That attempt clones, filters, collects, and shuffles the
address book before returning an empty result:

- `zebra-network/src/address_book.rs:285-315`

In the normal non-empty-cache path, every inbound `Message::GetAddr` still maps
to `Request::Peers`, clones the cached response, and sends it as an outbound
`addr` message:

- `zebra-network/src/peer/connection.rs:1345`
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:40-42`
- `zebra-network/src/peer/connection.rs:1465-1469`
- `zebra-network/src/protocol/external/codec.rs:274-283`

With a full address book, that cached response can contain 1,000 v1 address
entries, roughly 30 KB plus framing, in response to a tiny `getaddr`. This
secondary path is bounded by connection limits, per-peer request sequencing, send
timeouts, inbound load shedding, and the protocol/address-book caps.

Suggested fix direction: keep the global cache, but advance the next-refresh
deadline after empty refreshes or briefly cache `Nil`; add per-connection or
per-peer `getaddr` response throttling as defense in depth.

Detailed note: `docs/analysis/p2p-getaddr-response-amplification-note.md`.

### P2P stale gossiped address dial churn

Status: public P2P availability hardening.

Confidence: high on current behavior, medium-low on practical severity.

The address-crawler path already limits how many addresses it accepts per peer
response, and old entries are not gossipable. But old gossiped addresses are
still accepted into the address book and are still eligible for one initial
outbound connection attempt:

- `zebra-network/src/peer_set/candidate_set.rs:315-323`
- `zebra-network/src/peer_set/candidate_set.rs:446-479`
- `zebra-network/src/meta_addr.rs:630-638`
- `zebra-network/src/meta_addr.rs:666-684`

The relevant TODO is explicit: `validate_addrs()` says Zebra should eventually
ignore peers older than 3 weeks, but today it only clamps future timestamps and
rejects underflow cases:

- `zebra-network/src/peer_set/candidate_set.rs:469-477`

This means an attacker-controlled peer can supply old, unreachable, but
syntactically valid Zcash listener addresses. Zebra will not gossip those
entries outside the three-hour active-gossip window, but it can still spend
crawler/dialer work on each one before marking it failed. The path is bounded by
per-response address limits, crawler fanout, crawl rate limits, address-book
capacity, connection-attempt rate limits, and handshake timeouts:

- `zebra-network/src/constants.rs:88-90`
- `zebra-network/src/constants.rs:255-287`
- `zebra-network/src/peer_set/candidate_set.rs:400-422`
- `zebra-network/src/peer_set/initialize.rs:1093-1159`

I temporarily added and removed a direct unit test proving that a gossiped peer
last seen 30 days ago survives `validate_addrs()` and returns true from
`is_ready_for_connection_attempt()` while it is still
`NeverAttemptedGossiped`:

```sh
cargo test -p zebra-network old_gossiped_peer_is_still_initially_connectable_today --lib
```

Result: the test passed.

Suggested fix direction: implement the existing `validate_addrs()` old-address
filter, after clock-skew normalization, and add regressions for both normal
`getaddr` responses and unsolicited cached `addr` entries.

Detailed note: `docs/analysis/p2p-stale-gossiped-address-dial-churn-note.md`.

### RPC `z_listunifiedreceivers` invalid Orchard receiver echo

Status: public RPC validation hardening; adjacent to the private Sapling panic.

Confidence: high on behavior, low on security severity.

The fatal `z_listunifiedreceivers` issue is the Sapling receiver path, which
unwraps semantic conversion of decoded bytes. A follow-up check showed that the
Orchard branch has the same "structural Unified Address decode is enough"
assumption, but with a lower-impact result:

- `zebra-rpc/src/methods.rs:2886-2889`
- `zebra-chain/src/primitives/address.rs:88-97`

For `Receiver::Orchard([0; 43])`, Zebra's whole-address conversion rejects the
Unified Address because the Orchard receiver bytes are semantically invalid.
But `z_listunifiedreceivers` re-wraps the decoded Orchard item with
`zcash_address::unified::Address::try_from_items(vec![item])`, which checks the
Unified Address container shape but not Orchard payment-address validity. The
RPC therefore returns `Ok` with an `orchard` field containing an Orchard-only UA
for invalid Orchard bytes.

I temporarily added and removed a direct proof test:

```sh
cargo test -p zebra-rpc rpc_z_listunifiedreceivers_echoes_invalid_orchard_receiver_today --lib
```

Result: the test passed.

This is not another process-fatal bug. It is a low-severity validation bug in an
RPC helper method and should be fixed with the same whole-Unified-Address
validation step recommended for the Sapling panic.

Detailed note:
`docs/analysis/rpc-z-listunifiedreceivers-invalid-sapling-panic-finding.md`.

### Mempool metrics full-scan amplification

Status: public P2P/mempool availability hardening.

Confidence: high on the code shape, medium-low on practical severity.

The verified mempool set recomputes several aggregate metrics by scanning every
stored transaction after hot-path mutations. Successful insertion updates
indexed structures and then calls `update_metrics()`:

- `zebrad/src/components/mempool/storage/verified_set.rs:148-190`

Removal also calls the same recomputation after deleting a transaction and its
dependents:

- `zebrad/src/components/mempool/storage/verified_set.rs:288-308`

`update_metrics()` walks all stored transactions to recompute unpaid-action and
weighted-size buckets before emitting gauges:

- `zebrad/src/components/mempool/storage/verified_set.rs:370-473`

This turns attacker-fed mempool growth into an additional `1 + 2 + ... + n`
metric-accounting walk over accepted transactions. With the default
`tx_cost_limit = 80,000,000` and minimum transaction cost of 10,000 bytes, the
default bound is on the order of 8,000 minimum-cost transactions:

- `zebrad/src/components/mempool/config.rs:52-65`
- `zebra-chain/src/transaction/unmined.rs:60-67`

This is bounded and does not imply consensus failure or invalid transaction
acceptance. It is still avoidable work on the mempool mutation path. The fix is
to maintain these buckets incrementally or recompute them periodically instead
of scanning the full verified set after each insert/remove.

Detailed note: `docs/analysis/mempool-metrics-full-scan-amplification-note.md`.

### RPC `getrawmempool(true)` quadratic verbose assembly

Status: public RPC availability hardening.

Confidence: high on the code shape, medium-low on practical severity.

Verbose `getrawmempool` asks the mempool for `FullTransactions`:

- `zebra-rpc/src/methods.rs:1618-1621`

The mempool clones every stored verified transaction and the dependency graph:

- `zebrad/src/components/mempool.rs:834-848`

The RPC handler then iterates each transaction and calls
`MempoolObject::from_verified_unmined_tx()`:

- `zebra-rpc/src/methods.rs:1637-1650`

That helper rebuilds a full transaction-id lookup map from the entire
transaction slice for each object:

- `zebra-rpc/src/methods/types/get_raw_mempool.rs:59-68`

So a mempool with `n` transactions does `n` all-mempool map constructions before
JSON serialization. RPC is disabled/authenticated by default and mempool size is
bounded, but exposed or shared RPC deployments can still be pushed into avoidable
CPU/allocation work.

Suggested fix direction:

- Build the transaction-id lookup map once per verbose request.
- Pass the precomputed map into `MempoolObject::from_verified_unmined_tx()`.
- Preserve the existing response schema.

Detailed note: `docs/analysis/rpc-getrawmempool-verbose-quadratic-note.md`.

### RPC verbose Orchard action serialization quadratic work

Status: public RPC availability hardening.

Confidence: high on the code shape, medium-low on practical severity.

Verbose transaction output uses `TransactionObject::from_transaction()` for
`getrawtransaction(..., verbose=1)` and for every transaction in
`getblock(..., verbosity=2)`:

- `zebra-rpc/src/methods.rs:1718-1730`
- `zebra-rpc/src/methods.rs:1783-1790`
- `zebra-rpc/src/methods.rs:1329-1352`

Inside `TransactionObject`, the Orchard response path collects
`tx.orchard_actions()` into a vector, iterates it, and for each action searches
the whole `shielded_data.actions` list to find the matching spend authorization
signature:

- `zebra-rpc/src/methods/types/transaction.rs:864-902`

`Action` equality includes the full action description, including ciphertext
fields:

- `zebra-chain/src/orchard/action.rs:23-42`

The action count is bounded by block size and consensus limits:

- `zebra-chain/src/orchard/shielded_data.rs:185-204`
- `zebra-chain/src/block/serialize.rs:24`

So this is not unbounded, but a caller with RPC access can request verbose
serialization of a transaction or block with many Orchard actions and trigger
avoidable `O(n^2)` action comparisons before the expected large JSON response is
returned.

Suggested fix direction:

- Iterate `shielded_data.actions.iter()` directly when Orchard data exists, so
  each action and its `spend_auth_sig` are consumed as an already-paired
  `AuthorizedAction`.
- Avoid collecting actions and searching back through the same list.

Detailed note: `docs/analysis/rpc-verbose-orchard-action-quadratic-note.md`.

### Indexer non-finalized-state stream subscriber amplification

Status: public indexer availability hardening.

Confidence: high on the code shape, medium on practical severity.

The optional indexer gRPC server is disabled by default, but when
`rpc.indexer_listen_addr` is configured it starts a plain tonic server with
reflection and `IndexerServer::new(...)`:

- `zebra-rpc/src/config/rpc.rs:33-47`
- `zebrad/src/commands/start.rs:277-287`
- `zebra-rpc/src/indexer/server.rs:57-61`

The `NonFinalizedStateChange` streaming method creates a per-RPC task and asks
state for a fresh `ReadRequest::NonFinalizedBlocksListener`:

- `zebra-rpc/proto/indexer.proto:48-56`
- `zebra-rpc/src/indexer/methods.rs:84-99`
- `zebra-state/src/service.rs:1314-1327`

Each state-side listener then spawns its own task, starts from an empty previous
state, clones the watched `NonFinalizedState`, walks chains, checks candidate
blocks against the previous snapshot, and queues unseen blocks through a
1,000-item channel:

- `zebra-state/src/response.rs:219-279`
- `zebra-state/src/service/watch_receiver.rs:113-115`
- `zebra-state/src/service/non_finalized_state.rs:101-114`

The RPC layer then serializes each block into a `BlockAndHash` response:

- `zebra-rpc/src/indexer.rs:45-57`

So if the indexer port is exposed to untrusted clients, many live
`NonFinalizedStateChange` streams can multiply clone/diff work and full-block
serialization work on every non-finalized-state update. Slow consumers are
dropped once the 64-message RPC response channel fills, but clients that keep
reading can keep independent state listener tasks alive.

Existing bounds keep this out of consensus-critical territory: non-finalized
fork count is capped at 10, the best chain is finalized past
`MAX_BLOCK_REORG_HEIGHT`, the state-side channel is bounded to 1,000 block
references, and the indexer is opt-in. The missing control is a subscriber cap
or shared broadcast model that prevents work from scaling linearly with client
count.

Suggested fix direction:

- Add explicit connection/subscriber limits for the indexer server.
- Prefer one shared non-finalized-state diff/broadcast task instead of one diff
  task per subscriber.
- Bound or configure the initial catch-up burst for new subscribers.
- Set tonic server limits such as `max_concurrent_streams`,
  `concurrency_limit_per_connection`, request timeout, keepalive, and load
  shedding.

Detailed note:
`docs/analysis/indexer-non-finalized-state-stream-amplification-note.md`.

### Non-finalized transparent received-value overflow

Status: public correctness / availability hardening.

Confidence: medium on the code behavior, low on practical exploitability.

Finalized transparent address `received` accounting uses saturating addition,
and the final merge between finalized and non-finalized balances also documents
that an address can receive more than the max money supply by sending to itself:

- `zebra-state/src/service/finalized_state/disk_format/transparent.rs:255`
- `zebra-state/src/service/read/address/balance.rs:140-147`

But the non-finalized address index uses normal `u64` summation in two places:

- `TransparentTransfers::received()` sums created UTXO values with `.sum()` in
  `zebra-state/src/service/non_finalized_state/chain/index.rs:233-237`;
- `Chain::partial_transparent_balance_change()` combines per-address received
  totals with `received + transfers.received()` in
  `zebra-state/src/service/non_finalized_state/chain.rs:1357-1365`.

In debug builds, a `u64` overflow panics, and this workspace sets
`panic = "abort"` for the dev profile. In release builds, default Rust overflow
semantics wrap the value, producing an incorrect RPC `received` total rather
than a panic.

This is not a consensus issue: the balance path uses checked `Amount`
arithmetic, and the overflow only affects address-index `received` accounting
for `getaddressbalance`. It also needs an extreme valid-chain shape: enough
recent non-finalized transparent self-transfer churn to push summed received
zatoshis above `u64::MAX`. Same-block spends are supported by contextual UTXO
lookup, and the serialized block size constants show a very high theoretical
transaction count per block, but the required value turnover is still
economically and operationally unrealistic for ordinary attackers.

Suggested fix direction:

- Change the non-finalized `received` paths to `saturating_add()` or an
  explicit saturating sum, matching finalized accounting.
- Add a small unit test around `TransparentTransfers::received()` or
  `partial_transparent_balance_change()` with synthetic high-value transfers.
- Treat this as public cleanup unless further testing shows a cheap way to
  produce the overflow through ordinary mempool/RPC inputs.

### Regtest/custom-testnet `generate` RPC unbounded loop

Status: public RPC hardening for PoW-disabled networks.

Confidence: high on behavior, low on normal-deployment severity.

The `generate` RPC takes an uncapped `u32` `num_blocks`. It rejects normal
networks where proof of work is enabled, but on PoW-disabled networks it loops
once per requested block:

- `zebra-rpc/src/methods.rs:2946-3013`

Each iteration:

- mutates the coinbase extra data;
- calls `get_block_template(None)`;
- turns the template into a proposal block;
- serializes it;
- submits it through `submit_block()`;
- appends the generated hash to an in-memory response vector.

Regtest enables `disable_pow` by default:

- `zebra-chain/src/parameters/network/testnet.rs:994-1000`

Custom Testnets can also set `disable_pow = true`, and the user docs show that
configuration:

- `book/src/user/custom-testnets.md:14`
- `book/src/user/custom-testnets.md:36-43`

So an authenticated or exposed RPC client on a PoW-disabled network can request
an extremely large number of generated blocks, tying up an RPC worker and
forcing repeated template generation, block serialization, state submission, and
response-vector growth. This is not mainnet/testnet consensus-critical, and
Regtest is documented as private/isolated test infrastructure, so the issue is
public hardening rather than private disclosure.

Suggested fix direction:

- Cap `num_blocks` to a small configurable maximum for one RPC call.
- Return an invalid-parameter error for excessive values.
- Consider streaming/progress alternatives for test tooling that genuinely
  needs many blocks.

### Trusted RPC `invalidateblock` invalidated-block cache footprint

Status: public trusted-RPC hardening.

Confidence: high on behavior, low-medium on normal-deployment severity.

The `invalidateblock` RPC is intentionally powerful: it removes a block and its
descendants from the non-finalized state, and future contextual validation
rejects blocks that remain in the invalidated-block cache:

- `zebra-rpc/src/methods.rs:695-703`
- `zebra-rpc/src/methods.rs:2917-2928`
- `zebra-state/src/service/non_finalized_state.rs:372-410`
- `zebra-state/src/service/non_finalized_state.rs:558-564`

The operation is bounded to the non-finalized state. `invalidateblock` returns
`BlockNotFound` for hashes outside tracked non-finalized chains, and the normal
write loop finalizes best-chain blocks once the non-finalized best chain grows
past `MAX_BLOCK_REORG_HEIGHT`:

- `zebra-state/src/error.rs:114-133`
- `zebra-state/src/service/write.rs:442-451`
- `zebra-state/src/constants.rs:14-31`

The hardening concern is memory footprint. Each invalidation stores the removed
block plus all descendants in `invalidated_blocks`, and Zebra retains up to
`MAX_INVALIDATED_BLOCKS = 100` invalidated entries. The source comment estimates
the bound as roughly `100 entries * up to 99 blocks * 2 MB per block = 20 GB`:

- `zebra-state/src/constants.rs:112-116`

An exposed or compromised RPC client could repeatedly feed and invalidate valid
recent forks to push the node toward that cache budget. This is not a consensus
issue and is not a peer-only attack; it requires RPC control plus valid
non-finalized block data. But it is a large enough trusted-RPC resource budget
that a byte-oriented cap would be safer than a record-count cap.

There is also a functional edge in the same cache: invalidated records are keyed
only by height, and the code has a TODO noting that multiple invalidated block
hashes at the same height are not yet supported. Invalidating a second block at
the same height replaces the earlier entry, which weakens the RPC documentation's
"rejecting it during contextual validation if it is submitted" guarantee:

- `zebra-state/src/service/non_finalized_state.rs:398-404`

Suggested fix direction:

- Add a byte/serialized-size cap for cached invalidated blocks, not just an
  entry count.
- Lower the retained invalidated-record bound or make it configurable.
- Track invalidated blocks by hash, or by `(height, hash)`, so competing
  same-height invalidations do not evict each other.
- Document `invalidateblock` / `reconsiderblock` as dangerous trusted-control
  RPCs alongside any public-bind warnings.

### Inbound gossiped-block verifier misbehavior drop

Status: public P2P hardening.

Confidence: high on the type mismatch, medium on operational severity.

Inbound gossiped blocks are downloaded and then committed through the semantic
block verifier. The inbound verifier service alias returns `RouterError`
(`zebrad/src/components/inbound.rs:82-87`), and the downloader preserves the
responding peer address on verifier failures
(`zebrad/src/components/inbound/downloads.rs:394-398`). But when
`Inbound::poll_ready()` drains completed downloader tasks, it only forwards
misbehavior if the boxed error downcasts to `VerifyBlockError`
(`zebrad/src/components/inbound.rs:332-343`).

That misses the live error type: `RouterError` wraps `VerifyBlockError` and
delegates `misbehavior_score()` to the inner verifier error
(`zebra-consensus/src/router.rs:100-153`). A local backstop test documents the
current mismatch by boxing a score-bearing `RouterError` and confirming that
the inbound downcast target fails while the router error still has score 100
(`zebrad/src/components/inbound/tests.rs:6-32`).

The invalid block is still rejected, and the inbound downloader remains bounded
by hash deduplication, total queue limits, and one in-flight download per
advertiser IP. The hardening issue is missed address-book scoring/banning for
peers that serve score-bearing invalid gossiped blocks.

Suggested fix direction:

- Downcast or otherwise extract `RouterError` in `Inbound::poll_ready()`, then
  use `RouterError::misbehavior_score()`.
- Keep direct `VerifyBlockError` support only if another production verifier
  wiring still needs it.
- Add an integration-style inbound downloader regression that observes a
  score-bearing verifier error arriving on `misbehavior_sender`.

Detailed note:
`docs/analysis/inbound-gossiped-block-router-error-misbehavior-note.md`.

## Eliminated Leads

### P2P `getblocks` / `getheaders` locator scan

Result: duplicate of existing pass-5 finding, not a new continuation finding.

The fresh scan independently rediscovered the oversized block-locator request
shape: inbound `getblocks` and `getheaders` can carry large `known_blocks`
vectors, and Zebra scans for a chain intersection before returning a capped
response. This is already documented in:

- `docs/analysis/p2p-block-locator-length-hardening-note.md`
- `docs/analysis/post-v4.4.0-security-audit-pass-5-findings.md:613-626`

Do not count this as a new finding from this continuation pass.

### RPC `rpc.discover` schema regeneration

Result: deprioritized as low-severity bounded work.

The `openrpc` / `rpc.discover` path regenerates the OpenRPC schema on each
request:

- `zebra-rpc/src/methods.rs:3028-3046`

That is avoidable CPU/allocation work, but it is over a static method set and
does not scale with chain height, mempool size, peer-supplied vector lengths, or
attacker-chosen numeric parameters. Caching the schema would be a reasonable
cleanup, but this did not meet the bar for a separate security finding in this
pass.

### Health endpoint connection-hold DoS

Result: duplicate of existing pass-5 finding, not a new continuation finding.

The fresh scan independently rediscovered that the optional health endpoint has
an accept-rate counter but no open-connection cap or request/header timeout.
This is already documented in:

- `docs/analysis/health-endpoint-connection-hardening-note.md`
- `docs/analysis/post-v4.4.0-security-audit-pass-5-findings.md:837-847`

Do not count this as a new finding from this continuation pass.

### RPC `sendrawtransaction` retry queue

Result: duplicate / already bounded, not a new continuation finding.

The fresh scan revisited the RPC transaction retry queue because it stores
caller-submitted transactions for later retry. This is already covered in:

- `docs/analysis/post-v4.4.0-security-audit-pass-5-findings.md:927-939`
- `docs/analysis/post-v4.4.0-security-audit-followups.md:119-155`

Current source still shows `CHANNEL_AND_QUEUE_CAPACITY = 20`, a bounded
broadcast channel of that size, and oldest-entry eviction when the queue exceeds
that capacity:

- `zebra-rpc/src/queue.rs:42`
- `zebra-rpc/src/queue.rs:62`
- `zebra-rpc/src/queue.rs:87-94`

So this remains a bounded retryability-semantics / resource-churn hardening
item, not a fresh unbounded memory finding.

### RPC `FixRpcResponseMiddleware` invalid-params panic

Result: eliminated as an attacker-reachable panic through normal JSON-RPC
requests.

The candidate was that `FixRpcResponseMiddleware` unwraps assumptions while
rewriting `InvalidParams` errors:

- `zebra-rpc/src/server/rpc_call_compatibility.rs:42-79`

`jsonrpsee` backs `MethodResponse::is_error()` and
`MethodResponse::as_error_code()` with the same internal `Failed(i32)` state, so
`is_error() == true` implies an error code exists:

- `jsonrpsee-core-0.24.10/src/server/method_response.rs:245-273`

For valid method calls, `jsonrpsee` request IDs deserialize only as `null`,
unsigned number, or string:

- `jsonrpsee-types-0.24.10/src/params.rs:339-350`

Those are exactly the ID shapes Zebra accepts when it parses the response JSON.
Malformed request IDs and notifications are handled before per-call middleware:

- `jsonrpsee-server-0.24.10/src/server.rs:1285-1294`
- `jsonrpsee-server-0.24.10/src/server.rs:1318-1351`

This does not make the middleware ideal; the response-rewrite code could be
made less brittle by avoiding `expect()`. But I did not find a remote caller
shape that reaches those panics through Zebra's installed RPC server.

### Peer connection state invariant panics

Result: eliminated as a current remote peer panic path.

`Connection::handle_client_request()` panics if called while the connection is
`Failed` or already `AwaitingResponse`:

- `zebra-network/src/peer/connection.rs:1015-1026`

But the event loop only polls the client request channel in
`State::AwaitingRequest`, stops polling it while waiting for a peer response,
and exits once the state is `Failed`:

- `zebra-network/src/peer/connection.rs:709-779`
- `zebra-network/src/peer/connection.rs:827-947`

The client side also checks request-channel readiness and task errors before
sending a request:

- `zebra-network/src/peer/client.rs:614-668`

So these panics remain internal invariant checks. A remote peer can cause
timeouts, disconnects, or request errors, but I did not find a path where remote
traffic causes a second client request to be processed in the pending state.

### WTXID Unicode slicing panic

Result: eliminated.

Because the known `longpollid` issue involved slicing a UTF-8 string at fixed
byte offsets, I checked `WtxId::from_str()`, which also splits a string. It is
explicitly safe against that class: it first converts to bytes, checks the byte
length is exactly 128, splits the byte slice, then validates UTF-8 and hex
parsing:

- `zebra-chain/src/transaction/hash.rs:313-335`

This parser can reject invalid input, but it should not panic on multibyte
characters.

### Anchor/nullifier stale-state leak across forks

Result: eliminated for the normal non-finalized fork/reorg path.

Block contextual validation checks anchors and nullifiers against the parent
chain being extended, plus the finalized database:

- `zebra-state/src/service/non_finalized_state.rs:546-585`
- `zebra-state/src/service/check/anchors.rs:24-124`
- `zebra-state/src/service/check/nullifier.rs:103-128`

Forking a chain clones the candidate chain and pops tip blocks until the fork
point. Each popped block runs the same rollback machinery that removes the
block hash, transaction locations, transparent UTXO changes, shielded
nullifiers, Sapling/Orchard/Sprout anchors, note commitment tree indexes,
history tree entries, and value-pool changes:

- `zebra-state/src/service/non_finalized_state/chain.rs:400-416`
- `zebra-state/src/service/non_finalized_state/chain.rs:1693-1833`
- `zebra-state/src/service/non_finalized_state/chain.rs:650-703`
- `zebra-state/src/service/non_finalized_state/chain.rs:852-905`
- `zebra-state/src/service/non_finalized_state/chain.rs:1057-1110`
- `zebra-state/src/service/non_finalized_state/chain.rs:2064-2210`

The property tests also compare forked chains against independently pushed
chains using the whole internal chain state, including anchor maps, note
commitment trees, nullifiers, UTXOs, history trees, and value pools:

- `zebra-state/src/service/non_finalized_state/tests/prop.rs:111-224`
- `zebra-state/src/service/non_finalized_state/tests/prop.rs:224-285`
- `zebra-state/src/service/non_finalized_state/tests/prop.rs:615-650`

I did not find a peer-driven path where an anchor or nullifier from a discarded
fork leaks into validation of a competing fork. The separate trusted-RPC
`invalidateblock` cache caveats are documented as public hardening above, but
they do not change the normal branch-isolation conclusion.

### Transparent address balance arithmetic panic

Result: eliminated for valid chain data.

The transparent address index has several `expect()`s around balance arithmetic:

- the RocksDB merge operator adds serialized address balance changes in
  `zebra-state/src/service/finalized_state/zebra_db/transparent.rs:57-72`;
- finalized address balance aggregation uses checked `Amount` addition and then
  expects the partial sum to remain valid in
  `zebra-state/src/service/finalized_state/zebra_db/transparent.rs:340-353`;
- in-memory address index updates expect `receive_output()` not to overflow in
  `zebra-state/src/service/finalized_state/zebra_db/transparent.rs:522-529`;
- `receive_output()` and `spend_output()` use `i64::checked_add()` /
  `checked_sub()` before re-constraining to `Amount` in
  `zebra-state/src/service/finalized_state/disk_format/transparent.rs:216-245`.

The checked `i64` operations are not reachable as arithmetic panics for valid
blocks: `Amount<NonNegative>` and `Amount<NegativeAllowed>` are constrained to
`0..=MAX_MONEY` and `-MAX_MONEY..=MAX_MONEY` respectively, with `MAX_MONEY` set
to 2,100,000,000,000,000 zatoshis in `zebra-chain/src/amount.rs:548-610`.
Those bounds are far below `i64::MAX`, and transparent outputs are represented
as non-negative `Amount`s before reaching the address-index update path.

The cumulative `received` field uses `saturating_add()` rather than panicking.
That could theoretically cap the accounting value after an impossible amount of
received value, but it does not provide a panic or consensus bypass path in the
reviewed code.

Remaining caveat: the disk `FromDisk` implementations still use unwraps for
malformed RocksDB bytes, so local database corruption can still panic. I did not
find an attacker path from a valid peer block or RPC query to malformed address
balance bytes.

### RPC `addnode` outbound-connection control

Result: eliminated as a current RPC SSRF/direct-dial issue.

The candidate was that an exposed RPC caller might be able to force Zebra to
connect to arbitrary internal or attacker-chosen addresses. In this checkout,
`addnode` is much narrower:

- the RPC is rejected outside Regtest in `zebra-rpc/src/methods.rs:3016-3042`;
- only the `add` command is implemented, not `onetry`, `remove`, or direct
  connection control, in `zebra-rpc/src/methods.rs:4708-4714`;
- `add` calls the `AddressBookPeers::add_peer()` trait method in
  `zebra-network/src/address_book_peers.rs:13-20`;
- the real address book implementation inserts a `MetaAddr::new_initial_peer`
  and returns false if already present in
  `zebra-network/src/address_book.rs:812-829`;
- `AddressBook::update()` filters invalid outbound addresses and enforces the
  address-book size limit in `zebra-network/src/address_book.rs:482-552`.

So an RPC caller on Regtest can mutate the candidate address book, but I did
not find a direct outbound connection primitive or an unbounded address-book
growth primitive. This remains trusted-Regtest RPC behavior, not a private
issue.

### Address-book misbehavior ban panic with multiple connections per IP

Result: new finding.

`AddressBook::update()` has a configuration-dependent panic in the peer-ban
path. The `most_recent_by_ip` cache is explicitly optional: the field comment
says it only supports `max_connections_per_ip == 1` and must be `None` for
larger values, and the constructor only creates it when
`max_connections_per_ip == 1`:

- `zebra-network/src/address_book.rs:76-82`
- `zebra-network/src/address_book.rs:158-169`

The configuration is non-default but supported. The default is 1, while config
parsing accepts any positive configured value:

- `zebra-network/src/constants.rs:71-81`
- `zebra-network/src/config.rs:177-195`
- `zebra-network/src/config.rs:931-957`

The ban branch does not preserve that optional-cache invariant. Once an updated
peer score reaches `MAX_PEER_MISBEHAVIOR_SCORE`, it inserts the banned IP and
then unconditionally unwraps `most_recent_by_ip`:

- `zebra-network/src/constants.rs:389-391`
- `zebra-network/src/address_book.rs:443-458`

The misbehavior update path is network-reachable. Mempool transaction
verification assigns score 100 to many invalid transaction errors and forwards
nonzero scores with the advertising peer address; block verification similarly
assigns score 100 to selected block/Equihash errors and forwards them from
inbound/sync paths:

- `zebra-consensus/src/error.rs:260-300`
- `zebrad/src/components/mempool.rs:641-652`
- `zebra-consensus/src/error.rs:400-410`
- `zebra-consensus/src/block.rs:109-117`
- `zebrad/src/components/inbound.rs:330-344`
- `zebrad/src/components/sync.rs:1143-1151`
- `zebra-network/src/peer_set/initialize.rs:122-157`
- `zebra-network/src/address_book_updater.rs:101-114`

I also temporarily added and then removed a direct `AddressBook` unit test that
constructed an address book with `max_connections_per_ip = 2` and applied an
`UpdateMisbehavior` at `MAX_PEER_MISBEHAVIOR_SCORE`. Running:

```sh
cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one
```

passed with `#[should_panic]`, confirming the panic at the documented `expect`
site.

Triage: private disclosure recommended. This is not a default-node consensus
issue, but it is a remotely triggerable network-facing panic for a supported
non-default configuration.

Detailed note:
`docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`.

### Semantically verified queued-block retention after verifier timeout

Result: public hardening note, not private disclosure on current evidence.

The prior pass eliminated state write queues as a separate unbounded externally
reachable queue because sync/inbound paths are bounded before they reach state.
That conclusion still mostly holds, but there is a narrower cleanup gap: after a
semantically verified block is queued in state waiting for a missing parent, a
sync/inbound verifier timeout drops the caller future without removing the
queued block.

Evidence:

- `zebra-state/src/service/queued_blocks.rs:54-81` stores queued blocks and
  known UTXOs without a local queue length cap.
- `zebra-state/src/service.rs:659-714` queues the semantically verified block
  before checking whether its parent is forkable.
- `zebra-state/src/service.rs:746-748` returns early when the parent is not
  forkable, leaving the block queued.
- `zebra-state/src/service.rs:1028-1039` awaits the queued block result on a
  `oneshot`; if the caller is timed out, the sender remains stored with the
  queued block.
- `zebra-state/src/service/queued_blocks.rs:131-184` prunes by finalized tip
  height and ignores send errors for dropped receivers.
- `zebra-state/src/service.rs:787-815` dequeues queued blocks only when a parent
  arrives and can be sent to the non-finalized write task.
- `zebrad/src/components/sync.rs:141-173` defines `BLOCK_VERIFY_TIMEOUT` for
  missing previous blocks and other stuck verification cases.
- `zebrad/src/components/sync.rs:495-496` and
  `zebrad/src/components/inbound.rs:275-280` wrap sync/inbound block verifier
  calls in that timeout.
- `zebrad/src/components/sync/downloads.rs:589-613` cancels downloader tasks on
  sync reset but does not clear state-service queued blocks.

Existing mitigations keep this out of private-report territory on the current
default public networks: blocks must pass proof-of-work and semantic checks
before queueing, inbound has a 200-task cap plus a one-download-per-IP cap,
sync defaults to 20 full-verification tasks and pauses at lookahead limits, and
height lookahead / finalized-height pruning bound the normal retention window.

The hardening direction is to add abandoned-receiver cleanup or periodic
pruning for queued semantically verified blocks, expose a metric/assertion for
queue count versus lookahead expectations, and update the inbound downloader
comment that says timed-out malicious blocks are deallocated so it accounts for
the already-queued missing-parent case.

Detailed note:
`docs/analysis/state-queued-block-timeout-retention-note.md`.

### Address-book ban cleanup assumes same-IP entries are contiguous

Result: public hardening note.

While validating the address-book ban panic, I found a separate cleanup bug in
the same ban branch. The code tries to remove all entries for a banned IP by
skipping until the first matching IP and then taking while the IP still matches:

- `zebra-network/src/address_book.rs:443-480`

But the address book is an `OrderedMap` ordered by `Reverse<MetaAddr>`, not by
IP:

- `zebra-network/src/address_book.rs:73-74`
- `zebra-network/src/meta_addr.rs:1187-1285`

`MetaAddr::Ord` compares connection state, peer preference, local timestamps,
untrusted last-seen time, and services before IP/port tie-breakers, so same-IP
entries are not guaranteed to be contiguous.

I temporarily added and removed a direct regression with three gossiped entries:
old `127.0.0.1:8233`, middle `127.0.0.2:8233`, and newer
`127.0.0.1:8234`. Applying an `UpdateMisbehavior` at the ban threshold to
`127.0.0.1:8233` and expecting both `127.0.0.1` entries to be gone failed:

```sh
cargo test -p zebra-network banning_ip_removes_non_contiguous_address_book_entries_today
```

The failure was on `address_book.get("127.0.0.1:8233").is_none()`, confirming
that the misbehaving address itself can remain when same-IP entries are
non-contiguous in `MetaAddr` ordering.

This does not appear to bypass the IP ban for active use:

- future updates for banned IPs are rejected in
  `zebra-network/src/address_book.rs:417-423`;
- active peer-set services receive ban updates and are dropped elsewhere;
- `CandidateSet::next()` marks a candidate as reconnecting through
  `guard.update()`, so a stale banned entry returns `None` rather than producing
  a successful outbound candidate in
  `zebra-network/src/peer_set/candidate_set.rs:400-423`.

But the stale entry can still consume address-book space, cause candidate
selection to waste a tick if it becomes the selected reconnection peer, and
possibly remain cache/gossip eligible if the stale entry has zero misbehavior
score and active last-seen data:

- `zebra-network/src/address_book.rs:285-311`
- `zebra-network/src/meta_addr.rs:707-745`

Detailed note:
`docs/analysis/address-book-ban-noncontiguous-ip-cleanup-note.md`.

### Fresh non-test panic, unwrap, and assert sweep

Result: no new attacker-reachable panic confirmed in this slice.

I ran a narrower panic/`unwrap`/`assert` pass over non-test paths in
`zebra-network`, `zebra-rpc`, `zebrad`, and `zebra-state`, then checked the
interesting candidates against actual attacker control. The credible-looking
sites were either config/startup fail-fast behavior, internal service-contract
assertions, or secondary poison paths that require an earlier panic.

Eliminated candidates:

- `zebra-rpc/src/indexer/methods.rs:84-99` calls
  `listener.unwrap()` after requesting
  `ReadRequest::NonFinalizedBlocksListener`. The unwrap itself is brittle:
  `zebra-state/src/response.rs:210-292` stores the receiver in an `Arc` and
  uses `Arc::try_unwrap(...).unwrap()`, which panics if the listener is aliased.
  But the current state path creates a fresh listener per request in
  `zebra-state/src/service.rs:1314-1328` and returns it directly. I did not find
  a remote or gRPC-client-controlled path that clones the same listener before
  extraction. This is a good small hardening target, not a current disclosure
  issue.
- `zebrad/src/components/tracing/endpoint.rs:88-96` panics if the optional
  tracing endpoint is configured but cannot bind. This is operator/config and
  local-environment controlled, not peer or RPC request controlled. If the
  endpoint is intended as best-effort tooling, it could log and return instead
  of panicking; if it is intended as mandatory admin infrastructure, fail-fast is
  a policy choice rather than a vulnerability.
- `zebra-rpc/src/methods/types/get_block_template.rs:476-532` panics/asserts on
  unsupported configured `miner_address` pool type or oversized configured
  `extra_coinbase_data`. These values come from mining config or internal test
  mutation, not `getblocktemplate` request parameters. A fallible constructor
  would improve startup UX but does not change the remote attack surface.
- `zebrad/src/components/inbound/cached_peer_addr_response.rs:88-99` panics on
  a poisoned address-book mutex. That is a secondary failure mode: it needs an
  earlier panic while holding the mutex. The primary address-book panic found in
  this pass is documented separately, so this should not be counted as a fresh
  independent vulnerability.
- `zebrad/src/components/inbound.rs` has multiple `unreachable!` assumptions in
  request and response matching. A peer controls request contents, but not the
  Rust enum response variant returned by Zebra's internal state and mempool
  services. These are internal contract checks.
- `zebra-network/src/peer_set/candidate_set.rs:348-352` expects gossiped peers
  to have services set. The remote `addr` and `addrv2` deserializers construct
  `MetaAddr` values through `MetaAddr::new_gossiped_meta_addr()`, which fills
  `services` and `untrusted_last_seen`; unsupported `addrv2` entries are
  filtered before this path. I did not find a remote bypass.

Hardening ideas worth keeping, but not private-disclosure material on current
evidence:

- Make `NonFinalizedBlocksListener` single-owner in the type system, or replace
  its panicking `unwrap()` with a fallible `into_receiver()` and convert failure
  to a gRPC internal error.
- Decide whether optional tracing endpoint bind failure should be fatal or
  best-effort, then encode that policy without an async task panic if the answer
  is best-effort.
- If desired, make `GetBlockTemplateHandler::new()` return a typed config error
  for invalid mining config instead of panicking during startup.

### Trusted chain sync accepts indexer-streamed block/hash state

Result: public hardening note.

The `TrustedChainSync` helper is explicitly a trusted-source read-state syncer,
so this is not a default-node consensus issue. But the validation boundary is
worth documenting because it is easy for downstream users to treat the endpoint
as merely "a remote Zebra indexer" rather than as fully trusted infrastructure.

The syncer connects to a caller-supplied indexer gRPC address, receives
`BlockAndHash` messages from `NonFinalizedStateChange`, decodes a serialized
block and a separately supplied hash, and wraps them in
`SemanticallyVerifiedBlock::with_hash(...)`:

- `zebra-rpc/src/sync.rs:43-55`
- `zebra-rpc/src/sync.rs:157-186`
- `zebra-rpc/src/indexer.rs:60-76`

`BlockAndHash::decode()` checks that the supplied hash field is 32 bytes and the
block bytes deserialize, but it does not check that the supplied hash equals
`block.hash()`.

The syncer then calls `NonFinalizedState::commit_new_chain()` or
`NonFinalizedState::commit_block()` directly through
`TrustedChainSync::try_commit()`:

- `zebra-rpc/src/sync.rs:217-225`
- `zebra-state/src/service/non_finalized_state.rs:345-366`
- `zebra-state/src/service/non_finalized_state.rs:510-539`

That bypasses the normal state-service helper in
`zebra-state/src/service/write.rs:55-62`, which calls
`check::initial_contextual_validity()` before lower-level non-finalized commit.
The skipped gate is where Zebra checks recent-chain context such as parent
height sequencing, difficulty threshold, and timestamp rules:

- `zebra-state/src/service/check.rs:396-415`
- `zebra-state/src/service/check.rs:50-130`

The lower-level commit still does transparent spend/value-balance checks,
anchor checks, chain-history block commitment checks, and tree/index updates.
But it also indexes the block by the `SemanticallyVerifiedBlock.hash` value that
came from the stream:

- `zebra-state/src/service/non_finalized_state/chain.rs:1533-1542`
- `zebra-state/src/service/non_finalized_state/chain.rs:1200-1206`

So a malicious or compromised trusted-indexer endpoint can poison a standalone
read-state mirror's non-finalized view more easily than a normal Zebra peer can
poison the full-node state.

Suggested fix direction:

- Recompute and compare the block hash in `BlockAndHash::decode()`.
- Have `TrustedChainSync::try_commit()` use the same
  `validate_and_commit_non_finalized()` helper as the state service, or call
  `check::initial_contextual_validity()` explicitly before lower-level commit.
- Document that `TrustedChainSync` requires authenticated/local/trusted indexer
  endpoints.

Detailed note:
`docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`.

### Indexer `MempoolChange` local-mempool privacy stream

Result: public privacy / deployment hardening note.

The broad indexer exposure note already documents the unauthenticated optional
tonic server and streaming availability surface. A separate privacy detail is
worth tracking: `MempoolChange` streams the node's local mempool changes,
including V5 authorization digests, to every indexer subscriber.

The indexer feature is not part of default release binaries, and the config
defaults `indexer_listen_addr` to `None`:

- `zebrad/Cargo.toml:52-63`
- `zebra-rpc/src/config/rpc.rs:33-47`
- `zebra-rpc/src/config/rpc.rs:75-83`

When enabled, `zebrad` starts the indexer server with the live
`mempool_transaction_subscriber` and the server is built without auth/TLS:

- `zebrad/src/commands/start.rs:277-287`
- `zebra-rpc/src/indexer/server.rs:57-61`

The protobuf schema exposes `tx_hash` and `auth_digest`:

- `zebra-rpc/proto/indexer.proto:25-45`
- `zebra-rpc/proto/indexer.proto:55-56`

The method implementation sends `tx_id.mined_id()` and `tx_id.auth_digest()` for
each changed transaction:

- `zebra-rpc/src/indexer/methods.rs:154-182`

That matters for V5 transactions because Zebra's `UnminedTxId` type says the
mined ID alone does not uniquely identify unmined V5 transactions, while
`auth_digest()` returns the digest of authorizing data. The same type's
`Debug`/`Display` implementations intentionally redact these IDs, with a comment
that logging unmined transaction IDs can leak sensitive user information:

- `zebra-chain/src/transaction/unmined.rs:93-108`
- `zebra-chain/src/transaction/unmined.rs:111-130`
- `zebra-chain/src/transaction/unmined.rs:170-216`

This does not reveal full transaction bytes by itself, and public gossip already
reveals many transactions. The privacy issue is local vantage point: an exposed
indexer can show what this node saw, when it saw it, what was invalidated, and
the V5 authorization digest for witnessed mempool IDs.

Suggested fix direction:

- Keep `MempoolChange` localhost-only or authenticated.
- Document that the stream exposes local mempool timing and V5 auth-digest data,
  not only generic node state.
- Consider splitting detailed transaction identifiers behind a privileged
  capability if external consumers only need coarse change notifications.

Detailed note:
`docs/analysis/indexer-mempool-change-privacy-note.md`.

### Trusted mirror best-tip forwarding task exits on non-finalized tips

Result: public trusted-mirror robustness hardening note.

This is distinct from the previously documented trusted-sync validation
boundary: no malformed block/hash pair is needed. The optional mirror syncer
subscribes a helper task to the upstream indexer's `ChainTipChange` stream, but
that stream publishes best-tip changes, while the helper looks the streamed hash
up in the mirror's finalized `ZebraDb`.

The upstream indexer is wired to `latest_chain_tip`, which is the non-finalized
best tip when available:

- `zebrad/src/commands/start.rs:277-287`
- `zebra-state/src/service/chain_tip.rs:303-305`

The `ChainTipChange` RPC method awaits `best_tip_changed()` and sends
`best_tip_height_and_hash()`:

- `zebra-rpc/src/indexer/methods.rs:36-53`

The mirror helper subscribes to that stream and then exits permanently if the
hash is not present in finalized storage:

- `zebra-rpc/src/sync.rs:62-70`
- `zebra-rpc/src/sync.rs:101-114`

The code comment says this is intended to let `TrustedChainSync::sync()` send
non-finalized updates. In healthy cases, that separate non-finalized-state stream
does compensate:

- `zebra-rpc/src/sync.rs:136-212`
- `zebra-rpc/src/sync.rs:258-277`

The fragility is that one normal non-finalized best-tip notification ends the
finalized-tip forwarding task for the lifetime of the mirror syncer. If the
non-finalized stream is lagging, broken, or filtered while `ChainTipChange`
remains active, mirror tip metadata can become stale.

Suggested fix direction:

- Keep the chain-tip stream subscription alive when a streamed hash is not in
  finalized storage.
- Or split the indexer API into explicit best-tip and finalized-tip streams so
  `TrustedChainSync` can subscribe to the semantic stream it actually needs.
- Add a mirror-mode regression covering a non-finalized best-tip update followed
  by later finalized-only progress.

Detailed note:
`docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`.

### `getinfo.errors` log disclosure candidate

Result: not a fresh finding.

The context builder re-surfaced `getinfo.errors` as a possible information leak.
The prior Sentry/OpenTelemetry privacy pass already covered this path and
correctly narrowed it: Zebra stores only the tracing event `message` field in
`LAST_WARN_ERROR_LOG_SENDER`, not structured fields such as peer addresses or
error objects.

Evidence rechecked:

- `zebrad/src/application.rs:40-42`
- `zebrad/src/components/tracing/component.rs:527-553`
- `zebra-rpc/src/methods.rs:818-826`
- `zebra-rpc/src/methods.rs:954-993`
- `zebra-rpc/src/methods.rs:3198-3247`

Existing coverage:

- `docs/analysis/sentry-opentelemetry-privacy-note.md`
- `docs/analysis/post-v4.4.0-security-audit-pass-5-findings.md`

### RPC `gettxout` non-atomic state snapshot

Result: public RPC correctness / client-safety hardening note.

`gettxout` builds a single response from several independent service calls. On
the state path, it reads the best block hash, then reads the transaction, then
asks whether the outpoint is spent:

- `zebra-rpc/src/methods.rs:3113-3127`
- `zebra-rpc/src/methods.rs:3129-3135`
- `zebra-rpc/src/methods.rs:3147-3162`

It then combines the earlier `best_block_hash` with the later transaction/spent
view:

- `zebra-rpc/src/methods.rs:3168-3177`

The state side handles those as separate read requests against the current
`latest_best_chain()` snapshot at the time each request is served:

- `zebra-state/src/service.rs:1418-1424`
- `zebra-state/src/service.rs:1718-1722`

So during active sync, finalization, or reorg handling, `bestblock`,
`confirmations`, and output/spent status can describe different local state
snapshots. Zebra already has an inline TODO for the `bestblock` mismatch:

- `zebra-rpc/src/methods.rs:3113-3114`

This is not a consensus issue. It is a client-safety hardening gap for RPC
consumers that treat `gettxout` as an atomic UTXO proof against the returned
`bestblock`.

Suggested fix direction:

- Add a single state request that returns tip hash, output, spent status, and
  confirmations from one coherent snapshot.
- Or retry the multi-request sequence if the tip changes between the first and
  last read.

Detailed note:
`docs/analysis/rpc-gettxout-snapshot-consistency-note.md`.

### RPC `getrawtransaction` non-atomic chain provenance

Result: public RPC correctness / client-safety hardening note.

The follow-up address/transaction-index builder pass surfaced a related but
distinct `getrawtransaction` consistency gap. This path is not about verbose
response size or Orchard action conversion. It is about assembling transaction
provenance fields from separate read-state snapshots.

On the no-`blockhash` path, `getrawtransaction(..., verbose=1)` first fetches
the transaction via `AnyChainTransaction(txid)`, then separately fetches
`BestChainBlockHash(tx.height)`:

- `zebra-rpc/src/methods.rs:1775-1783`
- `zebra-rpc/src/methods.rs:1813-1818`
- `zebra-rpc/src/methods.rs:1830-1841`

Those requests are served independently:

- `zebra-state/src/service.rs:1426-1431`
- `zebra-state/src/service.rs:1591-1594`

If the best chain changes between them, Zebra can return transaction height,
confirmations, and block time from one snapshot, but a block hash at that height
from a later snapshot, while marking the response active.

On the caller-supplied `blockhash` path, Zebra first checks whether the caller's
block contains the transaction and whether that block is in the best chain, then
separately fetches the transaction by `txid`:

- `zebra-rpc/src/methods.rs:1742-1766`
- `zebra-rpc/src/methods.rs:1775-1783`
- `zebra-rpc/src/methods.rs:1784-1810`

The earlier `in_best_chain` flag and caller block hash can be combined with a
later transaction view after a reorg. This can produce stale or internally
inconsistent `blockhash`, `height`, `confirmations`, or `in_active_chain`
metadata. It does not affect consensus validity, but it matters for downstream
systems that use verbose RPC transaction metadata as a low-confirmation
chain-membership signal.

Detailed note:
`docs/analysis/rpc-getrawtransaction-snapshot-consistency-note.md`.

### RPC `getblock <height>` non-atomic transaction provenance and panic variant

Result: private heads-up candidate, with public RPC correctness hardening
as the baseline.

The adjacent block-RPC sweep found a more concrete height-query variant of the
same snapshot-consistency class. In `getblock`, the code comment says Zebra
looks up by block hash so the hash, transaction IDs, and confirmations are
consistent:

- `zebra-rpc/src/methods.rs:1292-1296`

But the transaction request is built from the original caller `hash_or_height`
before `hash_or_height` is shadowed with the resolved header hash:

- `zebra-rpc/src/methods.rs:1260-1284`
- `zebra-rpc/src/methods.rs:1286-1289`
- `zebra-rpc/src/methods.rs:1292-1296`

For a height-based caller, `getblock "H" 1` can therefore return header/provenance
fields for the block at height `H` seen by the header subcall, while the
transaction IDs are read later by height and can come from a different block at
height `H` after a reorg. `getblock "H" 2` has the same shape for full
transaction objects:

- `zebra-rpc/src/methods.rs:1319-1354`
- `zebra-rpc/src/methods.rs:1411-1436`

The sharper variant is a panic in `getblock "H" 2`. `get_block_header()` can
return the zcashd-compatible `confirmations = -1` sentinel if `Depth(hash)`
returns `None` after a reorg between the header read and the depth read:

- `zebra-rpc/src/methods.rs:1497-1515`
- `zebra-state/src/service.rs:1358-1363`

The parent `getblock` verbosity-2 path then converts that signed value to `u32`
with `expect(...)` while constructing per-transaction objects:

- `zebra-rpc/src/methods.rs:1332-1349`

So a height-based verbosity-2 call can panic if the header subcall resolved block
`A`, `A` fell out of the best chain before the depth read, and the later
height-based block read returned another block `B` with transactions at the same
height. This is race-dependent and requires RPC access plus a non-finalized
reorg window, but Zebra sets `panic = "abort"` in dev and release profiles:

- `Cargo.toml:184`
- `Cargo.toml:305`

The state service serves those reads independently against the current best-chain
snapshot for each request:

- `zebra-state/src/service.rs:1385-1388`
- `zebra-state/src/service.rs:1434-1440`

`getblockheader <height>` has a related smaller issue: after resolving the
header/hash/height, it still asks for the Sapling tree using the original
height query before separately computing depth:

- `zebra-rpc/src/methods.rs:1455-1479`
- `zebra-rpc/src/methods.rs:1484-1515`
- `zebra-state/src/service.rs:1499-1505`

Hash-based calls are less exposed because the original transaction request is
already hash-based. The fix is to use the resolved hash for all follow-up
transaction/tree reads, or to move this response assembly into one read-state
request.

Detailed note:
`docs/analysis/rpc-getblock-height-snapshot-consistency-note.md`.

### RPC `getblockchaininfo` mixed snapshots and genesis fallback

Result: public RPC client-safety hardening note.

`getblockchaininfo` builds one JSON object from several independently served
sources. It joins `UsageInfo`, `TipPoolValues`, and
`chain_tip_difficulty(...)`, where the difficulty helper issues a separate
`ChainInfo` read:

- `zebra-rpc/src/methods.rs:1000-1011`
- `zebra-rpc/src/methods.rs:4636-4666`

Those state requests are independent:

- `zebra-state/src/service.rs:1340-1350`
- `zebra-state/src/service.rs:1596-1615`

The final response combines `blocks`, `bestblockhash`, value pools, upgrades,
and consensus branch IDs from `TipPoolValues`; `difficulty` from `ChainInfo`;
and estimated height/progress from the chain-tip watcher:

- `zebra-rpc/src/methods.rs:1021-1034`
- `zebra-rpc/src/methods.rs:1037-1059`
- `zebra-rpc/src/methods.rs:1067-1119`

So during sync, finalization churn, or a non-finalized reorg, the response can
mix tip/value-pool fields from one snapshot with difficulty/progress fields from
another. This is client-safety hardening rather than consensus-sensitive logic.

The sharper fallback issue is that `TipPoolValues` failure does not make the RPC
fail. Zebra substitutes `(Height::MIN, genesis_hash, zero value pools)`:

- `zebra-rpc/src/methods.rs:1021-1029`

And when `chain_tip_difficulty(..., should_use_default = true)` sees a
`ChainInfo` error, it returns default difficulty:

- `zebra-rpc/src/methods.rs:4655-4660`

The behavior is covered by property tests:

- `zebra-rpc/src/methods/tests/prop.rs:370-430`
- `zebra-rpc/src/methods/tests/prop.rs:536-607`

That fallback is reasonable for truly empty state, but if a live node hits
retry-exhaustion/state-churn errors, a successful genesis-like response can
mislead monitoring, orchestration, or wallet middleware that does not retry
successful RPCs.

The focused builder pass eliminated the adjacent `getblocktemplate`,
`z_gettreestate`, `z_getsubtreesbyindex`, and hash-pinned block-info/value-pool
paths as fresh mixed-snapshot findings.

Detailed note:
`docs/analysis/rpc-getblockchaininfo-snapshot-fallback-note.md`.

### Address-index ordering assertion sweep

Result: eliminated as a fresh vulnerability on current evidence.

The RPC layer asserts that address transaction IDs and UTXOs are returned in
chain order:

- `zebra-rpc/src/methods.rs:2066-2088`
- `zebra-rpc/src/methods.rs:2114-2131`

The sharpest variant was a sentinel collision: both methods initialize their
last-seen location to the exact genesis transparent transaction/output location.
If state returned the genesis coinbase address index entry, an ordinary RPC
query for that address would trip a process-fatal assertion in release builds.

Current evidence eliminates that as a normal remote path. Finalized commit skips
genesis UTXO and transparent address-index updates, the address reader's full
range starts at height 1, and the finalized address-transaction iterator depends
on the first indexed UTXO location rather than the unindexed genesis output:

- `zebra-state/src/service/finalized_state/zebra_db/block.rs:641-658`
- `zebra-state/src/service/read/address/utxo.rs:33-37`
- `zebra-state/src/service/read/address/utxo.rs:419-457`
- `zebra-state/src/service/finalized_state/disk_format/transparent.rs:527-558`

Those responses are backed by `BTreeMap` keys over `TransactionLocation` and
`OutputLocation`, and the state helper paths have retry/error branches for
finalization races before reaching their remaining invariant asserts:

- `zebra-state/src/service/read/address/tx_id.rs:34-74`
- `zebra-state/src/service/read/address/tx_id.rs:119-269`
- `zebra-state/src/service/read/address/utxo.rs:111-199`
- `zebra-state/src/service/read/address/utxo.rs:246-400`

I did not find a realistic attacker-controlled input that can make those maps
return out-of-order entries or violate the non-finalized-chain overlap asserts.
The existing address-index bounds note remains the better confirmed finding for
this surface. The RPC sentinels are still worth hardening to `Option` locals so
the RPC layer does not rely on a storage invariant for process safety.

Detailed note:
`docs/analysis/rpc-response-construction-panic-sweep-note.md`.

### Primitive batch verifier failure semantics

Result: eliminated as a fresh private vulnerability; public consensus-plumbing
hardening remains.

The interesting hypothesis was cross-item poisoning: because several primitive
verifiers batch contemporaneous proof/signature checks, one invalid item might
make unrelated valid items in the same batch fail, hang, or surface as an
internal error.

Current evidence eliminates that as a final validation outcome. `tower_fallback`
clones each request before calling the primary service and retries the cloned
request on the fallback service when the primary batch future returns `Err`:

- `tower-fallback/src/service.rs:53-56`
- `tower-fallback/src/future.rs:86-115`

The production Ed25519, RedJubjub, RedPallas, Sapling, and Halo2 verifier
statics all wrap their primary batch services in a single-item fallback:

- `zebra-consensus/src/primitives/ed25519.rs:69-95`
- `zebra-consensus/src/primitives/redjubjub.rs:64-90`
- `zebra-consensus/src/primitives/redpallas.rs:82-108`
- `zebra-consensus/src/primitives/sapling.rs:200-218`
- `zebra-consensus/src/primitives/halo2.rs:133-160`

Sprout Groth16 is not currently batched in production:

- `zebra-consensus/src/primitives/groth16.rs:76-102`

The transaction verifier queues these primitive futures into `AsyncChecks` for
Sprout, Sapling, and Orchard transaction validation:

- `zebra-consensus/src/transaction.rs:1045-1084`
- `zebra-consensus/src/transaction.rs:1091-1150`
- `zebra-consensus/src/transaction.rs:1155-1182`

Worker/channel failures also fail closed: the shared Rayon helper reports a
dropped response channel as an error, and the batch item futures treat missing
watch-channel results as errors rather than success:

- `zebra-consensus/src/primitives.rs:22-49`
- `zebra-consensus/src/primitives/ed25519.rs:150-210`
- `zebra-consensus/src/primitives/redjubjub.rs:145-206`
- `zebra-consensus/src/primitives/redpallas.rs:163-223`
- `zebra-consensus/src/primitives/halo2.rs:222-303`
- `zebra-consensus/src/primitives/sapling.rs:141-165`

Remaining hardening is observability and error taxonomy. Batch item futures emit
`invalid` counters before fallback retries the item individually, so a mixed
valid/invalid batch can overcount invalid primitive checks even when fallback
later accepts the valid neighbors. Some dropped-verifier paths also still panic
with "verifier was dropped without flushing", and several primitive errors are
boxed into `InternalDowncastError` instead of stable consensus error variants.

An independent RepoPrompt builder pass agreed with the integrity conclusion and
called out the availability angle: one invalid primitive item can force
unrelated valid neighbors in the same global batch onto slower single-item
fallback verification. That is bounded by batch sizing and verifier concurrency,
but it can add CPU/latency to unrelated block, mempool, or RPC-submitted
transaction verification. The same pass also flagged the mempool download
stream's spawned-task `expect` as panic-containment hardening:

- `zebrad/src/components/mempool/downloads.rs:215-217`

Detailed note:
`docs/analysis/primitive-verifier-failure-taxonomy-note.md`.

### Queued-block height-index desync

Result: new public state-queue availability hardening finding.

The existing queued-block retention note identified that semantically verified
missing-parent blocks can remain retained after caller timeout. A sharper
secondary-index issue exists in the same queue.

`QueuedBlocks::queue()` indexes every queued block by hash, parent, and height:

- `zebra-state/src/service/queued_blocks.rs:54-80`

`QueuedBlocks::dequeue_children()` removes children selected by parent from the
primary `blocks` map, but then removes the entire height bucket for each
dequeued child:

- `zebra-state/src/service/queued_blocks.rs:93-110`

If two queued blocks share the same height but wait on different parents,
dequeueing one parent's child removes the `by_height` entry for the other child
without removing that other child from `blocks` or `by_parent`. Later
`prune_by_height()` depends on `by_height` to find expired queued blocks:

- `zebra-state/src/service/queued_blocks.rs:131-180`

So the surviving same-height block becomes invisible to finalized-height
pruning and can remain retained until its missing parent arrives or the queue is
cleared wholesale. If finalization later passes the missing parent's height, the
parent can no longer become a forkable non-finalized parent, so the orphaned
queued child can become effectively permanent for that node process.

I temporarily added and removed a targeted regression test that creates two
same-height fake children under different fake parents, dequeues one parent, and
then prunes at that height. It failed as expected:

```text
cargo test -p zebra-state dequeue_preserves_height_index_for_other_parents --lib
```

```text
assertion failed: queue.get_mut(&child2.hash()).is_none()
```

This remains public hardening rather than private disclosure because a queued
block must already be semantically verified, and default public networks require
proof of work before that path. The bug is still real and worth fixing by
removing only the dequeued hash from the height bucket, then deleting the bucket
only if it becomes empty.

Detailed note:
`docs/analysis/queued-block-height-index-desync-note.md`.

### ZIP-235 miner-fee share intermediate overflow panic

Result: conservative private maintainer heads-up / future-activation blocker.

The value-pool and amount-arithmetic pass confirmed a panic in the future
NU7/ZIP-235 miner-fee share check. When compiled with the combined unstable
configuration (`zcash_unstable = "nu7"`, `zcash_unstable = "zip235"`, and
`--features tx_v6`), `miner_fees_are_valid()` computes the minimum ZIP-233
amount as:

- `zebra-consensus/src/block/check.rs:337-344`

That expression uses `((block_miner_fees * 6).unwrap() / 10).unwrap()`.
`Amount<NonNegative> * u64` re-constrains the intermediate product to
`0..=MAX_MONEY`, so any otherwise representable block miner fee above
`MAX_MONEY / 6` makes `block_miner_fees * 6` return `MultiplicationOverflow`:

- `zebra-chain/src/amount.rs:377-394`
- `zebra-chain/src/amount.rs:580-610`

The later subsidy equality check can still be satisfiable for such a fee. For
example, with fee `MAX_MONEY / 2`, ZIP-233 amount `3 * MAX_MONEY / 10`,
transparent coinbase output `MAX_MONEY / 5`, zero subsidy, and zero deferred
amount, the total output formula equals the total input formula. The panic
happens before Zebra reaches that comparison.

I confirmed the combined unstable build is live by running the existing ZIP-233
test successfully:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' cargo test -p zebra-consensus miner_fees_validation_succeeds_when_zip233_amount_is_correct --features tx_v6 --lib
```

I then temporarily added and removed a targeted `#[should_panic]` regression with
the high-fee values above. It passed as expected:

```sh
RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' cargo test -p zebra-consensus miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows --features tx_v6 --lib
```

The same overflow-prone formula appears in V6 coinbase generation:

- `zebra-chain/src/transaction/builder.rs:90-95`
- `zebra-rpc/src/methods/types/get_block_template.rs:811-831`
- `zebra-rpc/src/methods.rs:2512-2544`

Follow-up reachability check: template mode reaches the builder-side expression,
while proposal mode, `submitblock`, and ordinary block sync reach the
consensus-side expression. The ZIP-317 fake-coinbase sizing path is not a direct
overflow trigger because it hardcodes `miner_fee = 1` before calling
`Transaction::new_v6_coinbase()`; the risky builder call is the later real
coinbase generation with aggregate selected mempool fees.

Current default mainnet/testnet impact is limited: the shipped activation lists
do not activate NU7 by default, ordinary all-feature CI snippets do not set the
unstable cfgs, source-controlled release/Docker paths pass feature strings but
do not set `zcash_unstable` RUSTFLAGS, and Docker defaults to
`default-release-binaries`. GitHub repository variable values such as
`RUST_PROD_FEATURES` are not visible locally, so maintainer confirmation is
still needed for non-public release settings. An independent RepoPrompt builder
pass agreed that this is not a default-release vulnerability unless some shipped
artifact enables the combined `nu7 + zip235 + tx_v6` configuration. But this is
consensus-path code for a future network upgrade, and Zebra uses `panic =
"abort"` in dev and release profiles:

- `Cargo.toml:183-184`
- `Cargo.toml:304-305`

Fix direction: compute the 60% fee share in a wider integer type or a shared
checked helper that only constrains the final result back to `Amount`, then use
that helper in both block validation and V6 coinbase generation. Add regression
coverage for fees above `MAX_MONEY / 6`, including `MAX_MONEY / 2`, under the
combined `nu7 + zip235 + tx_v6` configuration.

Detailed note:
`docs/analysis/zip235-miner-fee-share-intermediate-overflow-panic-note.md`.

### GetBlockTemplate high-fee coinbase overflow panic

Result: public RPC/mining hardening note.

The adjacent amount-arithmetic sweep found a current block-template panic with a
similar arithmetic shape, but lower practical security severity. The template
path sums selected mempool fees, checks only that the fee sum itself fits in
`Amount<NonNegative>`, and then constructs standard coinbase outputs:

- `zebra-rpc/src/methods/types/get_block_template.rs:811-812`
- `zebra-rpc/src/methods/types/get_block_template.rs:853-861`
- `zebra-rpc/src/methods/types/get_block_template.rs:873-881`

`standard_coinbase_outputs()` computes `miner_subsidy + miner_fee` and unwraps
the result with an `expect`. If the selected total fee is greater than
`MAX_MONEY - miner_subsidy`, the fee is still individually representable, but
the reward sum overflows the `Amount` constraint and panics.

The ZIP-317 selection path constrains candidate transactions by bytes, sigops,
unpaid actions, dependencies, and fee weighting, not by cumulative fee budget:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:61-143`
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:378-410`

The consensus validation path handles the corresponding arithmetic as an error:

- `zebra-consensus/src/block/check.rs:364-365`

I temporarily added and removed a direct `zebra-rpc` regression using
`miner_fee = MAX_MONEY` on a custom NU6-active test network. It passed as a
`#[should_panic]` test:

```sh
cargo test -p zebra-rpc standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money --lib
```

This is not a consensus-acceptance issue and is not private-disclosure material
on current evidence. A realistic mainnet attacker would need to get an
economically extreme high-fee transaction into the node's mempool and have an
operator/miner call `getblocktemplate`. The bug is still worth fixing by making
coinbase generation fallible and by capping selected total fees to
`MAX_MONEY - miner_subsidy` during template construction.

Detailed note:
`docs/analysis/getblocktemplate-high-fee-coinbase-overflow-panic-note.md`.

### GetBlockTemplate custom pre-Canopy panic

Result: public custom-network mining RPC hardening note.

The focused mining-RPC sweep found one more abort in the template path, but only
for custom activation schedules. `BlockTemplateResponse::new_internal()` unwraps
the result of coinbase/root generation:

- `zebra-rpc/src/methods/types/get_block_template.rs:340`

The helper it unwraps returns an explicit error for pre-Canopy block-template
generation:

- `zebra-rpc/src/methods/types/get_block_template.rs:832`

So a custom Testnet or custom Regtest-style configuration with the next block
height before Canopy activation can turn a `getblocktemplate` or `generate` RPC
call into a process abort. This is not a default public-network issue: default
Mainnet/Testnet are post-Canopy, and Zebra's default Regtest activates upgrades
through Canopy at height 1. The docs already warn that pre-Canopy templates are
unsupported; the bug is returning a panic/abort instead of a JSON-RPC error.

Detailed note:
`docs/analysis/gbt-custom-pre-canopy-panic-note.md`.

### GetBlockTemplate ZIP-317 selection quadratic work

Result: public mining RPC availability hardening note.

`getblocktemplate` fetches the full verified mempool transaction set and passes
it into ZIP-317 selection:

- `zebra-rpc/src/methods/types/get_block_template.rs:777-789`
- `zebrad/src/components/mempool.rs:834-845`
- `zebra-rpc/src/methods.rs:2513`

The selection loop rebuilds a `WeightedIndex` over the whole remaining candidate
list after each selected or rejected candidate:

- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:62-143`
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:201-207`
- `zebra-rpc/src/methods/types/get_block_template/zip317.rs:417-427`

That creates `O(n^2)` weighted-index construction work in the number of mempool
candidates, before root calculation and JSON serialization. The impact is
bounded by the mempool cost limit and requires mining RPC access, so it is not
private disclosure. It is still worth fixing for pool/custom-network operators:
an attacker who can influence mempool shape and repeatedly call
`getblocktemplate` can amplify template-construction CPU.

Follow-up on 2026-05-09 added a focused current-behavior proof:
`independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today`
confirms the selector rebuilds the weighted index over candidate counts
`n, n - 1, ..., 1` across the ZIP-317 candidate partitions.

Detailed note:
`docs/analysis/gbt-zip317-selection-quadratic-note.md`.

### GetBlockTemplate remaining panic sweep

Result: eliminated as a fresh private vulnerability on current default-network
evidence.

The same sweep checked the remaining nearby candidates:

- `calculate_miner_fee()` can panic if handed a synthetic selected transaction
  set whose fees sum above `MAX_MONEY`, but the live path consumes verified
  mempool transactions and the mempool rejects duplicate transparent spends and
  shielded nullifiers before insertion. I did not find a distinct default-network
  path beyond the already documented `miner_subsidy + miner_fee` template
  overflow.
- Fake coinbase generation does not add a new shipped-network panic beyond the
  documented high-fee and ZIP-235 cases. It uses the same network, height,
  address, and extra coinbase data as real generation, and transparent amount
  encoding is fixed-width.
- The history-root `expect("history tree can't be empty")` is not currently an
  attacker-controlled null-root path on default public networks. State supplies a
  chain-history root for post-Heartwood tips and the helper has a special
  activation-block fallback.
- `TransactionTemplate` conversion asserts that mempool transactions are not
  coinbase transactions, but the GBT path gets those transactions from
  `mempool::Request::FullTransactions`, not from raw RPC input.

### State format migration B5 revisit

Result: no new private disclosure item beyond the existing value-pool
maintainer heads-up.

The storage/migration revisit checked the current format-upgrade ordering,
version-marker writes, v27 `BlockInfo` replay, live block commits while
upgrades are running, and disk-format decode panics.

The migration framework marks each upgrade only after its `prepare()`, `run()`,
and `validate()` steps:

- `zebra-state/src/service/finalized_state/disk_format/upgrade.rs:571-592`

The targeted upgrade-order unit test still passes:

```sh
cargo test -p zebra-state format_upgrades_are_in_version_order --lib
```

The v27 block-info/address-received migration writes each height's `BlockInfo`
and transparent address received-balance operands in a single batch:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:155-229`

Live finalized block commits use merge operands while upgrades are unfinished,
so they should not overwrite address-balance updates being built by the
migration:

- `zebra-state/src/service/finalized_state/zebra_db/block.rs:536-549`

The important caveat is the already documented value-pool issue. The v27 replay
path still calls `block.chain_value_pool_change(...).unwrap_or_default()`:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:201-210`

The v27 migration also treats existing `BlockInfo` as authoritative when
resuming, so a previous interrupted run that wrote wrong-but-nondefault
derived data can become the cumulative value-pool baseline for later replay:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:81-87`

The v27 validation checks recent `BlockInfo` presence and recent transparent
received-balance nonzeroness, but it does not recompute exact historical
value-pool deltas:

- `zebra-state/src/service/finalized_state/disk_format/upgrade/block_info_and_address_received.rs:260-303`

There is also a live-read caveat: the database handle is returned after the
background format-change task is spawned, while historical `BlockInfo` reads go
straight to the finalized database. During a long v27 upgrade, callers can see
missing or partial derived metadata until the upgrade reaches those heights:

- `zebra-state/src/service/finalized_state/zebra_db.rs:154-179`
- `zebra-state/src/service/read/block.rs:347-364`

So B5 does not currently add a separate remote exploit path, but it strengthens
the recommendation to fix the value-pool helper, make migration replay fail
loudly instead of writing a zero delta, and avoid exposing derived read
semantics before the relevant format upgrade completes. Disk `FromDisk` panics
still appear to require local malformed RocksDB bytes rather than valid peer/RPC
input.

Detailed note:
`docs/analysis/state-format-migration-b5-revisit-note.md`.

### Feature-gated release paths B7 revisit

Result: public release-engineering hardening / maintainer confirmation item.

The B7 revisit did not find a new private consensus or remote-availability
vulnerability in `default-release-binaries`. That feature set enables
compile-time log filtering, the progress bar, Prometheus, Sentry, and
OpenTelemetry. Sensitive or experimental features such as `indexer`,
`internal-miner`, `elasticsearch`, `filter-reload`, `tokio-console`, `tx_v6`,
and `comparison-interpreter` are outside the default set:

- `zebrad/Cargo.toml:52-58`
- `zebrad/Cargo.toml:62-148`
- `docker/Dockerfile:18`

Important nuance: the `indexer` Cargo feature gates extra state indexing data,
but the gRPC indexer server module itself is compiled and runtime-config gated.
The server starts whenever `rpc.indexer_listen_addr` is configured, and the
startup path is not itself behind `cfg(feature = "indexer")`:

- `zebra-rpc/src/lib.rs:9`
- `zebrad/src/commands/start.rs:277-289`
- `zebra-rpc/src/config/rpc.rs:33-47`
- `zebra-rpc/src/indexer/server.rs:57-61`

This does not create a new B7 private issue; it reinforces the existing indexer
gRPC exposure notes and means "not in default-release-binaries" should be read
as "the extra state-indexing feature is not in the default set," not "the server
module is absent from the binary."

The notable hardening issue is the source-controlled deployment workflow shape.
Official release Docker images use `RUST_PROD_FEATURES`, but the GCP deployment
workflow builds a `runtime` image with both `RUST_PROD_FEATURES` and
`RUST_TEST_FEATURES`:

- `.github/workflows/release-binaries.yml:28-38`
- `.github/workflows/zfnd-deploy-nodes-gcp.yml:250-264`
- `.github/workflows/zfnd-build-docker-image.yml:70-72`
- `.github/workflows/zfnd-build-docker-image.yml:236-240`
- `.github/workflows/zfnd-build-docker-image.yml:262-266`

If `RUST_TEST_FEATURES` includes the same testing features used by CI, deployment
runtime images can compile public test helpers and extra test dependencies.
Static review found public `proptest-impl` exports such as state arbitrary/test
helpers, hidden database-version writers, consensus `init_test()`, and an
address-book constructor documented as able to break invariants:

- `zebra-state/src/lib.rs:27-28`
- `zebra-state/src/lib.rs:70-97`
- `zebra-consensus/src/router.rs:419-435`
- `zebra-network/src/address_book.rs:176-236`

I did not find a runtime path from `zebrad start` into those helpers, so this is
not a standalone vulnerability on current evidence. The unknown is the private
repository variable values: both `gh api` attempts to read `RUST_PROD_FEATURES`
and `RUST_TEST_FEATURES` returned HTTP 403 due to missing repository-variable
permission.

The same B7 pass eliminated several suspected release-bypass shapes:
`debug_skip_format_upgrades` is only honored for read-only or `cfg!(test)` DB
opens; `debug_force_finished_sync` only affects `getblockchaininfo` progress
reporting in the reviewed code; `internal-miner` is compile-gated plus runtime
config gated; and `CheckBlockProposalValidity` validates proposals against a
cloned non-finalized state rather than committing to live state.

Compile smoke checks passed for both `default-release-binaries` and
`default-release-binaries proptest-impl lightwalletd-grpc-tests
zebra-checkpoints`; both selected 0 tests in the binary harness but confirmed
the feature combinations compile locally.

Suggested fix direction: build deployment runtime images with production
features only, keep test features in test Docker targets, and add a CI guard
that prevents production/runtime variables from containing `proptest-impl`,
`comparison-interpreter`, `tx_v6`, `internal-miner`, `filter-reload`, or
`tokio-console` unless an intentional temporary override is documented.

Detailed note:
`docs/analysis/feature-gate-release-b7-revisit-note.md`.

### ZIP-244 sighash hash-type matrix A1 revisit

Result: no new private disclosure item beyond the already-disclosed V5
`SIGHASH_SINGLE` corresponding-output issue.

The A1 revisit split the ZIP-244 transparent sighash matrix into two parts. The
remaining live issue is still the canonical V5 `SIGHASH_SINGLE` /
`SIGHASH_SINGLE|ANYONECANPAY` missing-output shape:

- `zebra-script/src/tests.rs:480-487`
- `zebra-script/src/tests.rs:491-501`
- `zebra-chain/src/primitives/zcash_primitives.rs:480-505`

The adjacent malformed-hash-byte and stale-buffer shapes are eliminated in the
current checkout. The FFI callback now allowlists the six valid ZIP-244 hash
bytes, maps only those values into Zebra's typed hash flags, and returns a fresh
random digest for callback failures rather than relying on `None` propagation
through `libzcash_script`:

- `zebra-script/src/lib.rs:178-190`
- `zebra-script/src/lib.rs:207-218`
- `zebra-script/src/lib.rs:222-236`
- `zebra-chain/src/transaction/sighash.rs:37-49`

Targeted tests passed:

```sh
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x84_rejected --lib
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x50_rejected --lib
cargo test -p zebra-script stale_sighash_buffer_v5_two_checksig_rejected --lib
cargo test -p zebra-script sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
cargo test -p zebra-script sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
```

The first three tests eliminate the invalid-hash-byte / stale-buffer side. The
final two preserve local proof of the already-disclosed missing-output
acceptance.

Detailed note:
`docs/analysis/sighash-hash-type-matrix-a1-revisit-note.md`.

### zebra-script FFI safety A6 revisit

Result: eliminated as a fresh private vulnerability in the reviewed paths;
public defense-in-depth remains.

The FFI wrapper now checks input and previous-output alignment before invoking
the C++ interpreter, rejects coinbase inputs, allowlists valid V5 hash bytes,
and turns callback failure into a random dummy digest so the signature check
fails instead of relying on `None` propagation through `libzcash_script`.
Unknown `libzcash_script` errors remain errors:

- `zebra-script/src/lib.rs:147-153`
- `zebra-script/src/lib.rs:164-172`
- `zebra-script/src/lib.rs:178-190`
- `zebra-script/src/lib.rs:222-236`
- `zebra-script/src/lib.rs:244-251`
- `zebra-script/src/lib.rs:55-58`

The remaining caveats are defense-in-depth rather than new disclosure items:
`SigHasher::sighash()` still unwraps lower precondition errors, and
`p2sh_sigop_count()` still has a debug-assert-plus-`zip()` release truncation
shape for future misaligned callers. Current live alignment was already covered
by the transparent spent-output alignment note.

Targeted tests passed:

```sh
cargo test -p zebra-script is_valid_rejects_mismatched_previous_outputs_length --lib
cargo test -p zebra-script is_valid_rejects_out_of_range_input_index --lib
cargo test -p zebra-script stale_sighash_buffer_v5_two_checksig_rejected --lib
```

Detailed note:
`docs/analysis/script-ffi-safety-a6-revisit-note.md`.

### Anchor and nullifier reorgs A3 revisit

Result: eliminated as a fresh private vulnerability in this pass; public
regression-test hardening remains.

The non-finalized reorg machinery appears to maintain the key shielded-state
invariants reviewed here. Sapling and Orchard anchor checks consult either the
published non-finalized chain snapshot or finalized DB, nullifier checks reject
if any shielded pool's nullifier exists in either source, and fork creation
reverts tips through the same inverse operations that remove trees, anchors,
and nullifiers.

The important proposal-validation caveat is semantic rather than corrupting:
`CheckBlockProposalValidity` validates against a cloned read-time snapshot. It
can therefore disagree with a later live commit after intervening tip changes or
reorgs. That should be documented as an advisory snapshot check, not treated as
a promise that the same proposal will later commit to the same state.

Important evidence:

- `zebra-state/src/service/check/anchors.rs:49-69` and
  `zebra-state/src/service/check/anchors.rs:91-114` check Sapling and Orchard
  anchors against non-finalized and finalized state.
- `zebra-state/src/service/check/nullifier.rs:103-129` checks Sprout, Sapling,
  and Orchard nullifiers across non-finalized and finalized state.
- `zebra-state/src/service/non_finalized_state/chain.rs:400-416` forks by
  cloning and popping tips above the fork point.
- `zebra-state/src/service/non_finalized_state/chain.rs:791-838`,
  `zebra-state/src/service/non_finalized_state/chain.rs:852-908`,
  `zebra-state/src/service/non_finalized_state/chain.rs:991-1042`, and
  `zebra-state/src/service/non_finalized_state/chain.rs:1057-1113` implement
  Sapling/Orchard tree and anchor add/remove symmetry.
- `zebra-state/src/service/non_finalized_state/chain.rs:2074-2105`,
  `zebra-state/src/service/non_finalized_state/chain.rs:2129-2159`, and
  `zebra-state/src/service/non_finalized_state/chain.rs:2177-2207` implement
  Sprout/Sapling/Orchard nullifier add/remove symmetry.
- `zebra-state/src/service/non_finalized_state/tests/prop.rs:623-647`
  includes trees, anchors, and nullifier sets in the forked-vs-pushed internal
  state property comparison.

Targeted tests passed:

```sh
cargo test -p zebra-state service::check::tests::anchors --lib
cargo test -p zebra-state service::check::tests::nullifier --lib
cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib
```

Residual public hardening: `zebra-state/src/service/check/tests/anchors.rs:343`
still has a TODO for direct Orchard anchor coverage. A useful follow-up is an
explicit two-branch reorg regression where an anchor exists only on the
discarded branch and a nullifier spent on the discarded branch is spendable on
the replacement branch. A second useful follow-up is a short contract comment
near `zebra-state/src/service.rs:1655-1692` explaining that proposal validation
is snapshot-based and non-binding for later commit.

Detailed note:
`docs/analysis/anchor-nullifier-reorg-a3-revisit-note.md`.

### Non-finalized spent-output panic reachability

Result: eliminated as a private remote DoS lead.

`Chain` has non-test panic guards if a contextual block's transparent inputs are
missing from its `spent_outputs` map, but the normal external paths build that
map before any `Chain` mutation. `validate_and_commit_non_finalized()` runs the
state contextual path, `NonFinalizedState::validate_and_commit()` calls
`check::utxo::transparent_spend()`, and that helper either returns every spent
UTXO required by transparent inputs or returns typed validation errors such as
`MissingTransparentOutput`, `DuplicateTransparentSpend`, or
`EarlyTransparentSpend`.

Important evidence:

- `zebra-state/src/service/non_finalized_state/chain.rs:1946-1983` and
  `zebra-state/src/service/non_finalized_state/chain.rs:2003-2035` contain the
  forward and revert panic guards.
- `zebra-state/src/service/non_finalized_state.rs:551-607` computes spent UTXOs
  before contextual block construction and chain update.
- `zebra-state/src/service/check/utxo.rs:38-97` and
  `zebra-state/src/service/check/utxo.rs:126-173` reject missing, duplicate, or
  early transparent spends before commit.
- `zebra-state/src/request.rs:473-513` extends contextual `spent_outputs` with
  same-block `new_outputs` before chain indexing.
- `zebra-state/src/service.rs:1655-1692` confirms proposal validation uses the
  same contextual helper on a cloned state.
- `zebra-rpc/src/sync.rs:215-225` skips `initial_contextual_validity()` for the
  trusted mirror path, but still calls lower-level commit methods that converge
  on the same spent-output builder.

Targeted test passed:

```sh
cargo test -p zebra-state service::check::tests::utxo --lib
```

RepoPrompt builder pass `panic-reachability-audit-E5C550` independently reached
the same classification. The adjacent `TrustedChainSync` hash/body and skipped
recent-chain-context concerns remain public hardening, but they do not make this
specific missing-spent-output panic attacker-reachable.

Detailed note:
`docs/analysis/non-finalized-spent-output-panic-reachability-note.md`.

### RPC `text/plain` CSRF hardening

Status: public RPC hardening.

Confidence: medium.

The JSON-RPC HTTP compatibility middleware rewrites requests with missing
`Content-Type` or `Content-Type` starting with `text/plain` to
`application/json` before jsonrpsee handles them:

- `zebra-rpc/src/server/http_request_compatibility.rs:86-124`
- `zebra-rpc/src/server/http_request_compatibility.rs:234-253`

This is useful for old clients, but it also weakens the browser content-type
barrier for auth-disabled RPC deployments. Browser pages can attempt simple
cross-origin `text/plain` POSTs; they cannot read responses under SOP/CORS, but
they may still trigger side effects or expensive work if Zebra is reachable and
cookie auth is disabled.

This composes with docs/examples that disable cookie auth for localhost
lightwalletd or mining compatibility, and with Docker examples that disable
auth while binding/publishing RPC broadly:

- `book/src/user/lightwalletd.md:49-64`
- `book/src/user/mining-testnet-s-nomp.md:52-57`
- `docker/docker-compose.lwd.yml:16-20`
- `docker/docker-compose.observability.yml:28-37`
- `book/src/user/docker.md:91-116`

The strongest distinct case is an operator who believes `127.0.0.1` plus
disabled cookie auth is safe from web-origin traffic. Modern private-network
browser protections may reduce this shape for some public origins, so this
should be treated as hardening rather than a default remote exploit.

Suggested fix direction:

- Do not rewrite `text/plain` to `application/json` when cookie auth is disabled
  unless an explicit compatibility flag is set.
- Require auth for `text/plain` / missing-content-type compatibility.
- Reject unexpected `Origin` or `Referer` headers, or document and enforce an
  allowlist.
- Add warnings to docs that disable cookie auth on localhost.

Detailed note:
`docs/analysis/rpc-text-plain-csrf-hardening-note.md`.

### Custom-network implicit NU6.1 lockbox boundary

Status: public custom-network hardening.

Confidence: medium-high for source behavior, low for default-network impact.

`NetworkUpgrade::activation_height()` falls forward to the next configured
upgrade when the requested upgrade has no explicit activation height:

- `zebra-chain/src/parameters/network_upgrade.rs:340-360`

That fallback composes poorly with NU6.1 one-time lockbox accounting on custom
Regtest/Testnet networks. If a later upgrade such as NU7 is configured and
`nu6_1` is omitted, `NetworkUpgrade::Nu6_1.activation_height(network)` can
return the later upgrade height. The lockbox helpers and block subsidy checks
then treat that height as the NU6.1 activation block:

- `zebra-chain/src/parameters/network.rs:289-324`
- `zebra-consensus/src/block/check.rs:256-267`
- `zebra-rpc/src/methods/types/get_block_template.rs:867-902`

Regtest defaulting leaves `nu6_1` and later upgrades as configured, and missing
lockbox disbursements default to empty:

- `zebra-chain/src/parameters/network/testnet.rs:374-412`
- `zebra-chain/src/parameters/network/testnet.rs:986-1008`

Targeted test passed:

```sh
cargo test -p zebra-chain activates_network_upgrades_correctly --lib
```

This test confirms the fallback primitive: setting only `nu7: Some(1)` makes
earlier upgrades report `Height(1)`. The resulting risk is a custom-network
activation footgun: empty lockbox disbursements can make the inherited NU6.1
height unmineable, while non-empty custom disbursements can be unexpectedly
required in templates and block validation.

Detailed note:
`docs/analysis/custom-network-implicit-nu6-1-lockbox-boundary-note.md`.

### Exact finalized-boundary downloader filter

Status: public resource-hardening.

Confidence: high for the strict-boundary mismatch, low for serious impact.

The state write service finalizes while the best non-finalized chain length is
greater than `MAX_BLOCK_REORG_HEIGHT`, so a steady-state best tip at height `T`
has a finalized tip around `T - MAX_BLOCK_REORG_HEIGHT`:

- `zebra-state/src/service/write.rs:439-445`
- `zebra-state/src/constants.rs:14-31`

Both sync and inbound downloaders compute the lower bound as
`tip_height - MAX_BLOCK_REORG_HEIGHT`, but reject only blocks strictly below
that bound:

- `zebrad/src/components/sync/downloads.rs:427-440`
- `zebrad/src/components/sync/downloads.rs:501-512`
- `zebrad/src/components/inbound/downloads.rs:339-352`
- `zebrad/src/components/inbound/downloads.rs:379-391`

That admits exact-boundary blocks that are likely at the finalized tip height
under the steady-state invariant. Later verifier/state checks should reject
same-height alternates to finalized history, so this is bounded download,
decode, and verification work rather than invalid-block acceptance.

Detailed note:
`docs/analysis/finalized-boundary-downloader-height-filter-note.md`.

### RPC pre-guard HTTP connection retention

Status: public RPC availability hardening.

Confidence: medium-high for middleware ordering, medium for operational impact.

Zebra's RPC compatibility middleware checks cookie credentials before collecting
the request body:

- `zebra-rpc/src/server/http_request_compatibility.rs:234-240`

For authenticated or auth-disabled requests, it then collects and rewrites the
body before calling the inner jsonrpsee service:

- `zebra-rpc/src/server/http_request_compatibility.rs:127-154`
- `zebra-rpc/src/server/http_request_compatibility.rs:245-252`

That matters because jsonrpsee's `ConnectionGuard` is acquired inside the inner
`TowerServiceNoHttp::call()` path:

- `jsonrpsee-server-0.24.10/src/server.rs:1030-1044`

Zebra's HTTP middleware wraps that inner service before Hyper serves the
connection:

- `jsonrpsee-server-0.24.10/src/server.rs:1210-1223`

So the earlier pass-5 statement that jsonrpsee's 100-permit guard bounds RPC
request work needs a caveat: it does not cover Zebra's pre-inner-service body
collection and JSON-RPC compatibility rewriting. Wrong-auth requests are still
rejected before body collection, but auth-disabled or valid-auth clients can
hold slow or large body collection futures before the inner guard is acquired.

RepoPrompt's RPC admission builder pass independently confirmed the eliminated
branches: wrong-auth requests do not reach body collection, JSON parsing, batch
splitting, method metrics/tracing, or method dispatch. The fresh issue is the
pre-inner-service resource boundary, not an auth bypass.

There is also no apparent accept-level semaphore or request/header timeout
before accepted RPC TCP streams are handed to spawned Hyper connection tasks:

- `jsonrpsee-server-0.24.10/src/server.rs:130-154`
- `jsonrpsee-server-0.24.10/src/server.rs:1224-1231`

This does not bypass cookie auth and RPC is disabled by default. It is still
worth hardening for deployments that expose RPC, disable auth for compatibility,
or share credentials with semi-trusted clients.

Detailed note:
`docs/analysis/rpc-pre-guard-http-connection-retention-note.md`.

### ZIP-244 hash-type matrix current-run revalidation

Status: private-finding confidence check; no new disclosure item.

Confidence: high for the current checkout behavior.

I reran the focused A1 tests after the later RPC/P2P passes to avoid relying on
stale notes for the consensus-critical sighash surface. The undefined V5
hash-byte and stale-buffer hypotheses are still eliminated:

```sh
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x84_rejected --lib
cargo test -p zebra-script sighash_divergence_v5_p2pkh_malformed_0x50_rejected --lib
cargo test -p zebra-script stale_sighash_buffer_v5_two_checksig_rejected --lib
```

All three passed.

The already-disclosed canonical missing-output issue remains live:

```sh
cargo test -p zebra-script sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
cargo test -p zebra-script sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
```

Both passed, confirming that current Zebra still accepts the two V5
`SIGHASH_SINGLE` missing-corresponding-output variants exercised by the local
regressions.

Detailed notes:
`docs/analysis/sighash-hash-type-matrix-a1-revisit-note.md` and
`docs/analysis/sighash-single-corresponding-output-finding.md`.

### Peer-address timestamp normalization edge cases

Result: duplicate / eliminated as a fresh issue.

The fresh peer-discovery pass revisited attacker-controlled `addr` timestamps
because `validate_addrs()` deliberately normalizes future last-seen times and
rejects a whole batch if the offset would underflow:

- `zebra-network/src/peer_set/candidate_set.rs:315-323`
- `zebra-network/src/peer_set/candidate_set.rs:462-519`

The underflow case does not panic or corrupt the address book; it clears only the
current peer response before `send_addrs()` writes address-book updates. A
malicious peer can therefore make Zebra ignore that peer's own response by mixing
an extreme future timestamp with an extreme old timestamp, but that does not
delete existing peers or amplify beyond the already documented crawler churn.

The `expect("unexpected missing last seen")` sites in the same function also look
guarded for current wire input. V1 and V2 `addr` deserialization always read a
timestamp and construct `MetaAddr::new_gossiped_meta_addr(...)` with
`untrusted_last_seen`; unsupported `addrv2` entries are filtered before becoming
`Message::Addr` entries:

- `zebra-network/src/protocol/external/addr/v1.rs:110-127`
- `zebra-network/src/protocol/external/addr/v2.rs:151-168`
- `zebra-network/src/protocol/external/codec.rs:615-649`

Unsolicited `addr` messages can fill the per-connection cache, but later
`Request::Peers` responses served from that cache still return through
`Response::Peers(addrs)` and the candidate crawler still calls `validate_addrs()`
before inserting them into the address book:

- `zebra-network/src/peer/connection.rs:1252-1277`
- `zebra-network/src/peer/connection.rs:1028-1043`
- `zebra-network/src/peer_set/candidate_set.rs:315-323`

So the fresh timestamp edge does not add a separate vulnerability beyond the
existing stale-address dial-churn note:

- `docs/analysis/p2p-stale-gossiped-address-dial-churn-note.md`

### Mempool eviction-list invariant panics

Result: eliminated as a fresh attacker-reachable panic on current evidence.

`EvictionList` has process-fatal invariant checks: duplicate insertion into the
unique map asserts, and pruning/popping asserts that the queue and map stay in
sync:

- `zebrad/src/components/mempool/storage/eviction_list.rs:46-68`
- `zebrad/src/components/mempool/storage/eviction_list.rs:101-132`

The duplicate-insertion shape appears guarded by the storage entry points. Before
queueing or inserting a transaction, Zebra checks the rejection lists with
same-effects matching:

- `zebrad/src/components/mempool.rs:861-887`
- `zebrad/src/components/mempool/storage.rs:915-928`
- `zebrad/src/components/mempool/storage.rs:400-424`

Random eviction, expiry, mined removals, and duplicate-spend removals all add
same-effects chain rejections keyed by mined ID, and `rejection_error()` consults
those eviction lists before later download/verify/insert attempts can re-add the
same mined ID:

- `zebrad/src/components/mempool/storage.rs:463-497`
- `zebrad/src/components/mempool/storage.rs:540-613`
- `zebrad/src/components/mempool/storage.rs:771-821`
- `zebrad/src/components/mempool/storage.rs:882-910`

I did not find a peer/RPC path that inserts the same mined ID into the same
`EvictionList` twice without first expiring or evicting the older entry, nor a
path that mutates `ordered_entries` separately from `unique_entries`. The asserts
remain worth hardening if the structure is refactored, but this pass does not
promote them to a new vulnerability.

### Prometheus dynamic-label recheck

Result: duplicate of existing metrics cardinality note; no new label family
found in this recheck.

The current source still has the confirmed attacker-influenced Prometheus label
families documented in `docs/analysis/prometheus-cardinality-security-note.md`:

- peer handshake labels include remote peer address strings and peer-chosen
  `user_agent` values in `zebra-network/src/peer/handshake.rs:760-814`;
- peer connection and codec byte/message metrics include per-connection `addr`
  labels and decode-error strings in `zebra-network/src/peer/connection.rs` and
  `zebra-network/src/protocol/external/codec.rs`;
- mempool verification failures label by `error.to_string()` in
  `zebrad/src/components/mempool.rs:641-661`;
- RPC request metrics label by caller-supplied method name in
  `zebra-rpc/src/server/rpc_metrics.rs:44-86`;
- peer-cache metrics can label actual cached `IP:port` strings through
  `zebra-network/src/config.rs:499-536`.

The broader `metrics::counter!` / `metrics::gauge!` sweep did not add a fresh
attacker-controlled family. State request metrics use fixed `variant_name()`
strings, sync/result labels are small fixed result sets, RocksDB labels are local
column-family/level metadata, and build-info labels are local package metadata:

- `zebra-state/src/request.rs:1050-1082`
- `zebra-state/src/request.rs:1440-1467`
- `zebrad/src/components/sync/downloads.rs:365-398`
- `zebrad/src/components/sync/downloads.rs:542-556`
- `zebra-state/src/service/finalized_state/disk_db.rs:648-677`
- `zebrad/src/components/metrics.rs:21-36`

Classification remains public hardening: when Prometheus metrics are enabled, the
known peer, mempool, and RPC labels can create avoidable high-cardinality series,
but metrics are runtime-disabled by default and the exposed surfaces depend on
operator configuration.

### GetBlockTemplate long-poll max-time already reached

Result: public mining RPC availability/correctness hardening note.

The time-focused follow-up found a template-mode long-poll edge that is not
covered by the earlier proposal-mode verifier timeout note. State can legitimately
return `cur_time == max_time`, because `GetBlockTemplateChainInfo` clamps the
local clock into the valid block-time range:

- `zebra-state/src/service/read/difficulty.rs:214`
- `zebra-state/src/service/read/difficulty.rs:227-239`

The long-poll ID records tip height, a tip-hash checksum, `max_time`, mempool
count, and mempool checksum, but not `cur_time` or "max time already reached":

- `zebra-rpc/src/methods/types/long_poll.rs:68-115`
- `zebra-rpc/src/methods/types/long_poll.rs:198-212`

The `getblocktemplate` loop returns when the generated server ID differs from the
client ID or when the previous loop iteration selected the max-time sleep future:

- `zebra-rpc/src/methods.rs:2328-2359`
- `zebra-rpc/src/methods.rs:2477-2490`

But when `cur_time` has already been clamped to `max_time`, the code explicitly
omits the max-time future:

- `zebra-rpc/src/methods.rs:2388-2397`

So a caller with a matching current `longpollid` can enter a loop where the
server ID remains equal, `max_time_reached` remains false, and each five-second
mempool poll just rechecks the same state. The request then waits for an
unrelated mempool or tip change instead of returning promptly with
`submitold=false`.

This is not consensus-critical and requires mining RPC access with
`miner_address` configured. It is still worth fixing because it contradicts the
documented long-poll behavior that `submitold` is false when max time is reached:

- `zebra-rpc/src/methods/types/get_block_template.rs:209-224`

The suggested fix is to treat `cur_time >= max_time` as already reached for
long-poll requests, returning immediately with `submitold=false` or allowing a
zero-duration max-time future to fire. The missing regression shape is a mocked
state/mempool test where the supplied `longpollid` equals the generated server
ID and `cur_time == max_time`.

Detailed note:
`docs/analysis/gbt-longpoll-max-time-already-reached-note.md`.

### Fixed-index string slicing sibling sweep

Result: eliminated as a fresh externally reachable panic beyond the already
documented `longpollid` issue.

The follow-up searched for fixed byte-index string slicing and length-then-slice
parsers after the confirmed `getblocktemplate` `longpollid` panic. The only
production RPC/P2P candidate with the same shape remains:

- `zebra-rpc/src/methods/types/long_poll.rs:269-285`

Adjacent candidates were checked and did not reproduce the same bug class:

- `WtxId::from_str()` explicitly converts the input string to bytes before
  splitting at byte 64, checks total byte length first, then calls
  `std::str::from_utf8()` on each half instead of slicing the original UTF-8
  string at a possibly non-character boundary:
  `zebra-chain/src/transaction/hash.rs:313-335`.
- Ordinary block and transaction hash parsers delegate to hex decoding without
  fixed string slicing:
  `zebra-chain/src/block/hash.rs:135-139`,
  `zebra-chain/src/transaction/hash.rs:160-171`, and
  `zebra-chain/src/transaction/auth_digest.rs:137-148`.
- `HashOrHeight::new()` parses RPC block identifiers by attempting hash, height,
  and negative-height parsers; it does not slice the input string:
  `zebra-state/src/request.rs:144-178`.
- The other fixed-index string uses surfaced by `rg` are tests, CLI utilities,
  fixed local strings such as `"Testnet"` / `"Mainnet"`, or truncation of
  already-ASCII serialized output in tests/formatting paths.

So this slice did not find a second Unicode slicing panic. The existing
`longpollid` note remains the one confirmed externally reachable member of this
bug class.

### zebra-script FFI callback containment recheck

Result: eliminated as a fresh FFI-safety vulnerability; duplicate of the known
V5 `SIGHASH_SINGLE` missing-corresponding-output issue for the live consensus
bug.

This pass rechecked whether attacker-controlled block, mempool, gossip, or
RPC-submitted transactions can make a Rust panic cross the `libzcash_script`
callback boundary, make callback failure silently verify, or exploit
spent-output/input misalignment beyond the already documented V5
`SIGHASH_SINGLE` issue.

The current live path fails closed before entering the C++ interpreter for the
obvious panic and alignment cases:

- `zebra-consensus/src/transaction.rs:404-418` rejects transactions with missing
  inputs/outputs, mempool coinbase transactions, coinbase transactions with
  non-coinbase contents, and non-coinbase transactions containing coinbase
  inputs before building script checks.
- `zebra-consensus/src/transaction.rs:686-776` preallocates spent-output slots
  by transaction input index, fills best-chain and mempool outputs back into the
  original slot, and returns an error when a mempool outpoint cannot be resolved.
- `zebra-consensus/src/script.rs:61-73` checks the requested input exists before
  calling `cached_ffi_transaction.is_valid(input_index)`.
- `zebra-script/src/lib.rs:147-153` rejects out-of-range `input_index` values
  and mismatched `all_previous_outputs` / `transaction.inputs()` lengths with
  `Error::TxIndex`.
- `zebra-script/src/lib.rs:164-172` rejects coinbase inputs before constructing
  the FFI script verification state.

Callback failure also still appears fail-closed in the current wrapper:

- `zebra-script/src/lib.rs:178-190` allowlists only the six valid V5 ZIP-244
  transparent hash bytes.
- `zebra-script/src/lib.rs:195-204` keeps pre-V5 raw-hash-byte behavior on the
  V4 path.
- `zebra-script/src/lib.rs:207-218` maps valid V5 callback hash types into
  Zebra's typed `HashType` before calling the V5 sighasher.
- `zebra-script/src/lib.rs:222-240` converts callback computation failure into a
  fresh random dummy digest, avoiding both stale-buffer reuse and `None`
  propagation through `libzcash_script`.
- `zebra-script/src/lib.rs:242-252` maps `verify_callback()` errors and false
  script results into Zebra errors.

The remaining caveats are not fresh disclosure items:

- `zebra-chain/src/transaction/sighash.rs:112-124` still exposes a panicking
  `SigHasher::sighash()` API shape for input-index precondition failures. In the
  reviewed live path, upstream checks make that an internal invariant breach
  rather than an attacker-controlled FFI panic route.
- `zebra-script/src/lib.rs:399-423` still has a `debug_assert_eq!()` plus
  `zip()` alignment shape for `p2sh_sigop_count()`. Current callers satisfy the
  invariant, but a future refactor should prefer a checked/fail-closed API.
- The V5 `SIGHASH_SINGLE` missing-corresponding-output acceptance remains real,
  but it is the already disclosed consensus issue: the callback reaches the
  lower sighash computation successfully, and the missing rule belongs in the
  ZIP-244 sighash validation path rather than being a separate FFI callback
  containment bug.

Recommended follow-up remains targeted: add a non-panicking checked sighash path
for the FFI callback, enforce the V5 `SIGHASH_SINGLE` corresponding-output rule
in the lower sighash validation layer, keep the existing random-digest fallback
on checked failure, and optionally replace `p2sh_sigop_count()`'s debug-only
alignment assertion with a checked helper.

### P2P `addrv2` parser cap recheck

Result: eliminated as a fresh parser resource-exhaustion finding in the current
checkout.

This pass rechecked `addrv2` because it combines peer-controlled CompactSize
counts, peer-controlled per-entry address lengths, and unsupported transport
types that Zebra parses only to discard.

The current parser has explicit count and byte caps:

- `zebra-network/src/protocol/external/addr/v2.rs:35-41` defines the ZIP-155
  per-entry address-byte cap as `MAX_ADDR_V2_ADDR_SIZE = 512`.
- `zebra-network/src/protocol/external/addr/v2.rs:278-285` deserializes
  `addr_len` and rejects entries larger than that cap before reading the address
  bytes.
- `zebra-network/src/protocol/external/addr/v2.rs:287-304` reads the bounded
  address bytes, then returns `AddrV2::Unsupported` for unknown network IDs
  without storing the unsupported bytes.
- `zebra-network/src/protocol/external/addr/v2.rs:315-324` implements
  `TrustedPreallocate` as `MAX_ADDRS_IN_MESSAGE`, rather than deriving a larger
  allocation bound from the full protocol message size.
- `zebra-network/src/protocol/external/codec.rs:634-644` deserializes the
  `Vec<AddrV2>`, checks the same `MAX_ADDRS_IN_MESSAGE` protocol cap, and
  filters unsupported entries before returning `Message::Addr`.

So a peer can still make Zebra consume bounded parser work by sending up to 1000
unsupported 512-byte address payloads in one message, but this is capped by the
normal message size, the ZIP-155 per-entry bound, and the address-count bound.
The earlier addrv2 allocation class remains covered by the current caps and
tests, not a fresh continuation finding.

### P2P block/tx/header parse strictness

Result: public P2P hardening. The counted-header subcase is duplicate of an
earlier low-severity conformance note; the block and transaction trailing-byte
subcase is a fresh bounded parser-strictness finding.

The `headers` path still accepts nonzero per-header transaction counts:

- `zebra-chain/src/block/header.rs:144-152` documents `CountedHeader` as a
  header with a transaction-count field that is always zero.
- `zebra-chain/src/block/serialize.rs:110-119` writes zero when Zebra serializes
  counted headers.
- `zebra-chain/src/block/serialize.rs:124-134` reads the incoming count into
  `_transaction_count` and ignores it.
- `zebra-network/src/protocol/external/codec.rs:681-694` caps the outer
  `headers` count at 160, so this is not an allocation issue.

This was already captured in
`docs/analysis/sighash-single-corresponding-output-finding.md:244-252`.

The fresher object-body shape is that the codec accepts extra bytes after a
successfully parsed `block` or `tx` payload:

- `zebra-chain/src/block/serialize.rs:149-163` parses blocks through
  `reader.take(MAX_BLOCK_BYTES)`, where `MAX_BLOCK_BYTES` is 2,000,000.
- `zebra-chain/src/transaction/serialize.rs:768-784` similarly parses
  transactions through `reader.take(MAX_BLOCK_BYTES)`.
- `zebra-chain/src/serialization/zcash_serialize.rs:7-10` sets the P2P message
  body limit to `2 * 1024 * 1024`.
- `zebra-network/src/protocol/external/codec.rs:396-406` accepts and reserves
  message bodies up to that codec limit.
- `zebra-network/src/protocol/external/codec.rs:724-727` maps a parsed
  transaction body into `Message::Tx`.
- `zebra-network/src/protocol/external/codec.rs:777-797` performs transaction
  deserialization through `Transaction::zcash_deserialize(reader)`.
- `zebra-network/src/protocol/external/codec.rs:478-489` computes remaining
  bytes after command-specific parsing but only logs them as `extra data after
  decoding message`, then returns the parsed message.

So a peer can send a valid block or transaction prefix plus trailing junk,
including total `block` or `tx` message bodies above the 2,000,000-byte parser
limit but below the 2 MiB P2P message limit. The parsed object prefix is still
handled by the normal consensus or mempool path, and the junk suffix is
discarded, so this is not invalid-object acceptance. It is a malformed-peer
accounting and protocol-conformance hardening issue.

Suggested targeted fix: reject nonzero counted-header transaction counts in
`CountedHeader::zcash_deserialize`, and require exact body consumption for
`block` and `tx` messages in the codec while leaving the existing generic
extra-byte policy alone for other commands unless the team chooses a broader
tightening.

Detailed note:
`docs/analysis/p2p-block-header-parse-strictness-note.md`.

### GetBlockTemplate time-envelope mismatch

Result: public mining RPC correctness / availability hardening.

The core B4 consensus-time workstream remains eliminated in
`docs/analysis/time-consensus-parity-note.md`: mined transactions use candidate
block time, mempool time-locks use best-chain next median-time-past, and state
contextual validation enforces the strict MTP and MTP+90-minute block-time
rules.

This continuation found an adjacent template-generation family. The
`getblocktemplate` chain-info path derives the miner-visible time envelope in
state:

- `zebra-state/src/service/read/difficulty.rs:202-260` computes template
  `cur_time`, `min_time`, `max_time`, and `expected_difficulty` from the current
  tip.
- `zebra-state/src/service/read/difficulty.rs:227-239` sets `min_time` to
  median-time-past plus one second and `max_time` to median-time-past plus 90
  minutes.

That creates two broad mismatches with later validation:

- `zebra-consensus/src/block.rs:242-245` and
  `zebra-chain/src/block/header.rs:107-126` still reject proposal or
  `submitblock` headers whose time is more than two hours ahead of the node's
  local clock, but the template `maxtime` is not intersected with that local
  future-time ceiling.
- `zebra-state/src/service/check.rs:267-321` applies the MTP+90-minute upper
  bound only when `network.is_max_block_time_enforced(candidate_height)` is true,
  while `zebra-state/src/service/read/difficulty.rs:231-237` applies that bound
  unconditionally when producing the template.

The same path computes candidate difficulty using the next block height:

- `zebra-state/src/service/check/difficulty.rs:130-164` derives
  `candidate_height = previous_block_height + 1`.
- `zebra-state/src/service/check/difficulty.rs:188-203` uses that candidate
  height when deciding whether the candidate is a Testnet minimum-difficulty
  block.

But the Testnet template time-range adjustment chooses the
standard/minimum-difficulty split from the previous block height:

- `zebra-state/src/service/read/difficulty.rs:268-272` names the argument
  `previous_block_height`.
- `zebra-state/src/service/read/difficulty.rs:305-307` passes
  `previous_block_height` to
  `NetworkUpgrade::minimum_difficulty_spacing_for_height(...)`.
- `zebra-state/src/service/read/difficulty.rs:316-323` derives the standard and
  minimum-difficulty time boundary from that spacing.

At a target-spacing activation boundary such as Testnet Blossom, this can make
the returned `mintime` / `maxtime` envelope disagree with the returned `bits`.
Before Blossom the Testnet minimum-difficulty gap is `150 * 6 = 900` seconds;
from the Blossom candidate height onward it is `75 * 6 = 450` seconds
(`zebra-chain/src/parameters/network_upgrade.rs:391-407`,
`zebra-chain/src/parameters/network_upgrade.rs:439-453`).

The result is not invalid-block acceptance: proposal or full block validation
still computes difficulty and time from the candidate height and the node-local
clock. The impact is that a miner using Zebra's mutable `"time"` template field
can pick an advertised time that is invalid for the fixed `bits`, too far ahead
for the same node's local future-time rule, or unnecessarily excluded on
custom/pre-rule test networks.

Suggested targeted fix: derive template bounds from the same contextual
MTP-window helpers used by validation, intersect miner-visible `maxtime` with
the node-local future-time ceiling, keep a separate stable template-expiry
deadline for long-polling, and use `candidate_height = previous_block_height + 1`
for `minimum_difficulty_spacing_for_height(...)` inside
`adjust_difficulty_and_time_for_testnet()`.

Detailed note:
`docs/analysis/gbt-time-envelope-mismatch-note.md`.

### Checkpoint auth-data binding boundary

Result: eliminated for bad-state persistence.

The checkpoint verifier itself does not bind NU5/V5 transaction authorizing data
before queueing a block. `zebra-consensus/src/checkpoint.rs:591-632` validates
height, PoW/difficulty, deferred-pool accounting, and the transaction merkle
root, then returns a `CheckpointVerifiedBlock`. The duplicate-queued-block path
explicitly notes the deferred boundary: signatures, proofs, or scripts could be
different even for the same block hash, because the authorizing-data hash is not
checked until checkpoint blocks reach state
(`zebra-consensus/src/checkpoint.rs:676-683`).

The state path does enforce the NU5 commitment before persistence. The
checkpoint commit request is queued by `zebra-state/src/service.rs:1048-1070` and
sent to the finalized write task by `zebra-state/src/service.rs:556-603`. The
write task calls `FinalizedState::commit_finalized()` in
`zebra-state/src/service/write.rs:270-309`, and the checkpoint branch of
`commit_finalized_direct()` calls
`check::block_commitment_is_valid_for_chain_history(...)` before constructing the
`FinalizedBlock` or calling `db.write_block()`
(`zebra-state/src/service/finalized_state.rs:328-370`,
`zebra-state/src/service/finalized_state.rs:438-443`).

For NU5 onward, that check recomputes
`ChainHistoryBlockTxAuthCommitmentHash` from the previous history-tree root and
the block's `auth_data_root()`, then rejects mismatches
(`zebra-state/src/service/check.rs:184-219`). The auth-data root uses each
transaction's authorizing-data digest for V5 transactions
(`zebra-chain/src/block/merkle.rs:230-324`,
`zebra-chain/src/transaction.rs:274-289`), while the txid merkle root is
documented as not binding V5 authorizing data
(`zebra-chain/src/block/merkle.rs:15-19`).

The full semantic path reaches the same commitment check earlier, before
non-finalized-chain acceptance
(`zebra-state/src/service/non_finalized_state.rs:617-667`). So this is a real
deferred-validation boundary, but not a current persisted-state consensus
vulnerability.

Detailed note:
`docs/analysis/checkpoint-auth-data-binding-note.md`.

### Runtime/config/file/endpoint side-effect sweep

Result: no new private-disclosure item; two new public-hardening composition
notes.

This sweep revisited the runtime surfaces requested after pass 5: tracing filter
reload, metrics and health endpoints, Elasticsearch feature transport,
Docker/env exposure examples, RPC cookie lifecycle, Sentry/OpenTelemetry export,
startup file side effects, and operator-only features that become remotely
reachable through documented config.

Most candidates are duplicates of the pass-5 baseline:

| Surface | Evidence | Disposition |
| --- | --- | --- |
| Tracing filter endpoint unauthenticated listener and unbounded `POST /filter` body | `zebrad/src/components/tracing/endpoint.rs:36-49`, `zebrad/src/components/tracing/endpoint.rs:157-165` | Duplicate of `docs/analysis/tracing-filter-endpoint-security-note.md`. |
| Metrics endpoint listener without Zebra-side admission guards | `zebrad/src/components/metrics.rs:16-43` | Duplicate of `docs/analysis/metrics-endpoint-connection-hardening-note.md`. |
| Health endpoint open-connection/request-timeout gap | `zebrad/src/components/health.rs:303-337` | Duplicate of `docs/analysis/health-endpoint-connection-hardening-note.md`. |
| Elasticsearch TLS validation disabled and panic/assert behavior under the experimental feature | `zebra-state/src/service/finalized_state.rs:171-194`, `zebra-state/src/service/finalized_state.rs:523-550` | Duplicate of `docs/analysis/elasticsearch-feature-transport-and-panic-note.md`. |
| RPC cookie write-before-bind, existing-file mode preservation, and cleanup lifecycle | `zebra-rpc/src/server.rs:94-141`, `zebra-rpc/src/server/cookie.rs:33-69` | Duplicate of `docs/analysis/rpc-cookie-existing-file-permissions-note.md` and `docs/analysis/rpc-cookie-lifecycle-cleanup-note.md`. |
| Sentry/OpenTelemetry export and redaction expectations | `zebrad/src/components/tracing/component.rs:271-312`, `zebrad/src/sentry.rs:133-142` | Duplicate of `docs/analysis/sentry-opentelemetry-privacy-note.md`. |
| Docker RPC public bind with cookie auth disabled | `docker/docker-compose.observability.yml:24-37` and the lwd compose file | Duplicate of `docs/analysis/rpc-docker-unauthenticated-public-bind-note.md`. |
| Startup log/cache directory creation before later bind failure | `docker/entrypoint.sh:14-49`, `zebrad/src/components/tracing/component.rs:102-151` | Operator-local cleanup issue, not a fresh security finding beyond the RPC cookie lifecycle. |

The two fresh deltas are composition/documentation hardening rather than new
implementation vulnerabilities:

- Filter reload plus telemetry export: if `filter-reload` and Sentry or
  OpenTelemetry export are both enabled, `POST /filter` can remotely broaden the
  live tracing filter and therefore increase telemetry volume/content exported
  off-host. This is an extension of the existing unauthenticated filter endpoint
  and telemetry privacy notes, not a private issue. Detailed note:
  `docs/analysis/tracing-filter-reload-telemetry-amplification-note.md`.
- Docker/default observability public binds: pass 5 already covered Docker RPC
  public exposure, but the default Docker config also normalizes
  `0.0.0.0` examples for metrics and health. That widens the reachability of the
  known metrics and health endpoint hardening gaps in copied deployments.
  Detailed note:
  `docs/analysis/docker-default-observability-public-bind-note.md`.

### RPC parameter-to-work recheck

Result: no new private-disclosure item; one existing public-hardening note was
refined.

This sweep revisited RPC request-shape amplification paths in
`zebra-rpc/src/methods.rs` and the method-specific type builders, while
excluding the already-documented address-index, subtree, solution-rate, batch,
`z_gettreestate`, `getrawmempool(verbose)`, `getrawtransaction` V5 blockhash,
and `longpollid` parser issues.

| Surface | Evidence | Disposition |
| --- | --- | --- |
| `getblocktemplate` long-poll state/mempool mismatch retry | `zebra-rpc/src/methods.rs:2317-2325`, `zebra-rpc/src/methods/types/get_block_template.rs:759-793`, `zebrad/src/components/mempool.rs:834-848` | Same family as `docs/analysis/gbt-longpoll-full-mempool-poll-amplification-note.md`, now refined with the immediate-retry subcase. Public mining-RPC hardening, not private disclosure. |
| Verbose Orchard action response construction | `zebra-rpc/src/methods/types/transaction.rs:864-902` | Duplicate of `docs/analysis/rpc-verbose-orchard-action-quadratic-note.md`. Public RPC hardening. |
| `getpeerinfo` and `getnetworkinfo` peer counts | `zebra-rpc/src/methods.rs:2722-2776`, `zebra-network/src/address_book.rs:812-820`, `zebra-network/src/constants.rs:197-204`, `zebra-network/src/constants.rs:315-322` | Eliminated as a fresh finding: the methods map or count recently-live address-book entries with no caller-controlled count parameter, and the address book has age and size bounds. |
| `addnode` request shape | `zebra-rpc/src/methods.rs:3016-3045` | Eliminated: regtest-only, one address per request, and only the `add` command is implemented. |
| Generic verbose `getblock(verbosity=2)` / `getrawtransaction(verbose=1)` work | `zebra-rpc/src/methods.rs:1329-1352`, `zebra-rpc/src/methods.rs:1718-1790` | Eliminated as a standalone item: outside the already-documented Orchard subcase, response construction is mostly linear in consensus-bounded block/transaction size and still gated by RPC access and response limits. |

The GBT refinement is worth preserving because it is sharper than the normal
five-second polling shape. If `fetch_mempool_transactions()` returns `None`
because the mempool's `last_seen_tip_hash` does not equal the state tip, a
long-poll request reaches `continue` before constructing the
`MEMPOOL_LONG_POLL_INTERVAL` sleep. That can repeat `ReadRequest::ChainInfo` and
`mempool::Request::FullTransactions` immediately until the two snapshots align.

This remains public hardening. It requires configured mining RPC access, does
not affect consensus, and is bounded by RPC exposure and server capacity.
Suggested mitigation is a short backoff or wait-on-change path for the mismatch
retry, plus the previously noted waiter caps or shared long-poll snapshot.

### Inbound ephemeral address reconnect candidates

Result: new public P2P peer-discovery hardening finding.

The address-book contract says it should contain Zcash listener addresses, not
the remote socket addresses of inbound connections:

- `zebra-network/src/address_book.rs:45-63`

`ConnectedAddr::InboundDirect` is documented as the OS-provided inbound remote
address whose port is ephemeral, not a listener port
(`zebra-network/src/peer/handshake.rs:159-169`). But
`ConnectedAddr::get_address_book_addr()` returns `Some(addr)` for
`InboundDirect` (`zebra-network/src/peer/handshake.rs:249-267`), and the
successful handshake path sends that address into `MetaAddr::new_connected(...)`
with `is_inbound = true` (`zebra-network/src/peer/handshake.rs:974-988`,
`zebra-network/src/meta_addr.rs:384-392`).

`AddressBook::update()` rejects syntactically invalid outbound addresses and
then inserts the updated entry (`zebra-network/src/address_book.rs:482-499`).
The inbound flag prevents later gossip (`zebra-network/src/meta_addr.rs:707-715`)
but does not exclude the entry from `is_ready_for_connection_attempt()` or
`AddressBook::reconnection_peers()` after the normal recent-update window
expires (`zebra-network/src/meta_addr.rs:630-663`,
`zebra-network/src/address_book.rs:637-654`).

There is also a persistence angle: `AddressBook::cacheable()` filters by recent
activity but not by `is_inbound` (`zebra-network/src/address_book.rs:318-338`),
and the peer-cache updater writes only `meta_addr.addr` to disk
(`zebra-network/src/peer_cache_updater.rs:39-49`,
`zebra-network/src/config.rs:486-518`). If an inbound ephemeral address is still
active at cache time, the cache loses the inbound provenance bit.

I temporarily added a focused proof test,
`inbound_ephemeral_address_becomes_reconnection_candidate_today`, to
`zebra-network/src/address_book/tests/vectors.rs`. It inserted an inbound
`198.51.100.10:49152` `MetaAddr::new_connected(...)` update and asserted that
the same address appears in `reconnection_peers()` after
`MIN_PEER_RECONNECTION_DELAY + 1s`.

Targeted command:

```sh
cargo test -p zebra-network inbound_ephemeral_address_becomes_reconnection_candidate_today --lib
```

Result: the test passed. The temporary source edit was removed after the check.

This is not private-disclosure material on current evidence. A successful
inbound peer can add bounded, likely-useless reconnect work derived from its
transient source port, but inbound handshakes and outbound attempts are limited,
the address book is capped, failures self-heal the candidate, and no consensus
or validated state depends on it.

Suggested mitigation is to make `get_address_book_addr()` return `None` for
`InboundDirect`, and to add defense-in-depth guards excluding `is_inbound`
entries from outbound reconnection and cache persistence.

Detailed note:
`docs/analysis/p2p-inbound-ephemeral-address-reconnect-note.md`.

### Pre-handshake full-body decode refinement

Result: duplicate family, existing public hardening note refined.

The existing unsolicited full-block decode finding covered eager decoding after
a peer connection is established. The handshake path has the same shape before a
peer is fully admitted: `negotiate_version()` calls `peer_conn.next()` while
waiting for `Version` and ignores any non-`version` message
(`zebra-network/src/peer/handshake.rs:694-712`), then repeats the pattern while
waiting for `Verack` (`zebra-network/src/peer/handshake.rs:816-838`). Because
that framed connection uses the normal codec, a full `block` or `tx` message
reaches the eager decode paths before being ignored
(`zebra-network/src/protocol/external/codec.rs:656-658`,
`zebra-network/src/protocol/external/codec.rs:724-726`,
`zebra-network/src/protocol/external/codec.rs:775-819`).

This does not become a private issue because the entire handshake is wrapped in
`HANDSHAKE_TIMEOUT` (`zebra-network/src/peer/handshake.rs:1166-1168`), inbound
handshakes are connection/rate limited, and the global wire body cap still
applies. But it is a sharper pre-admission subcase of the same public
availability hardening class.

Detailed note updated:
`docs/analysis/p2p-unsolicited-block-decode-hardening-note.md`.

### Startup peer-cache ingestion and initial peer fanout

Result: eliminated as a fresh remote finding; minor local hardening only.

This sweep followed the peer-cache persistence angle from the inbound
ephemeral-address note into startup. `Config::initial_peers()` resolves DNS
seed peers, then, outside Regtest, loads disk peers and unions the two sets
(`zebra-network/src/config.rs:257-275`). `load_peer_cache()` currently reads the
entire local cache file with `fs::read_to_string()`, parses every line into a
`PeerSocketAddr`, logs invalid entries, and returns a deduplicated `HashSet`
(`zebra-network/src/config.rs:410-459`). There is no file-size or line-count cap
before that local parse work.

That looked interesting because `limit_initial_peers()` only applies after the
full `config.initial_peers().await` result is built
(`zebra-network/src/peer_set/initialize.rs:475-540`), and it also sends every
valid initial peer to the address-book updater before choosing the limited
startup dial set (`zebra-network/src/peer_set/initialize.rs:509-521`). But the
remote-influenced path is bounded:

- Zebra's own cache writer takes at most `MAX_PEER_DISK_CACHE_SIZE = 75`
  entries before writing (`zebra-network/src/constants.rs:183`,
  `zebra-network/src/config.rs:486-518`).
- The startup dial set is capped to `peerset_initial_target_size`, default 25
  (`zebra-network/src/constants.rs:85-86`,
  `zebra-network/src/config.rs:584`).
- Initial outbound attempts are staggered by
  `MIN_OUTBOUND_PEER_CONNECTION_INTERVAL` in `add_initial_peers()`
  (`zebra-network/src/peer_set/initialize.rs:361-390`), with a regression test
  covering the rate limit
  (`zebra-network/src/peer_set/initialize/tests/vectors.rs:1092-1135`).
- The address book enforces `MAX_ADDRS_IN_ADDRESS_BOOK` through
  `AddressBook::update()` surplus removal
  (`zebra-network/src/address_book.rs:510-545`).

So a peer-cache file manually inflated by a local operator or another local
process can cause startup memory/CPU/log work before the later limits apply,
but the network path that writes the cache through Zebra is capped. This does
not warrant private disclosure. If desired, a public hardening issue could add
a pre-parse byte or line cap in `load_peer_cache()`, plus a warning when the
cache exceeds the writer's maximum size.

Detailed note:
`docs/analysis/peer-cache-startup-ingest-hardening-note.md`.

### RPC response-size enforcement and compatibility rewrite

Result: duplicate/eliminated as a fresh vulnerability; public hardening only.

This recheck focused on whether Zebra's configured RPC response-size cap could
be bypassed by the outer JSON-RPC compatibility middleware. `RpcServer::start()`
sets a fixed request body cap for the compatibility middleware, installs
`HttpRequestMiddlewareLayer`, and then configures jsonrpsee with
`.max_response_body_size(conf.max_response_body_size)`
(`zebra-rpc/src/server.rs:123-153`). The configured default is 50 MiB
(`zebra-rpc/src/config/rpc.rs:69-97`).

The request side is not a fresh issue. `HttpRequestMiddleware::call()` rejects
bad cookie credentials before body collection
(`zebra-rpc/src/server/http_request_compatibility.rs:234-240`), and only then
collects the request body through `Limited::new(body, max_request_body_size)`
before JSON parsing or rewriting
(`zebra-rpc/src/server/http_request_compatibility.rs:127-154`). This leaves the
already-documented pre-header connection-retention note intact, but does not
show a wrong-auth or oversized-body path into expensive request parsing.

The response side remains a bounded extra-work hardening issue, not a response
cap bypass. `response_from_json_rpc_2()` runs after the inner service returns,
collects the full response body, optionally rewrites the top-level JSON-RPC
version envelope, and rebuilds the HTTP body
(`zebra-rpc/src/server/http_request_compatibility.rs:156-174`). That can add an
extra full-body buffer/copy for an already-accepted near-limit response, and
RPC methods can still construct large Rust response objects before jsonrpsee
serializes and enforces the byte cap. But the compatibility rewrite only
changes the top-level envelope, so any post-rewrite growth is constant-size
rather than attacker-amplified by the result payload.

No private disclosure is recommended from this slice. A public hardening issue
could add an explicit post-rewrite size check or response-side regression tests
near the configured limit, but the current evidence does not show an unbounded
wire-response-size bypass or a distinct availability vulnerability beyond the
existing RPC batch/request-side notes.

### Indexer and peer-set lifecycle duplicate recheck

Result: no new private-disclosure item; existing public hardening notes cover
the interesting cases.

The indexer/`TrustedChainSync` items from the pass-5 table were already
documented in dedicated notes:

- `docs/analysis/indexer-grpc-exposure-hardening-note.md`
- `docs/analysis/indexer-idle-stream-disconnect-retention-note.md`
- `docs/analysis/indexer-mempool-change-privacy-note.md`
- `docs/analysis/indexer-non-finalized-state-stream-amplification-note.md`
- `docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`
- `docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`

The source evidence still matches that classification. The optional indexer
server is disabled unless `rpc.indexer_listen_addr` is configured, but when it
is configured it starts a plain tonic server with reflection
(`zebra-rpc/src/indexer/server.rs:45-61`). The streaming methods spawn
per-subscriber tasks and communicate through bounded `mpsc` channels
(`zebra-rpc/src/indexer/methods.rs:38-122`,
`zebra-rpc/src/indexer/methods.rs:154-213`). `TrustedChainSync` still connects
to a caller-supplied HTTP endpoint and commits streamed non-finalized blocks
through its trusted mirror helper (`zebra-rpc/src/sync.rs:43-75`,
`zebra-rpc/src/sync.rs:176-228`). That is a trust-boundary/documentation and
hardening issue, not a fresh current-default remote vulnerability.

I also rechecked peer-set ready/unready cancellation invariants after excluding
the existing notes for `max_connections_per_ip` ban panic, non-contiguous ban
cleanup, lossy misbehavior reports, `notfound` routing, and long-lived unready
availability hardening. The remaining assert/panic sites look like internal
service-contract guards:

- `poll_unready()` removes cancel handles when an unready service becomes
  ready, is banned, or errors (`zebra-network/src/peer_set/set.rs:541-606`).
- `remove()` clears ready services directly or sends the stored cancel handle
  for unready services (`zebra-network/src/peer_set/set.rs:801-814`).
- `push_ready()` asserts the cancel-handle presence matches whether the service
  was previously unready (`zebra-network/src/peer_set/set.rs:819-832`).
- `Client::call()` panics only if a caller invokes `call()` after readiness was
  not reserved, while `PeerSet` removes each ready service before calling it
  and only returns it through `UnreadyService` after `poll_ready()` succeeds
  (`zebra-network/src/peer/client.rs:614-668`,
  `zebra-network/src/peer_set/set.rs:1330-1424`).

Duplicate connections and dropped cancel handles are explicitly handled as
non-panicking unready-service outcomes
(`zebra-network/src/peer_set/set.rs:580-600`). Ban removal and stall removal use
the same `remove()` path, so a remote peer can make Zebra drop that peer's
service, but I did not find a path where remote traffic alone makes
`cancel_handles` and `unready_services` diverge into a process-fatal assertion.

RepoPrompt builder was started for this slice, completed discovery, and then
stalled while generating the report for several minutes. I terminated the stuck
`rp-cli` process and kept the manual elimination above rather than treating the
helper as evidence.

### Sync fanout task panic and response-contract recheck

Result: eliminated as a fresh remote-panic finding; existing sync steering
hardening note still applies.

This recheck focused on panic-looking sync paths in `obtain_tips()` and
`extend_tips()`. Both methods spawn fanout `FindBlocks` calls, then panic if a
spawned task itself panics or mark any successful non-`BlockHashes` response as
unreachable (`zebrad/src/components/sync.rs:720-833`,
`zebrad/src/components/sync.rs:884-985`).

I did not find a peer-controlled response shape that reaches those panic arms.
The internal request contract says `Request::FindBlocks` returns
`Response::BlockHashes` (`zebra-network/src/protocol/internal/request.rs:93-112`),
and the peer connection handler only finishes a `FindBlocks` request when it
receives an all-block `inv`, which it translates to `Response::BlockHashes`
(`zebra-network/src/peer/connection.rs:402-409`). Empty, malformed, unrelated,
or non-block peer messages either remain ignored until timeout or are routed as
ordinary inbound messages; they do not become another successful response
variant. The peer-set stall tracker also treats empty `BlockHashes` and errors
as stalls, but non-empty junk hashes still clear the stall counter
(`zebra-network/src/peer_set/set.rs:175-183`). That latter behavior is the
already-documented `sync-findblocks-junk-hash-steering-note.md`, not a new
panic.

The spawned task panic propagation appears to depend on an internal Tower
service invariant violation rather than directly on peer bytes. `Client::call()`
panics if called without prior readiness reservation
(`zebra-network/src/peer/client.rs:639-668`), but the sync path obtains
readiness before each fanout `call()`
(`zebrad/src/components/sync.rs:720-726`,
`zebrad/src/components/sync.rs:884-890`), and the block downloader follows the
same sequential ready-then-call pattern before spawning verification work
(`zebrad/src/components/sync/downloads.rs:338-350`). A malicious peer can return
empty responses, junk non-empty `inv` responses, malformed messages, timeout, or
`notfound`-style errors, but this pass did not find a way for those inputs to
make the spawned service future panic.

No private-disclosure item from this slice. A public hardening issue could
still replace the sync `expect`/`unreachable!` arms with typed errors to reduce
blast radius if a future refactor violates the request/response contract, and
could tighten stall classification so non-empty junk `FindBlocks` responses do
not clear peer stall history.

### V5 shielded parser allocation and Orchard flag recheck

Result: eliminated as a fresh vulnerability.

This slice rechecked whether V5 Sapling or Orchard shielded-data parsing still
has a late-rejection allocation gap, flag-consensus mismatch, or parser panic
similar to the older coinbase Sapling-spend class.

The V5 Sapling parser reads `nSpendsSapling`, detects coinbase transactions from
the transparent input set, and rejects nonzero Sapling spends before allocating
spend prefixes (`zebra-chain/src/transaction/serialize.rs:198-223`,
`zebra-chain/src/transaction/serialize.rs:1019-1033`). It also returns `None`
before reading value balance, shared anchor, proofs, or binding signature when
both spend and output counts are zero
(`zebra-chain/src/transaction/serialize.rs:225-235`). The later
`expect("checked spends ...")` conversions are guarded by the same
count-derived branch, so I did not find a malformed peer transaction that turns
those into a parser panic (`zebra-chain/src/transaction/serialize.rs:341-352`).

For Orchard, the parser reads `nActionsOrchard` before any optional Orchard
fields and returns `None` when the action count is zero
(`zebra-chain/src/transaction/serialize.rs:420-430`). If actions are present,
reserved `flagsOrchard` bits are rejected by `Flags::from_bits()`
(`zebra-chain/src/orchard/shielded_data.rs:218-280`), and the action/signature
pairing uses the already-parsed action count when reading spend-auth signatures
(`zebra-chain/src/transaction/serialize.rs:440-477`). The separate consensus
rules for "at least one Orchard flag when actions exist" and "coinbase must not
set enableSpendsOrchard" are enforced in transaction checks
(`zebra-consensus/src/transaction/check.rs:140-188`).

The transaction parser still calls `tx.to_librustzcash(network_upgrade)?` after
assembling V5 data (`zebra-chain/src/transaction/serialize.rs:1040-1050`), which
keeps the broad librustzcash parser backstop described in the existing V5
sighash/parser notes. This pass did not find a second V5 parser-only acceptance
gap beyond the already-reported `SIGHASH_SINGLE` corresponding-output issue.

No private disclosure from this slice. The useful public follow-up remains
defense in depth: add targeted parser tests around zero-action Orchard bundles,
reserved Orchard flags, and V5 coinbase Sapling spends so these boundaries stay
locked during future transaction-format changes.
