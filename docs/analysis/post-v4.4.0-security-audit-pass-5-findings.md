# Post-v4.4.0 Security Audit Pass 5 Findings

Date: 2026-05-02

Scope: execute `docs/analysis/post-v4.4.0-security-audit-pass-5-plan.md` against the
local Zebra checkout after the initial private disclosure of the V5
`SIGHASH_SINGLE` issue.

## Executive Summary

Pass 5 did not find a new private-disclosure consensus divergence beyond the V5
`SIGHASH_SINGLE` / `SIGHASH_SINGLE|ANYONECANPAY` missing-corresponding-output
case already sent to ZF.

It did find one future-gated consensus-path crash candidate worth a conservative
private maintainer heads-up before NU7/ZIP-235 ships: with the combined
`zcash_unstable = "nu7"`, `zcash_unstable = "zip235"`, and `tx_v6`
configuration on an NU7-active network, the ZIP-235 miner-fee share check can
panic on high but otherwise representable block miner fees.

The existing V5 finding remains high confidence: local `zebra-script` tests show
the script interpreter currently accepts the transaction shape, and local
`zebra-consensus` tests show the full transaction verifier also accepts it.
Current zcashd source rejects the same missing-output shape before computing the
ZIP-244 digest.

The pass did produce availability and hardening leads:

- Prometheus metrics use attacker-influenced values as labels in peer handshake,
  mempool verification, and RPC metrics paths. This is a potential metrics
  cardinality/memory pressure issue when metrics, RPC, and/or mempool are
  enabled, but it is not a consensus divergence.
- RPC tracing also records the raw JSON-RPC method name as the `rpc.method`
  span attribute. The request body cap bounds any single method value, but
  unknown methods should still be normalized before metrics or trace export.
- Default release binaries compile Sentry and OpenTelemetry support, but both
  exporters require explicit runtime configuration before telemetry leaves the
  node. The privacy hardening gap is documentation/redaction: Sentry opt-in
  includes WARN/ERROR logs, INFO breadcrumbs, panic reports, and tracing fields,
  while OpenTelemetry defaults to 100% sampling once an endpoint is configured.
- The experimental `elasticsearch` feature is not part of default release
  binaries, but when compiled it unconditionally enables block indexing in the
  read-write state service, disables TLS certificate validation, and can panic
  on bulk request/response errors from the configured Elasticsearch endpoint.
- The opt-in Prometheus metrics endpoint uses the dependency-provided HTTP
  listener without a Zebra-side allowlist, open-connection cap, or request/header
  timeout. This is only relevant when operators bind `metrics.endpoint_addr`.
- RPC `submitblock` and `getblocktemplate` proposal mode call the block verifier
  without the timeout used by sync and inbound verification. Local tests now
  cover mocked pending-verifier hangs for both RPCs and a real
  checkpoint-verifier wait path for an out-of-order checkpoint-era block via
  `submitblock`.
- The optional tracing filter-reload endpoint is unauthenticated and collects
  `POST /filter` request bodies without an endpoint-level body size limit. This
  is disabled by default and not in `default-release-binaries`.
- RPC batch requests inherit jsonrpsee's unlimited batch-count default. Zebra's
  request body limit, response body limit, auth default, and connection cap are
  meaningful mitigations, but a configured RPC endpoint can still be driven
  through many method dispatches and high-cardinality method-label metrics in
  one HTTP request.
- Zebra's RPC HTTP compatibility middleware runs before jsonrpsee's inner
  `ConnectionGuard` permit is acquired. Wrong-auth requests are rejected before
  body collection, but auth-disabled or valid-auth clients can still hold
  request-body collection/rewrite futures, and headerless TCP streams can retain
  spawned Hyper connection tasks before any request guard runs.
- RPC HTTP compatibility accepts `text/plain` request bodies and rewrites them
  to `application/json`. This preserves legacy client compatibility, but it
  also permits browser-simple cross-origin POSTs against auth-disabled local or
  otherwise reachable RPC endpoints. SOP/CORS still blocks reading responses,
  and modern browser private-network protections may reduce practical exposure,
  so this is public RPC hardening rather than a default remote exploit.
- `getblocktemplate` `longpollid` parsing checks byte length and then slices the
  UTF-8 string at fixed byte offsets. A non-ASCII 46-byte value can panic during
  RPC parameter deserialization. Because Zebra's dev and release profiles use
  `panic = "abort"`, this is process-fatal when an attacker can reach RPC.
- `z_listunifiedreceivers` decodes Unified Addresses structurally, then unwraps
  Sapling receiver semantic validation. A syntactically valid Unified Address
  containing length-valid but invalid Sapling receiver bytes can panic the RPC
  method. With Zebra's aborting panic profile, this is process-fatal when an
  attacker can reach RPC.
- Height-based `getblock <height> 2` can combine a header/depth view of one
  non-finalized block with a later height-based block lookup after a reorg, then
  panic while converting the zcashd-compatible `confirmations = -1` sentinel to
  `u32` for verbose transaction objects. A local mock-state proof test confirms
  the panic path; practical exploitability depends on RPC access and timing a
  non-finalized reorg or best-chain switch affecting the queried height.
- A custom Regtest/Testnet with NU7 activation can make GBT coinbase generation
  construct a V5 transaction tagged `Nu7` in normal non-`tx_v6` builds. Serializing
  that transaction panics because the NU7 placeholder branch ID is test-only.
  This is custom-network/RPC availability hardening, not a mainnet/testnet
  consensus issue.
- On custom Regtest/Testnet configurations, omitting `nu6_1` while configuring a
  later upgrade can make `NetworkUpgrade::Nu6_1.activation_height()` inherit the
  later height. NU6.1 lockbox helpers and subsidy checks then treat that later
  height as the one-time lockbox activation boundary, which can make custom
  block production unexpectedly unmineable or require unintended disbursements.
- Zebra intentionally skips direct pre-Heartwood `hashLightClientRoot` equality
  validation for Sapling/Blossom blocks because production networks are covered
  by mandatory checkpoints. This is eliminated for default Mainnet/Testnet and
  configured Testnet, but custom Regtest can create a pre-Heartwood
  Sapling/Blossom interval without equivalent checkpoint coverage. Canonical
  commit has a mandatory-height assert, but proposal validation can false-accept
  the unsupported pre-Heartwood shape and semantic routing can become a
  custom-network abort. That is custom-network hardening, not a private
  mainnet/testnet disclosure.
- RPC cookie writing creates new files with `0600` permissions and rejects an
  already-present symlink, but does not tighten permissions when rewriting an
  existing regular `.cookie` file. A stale loose cookie file can therefore keep
  exposing fresh RPC credentials to local readers.
- RPC cookie lifecycle cleanup is not wired into the live `RpcServer::start()`
  path. A fresh cookie remains on disk if auth-enabled startup fails after the
  write, and `zebrad` aborts the returned server task on shutdown without
  invoking the unused `RpcServer` cleanup methods.
- RPC `invalidateblock` can panic when the target hash is the root block of a
  non-finalized chain. The root-invalidation branch calls `BTreeSet::remove` on
  the stored chain, making `Chain::cmp` compare equal tip hashes and hit its
  duplicate-tip `unreachable!`.
- RPC `invalidateblock` can panic the state write task when called on two
  competing same-height non-finalized fork tips that share a parent. The first
  invalidation inserts the shared parent-only chain; the second invalidation
  tries to insert the same parent-only chain before sibling cleanup runs,
  reaching `Chain::cmp`'s duplicate-tip `unreachable!`.
- RPC `reconsiderblock` can leave a successfully reconsidered invalidation
  record in the live non-finalized state because it removes from a cloned
  `invalidated_blocks` map. A repeated `reconsiderblock` for the same hash can
  replay the restored blocks again and hit the same duplicate-tip invariant.
- Some shipped Docker examples bind JSON-RPC to `0.0.0.0`, disable cookie auth,
  and publish the RPC port on the Docker host. This is an operational exposure
  footgun that turns several public RPC hardening leads into remote surface for
  copied deployments.
- Mempool `TransactionsById` currently matches V5 requests by mined transaction
  ID, not full witnessed transaction ID. A peer can request a `MSG_WTX` with the
  correct mined ID but the wrong auth digest and still receive the mempool
  transaction. This is P2P/mempool exactness hardening, not consensus.
- RPC `getrawtransaction(txid, verbose, blockhash)` validates the named block by
  mined `txid` and then fetches the transaction body through a separate
  any-chain lookup by the same mined ID. For V5 sibling-chain variants with the
  same mined ID but different auth digest, the response can pair the caller's
  block hash with raw transaction/authdigest data from another chain. This is
  public RPC exactness hardening.
- Verbose RPC response builders have lower-severity correctness mismatches:
  mempool `getrawtransaction` emits `in_active_chain: false` without a block
  context, non-Orchard transactions still get an `orchard` object, Orchard
  action signatures are recovered by equality search instead of positional
  pairing, and verbose mempool descendant fields use direct dependents rather
  than transitive descendants. This is public RPC compatibility hardening.
- Mempool cascading removals can remove dependent transactions without reporting
  or rejecting the full removed set. In the sharp insertion-time shape, random
  eviction can choose an ancestor of a newly inserted transaction, remove the
  new dependent, and still return `Ok(new_txid)`, causing an `added` mempool
  notification for a transaction that is no longer stored.
- `getblocktemplate` transaction metadata still serializes every non-coinbase
  transaction with `depends: []`, even though Zebra now tracks mempool
  dependencies and ZIP-317 selection can include a child transaction once its
  parent is selected. This is mining RPC compatibility hardening, not consensus.
- `getblocktemplate` and proposal validation bypass the near-tip sync gate on
  every test network, even though the helper documents a narrower
  Proof-of-Work-disabled exception. Default Testnet has PoW enabled, so this is
  public mining RPC readiness hardening.
- `getblocktemplate` can panic while building standard coinbase outputs if the
  selected mempool miner-fee sum is individually valid but overflows when added
  to the current miner subsidy. The verifier rejects the same arithmetic class
  cleanly, so this is RPC/template availability hardening.
- In future NU7/ZIP-235-capable builds, both block validation and V6 coinbase
  construction compute the required ZIP-233 miner-fee share as
  `((miner_fees * 6).unwrap() / 10).unwrap()`. Fees above `MAX_MONEY / 6` can
  overflow the intermediate `Amount` multiplication even though the intended
  60% result is valid. This is not reachable in current default release builds
  or default Mainnet/Testnet activations, but it should be fixed before
  NU7/ZIP-235-capable artifacts or custom networks rely on it.
- `getblocktemplate` can advertise a mutable block-time envelope that disagrees
  with later proposal/`submitblock` validation: `maxtime` is not intersected with
  the node-local two-hour future-time rule, the MTP+90-minute cap is applied in
  template generation even when contextual validation would not enforce it, and
  the Testnet minimum-difficulty time split uses previous-height spacing at a
  target-spacing activation boundary. This is mining RPC correctness hardening,
  not consensus acceptance.
- `getblocktemplate` long polling re-fetches state chain info and requests
  `FullTransactions` from the mempool on every matching waiter every five
  seconds. `FullTransactions` clones the verified transaction vector and
  dependency map, so callers with mining RPC access can make work scale with
  active long-poll waiters times mempool size. This is public availability
  hardening, not a private consensus issue.
- `getblocktemplate` long polling can also miss the max-time return condition
  when the caller's `longpollid` still matches current state and the template
  `curtime` is already clamped to `maxtime`. The zero-duration max-time future
  is omitted, so the request can remain pending until a later mempool or tip
  change rather than returning promptly with `submitold=false`.
- P2P transaction `getdata` requests forward the whole requested transaction-ID
  set to mempool before any transaction-count cap. Existing message-size,
  timeout, and load-shed limits reduce impact, but this is an availability
  hardening gap compared with the block `getdata` path's pre-lookup cap.
- P2P `getblocks` / `getheaders` block locators are bounded by overall protocol
  message size, but not by an explicit locator-count cap before state searches
  for a chain intersection.
- Sync `FindBlocks` / `FindHeaders` responses are bounded, but non-empty junk
  responses still clear peer-set stall tracking and can steer the initial block
  download order before honest peer hashes if the attacker responds first.
  Local proof test
  `obtain_tips_queues_fast_junk_hashes_before_later_honest_hashes_today`
  confirms the `obtain_tips` ordering behavior.
- P2P `notfound` handling has two request-correlation gaps: unsolicited
  `notfound` can self-mark the sending peer as missing inventory, and unrelated
  in-flight `notfound` can complete an active block/transaction download
  request as missing.
- A single-item inventory download panic hypothesis was eliminated for real peer
  traffic: block and transaction download tasks contain internal `expect()` /
  `unreachable!()` assertions, but the peer connection and peer-set layers
  convert missing single-item inventory into errors before those tasks see a
  `Missing` status.
- A peer response-sender panic hypothesis was eliminated for real peer traffic:
  the peer set only calls ready clients, the connection task only accepts new
  client requests while awaiting a request, and every remote-influenced exit
  from an in-flight response sends, cancels, or shutdown-flushes the response
  sender.
- Peer-set connection admission, crawler demand, and inventory tracking appear
  bounded in the reviewed production paths, but peers that remain unready by
  keeping their connection busy with inbound messages are not explicitly evicted
  by age. This is bounded P2P availability hardening, not consensus.
- Zebra ignores BIP37 filter messages, but still parses and consumes them. The
  `filteradd` parser truncates oversized bodies to 520 bytes and accepts the
  remaining body as extra data, while ignored filter messages bypass inbound
  service overload handling. This is public P2P parser/rate-limit hardening.
- P2P `headers` messages accept nonzero counted-header transaction counts, and
  P2P `block` / `tx` messages accept a valid object prefix followed by trailing
  junk bytes. Consensus or mempool checks still operate on the parsed object, so
  this is bounded protocol-strictness and malformed-peer-accounting hardening.
- The P2P codec reserves capacity for the entire declared message body after
  parsing only the 24-byte header. A peer can send a valid header declaring the
  2 MiB maximum body, then stall before sending any body bytes. This is bounded
  by connection limits and timeouts, but it is a public P2P memory-pressure
  hardening issue.
- Full `block` and `tx` messages are eagerly deserialized by the P2P codec
  before the connection state machine knows whether they are requested or useful.
  Unsolicited full blocks are ignored after decode and do not enter inbound
  overload handling. This is bounded public P2P availability hardening.
- Sync and inbound downloaders drop blocks strictly below
  `tip_height - MAX_BLOCK_REORG_HEIGHT`, but admit blocks at the exact boundary.
  In steady state that boundary is likely the finalized tip height, so later
  verifier/state layers should reject alternate finalized-history blocks, but
  the early filter can still allow bounded stale block download/decode work.
- P2P `getaddr` handling uses a global cached response in the normal non-empty
  path, but empty refreshes do not advance the refresh deadline. A stale or
  isolated node with a large retained address book and no gossipable peers can
  rescan the address book on every inbound `getaddr`.
- Address-book ban cleanup assumes same-IP entries are contiguous in
  reconnection-priority order. That can leave stale banned-IP entries in the
  address book after a ban. The ban map still blocks the important peer-set
  paths, so this is public cleanup hardening rather than a private ban bypass.
- A Sapling validating-key parser panic hypothesis was eliminated. Malformed V4
  and V5 Sapling `rk` bytes first pass through `redjubjub` canonical point
  decoding before Zebra's follow-up small-order check reaches its internal
  `unwrap()`. A new property test locks in that dependency invariant. Replacing
  the `unwrap()` with explicit error propagation remains public hardening.
- A separate Sapling `TransmissionKey::try_from([u8; 32])` public API path does
  panic on malformed Jubjub bytes before returning `Err`, contrary to its local
  contract. I did not find current node, consensus, mempool, or RPC reachability;
  treat as library API hardening unless a live untrusted-byte call site appears.
- A V5 transaction parser backstop hypothesis was eliminated for the existing
  `SIGHASH_SINGLE` finding. Zebra's `to_librustzcash()` round trip catches
  structural transaction-encoding errors, but the missing-corresponding-output
  rule lives in transparent script sighash validation. The lower
  `zcash_transparent` signable-input constructor only bounds-checks the input
  index, and `zcash_primitives` computes an empty-output digest for missing
  `SIGHASH_SINGLE` outputs. See
  `docs/analysis/v5-sighash-parser-backstop-note.md`.
- The opt-in indexer gRPC server exposes unauthenticated reflection and
  long-lived streaming methods without server-level tonic concurrency/stream
  limits. This is documented as unsafe to bind publicly, but should be hardened
  if used on shared networks.
- Those indexer stream tasks also only notice client disconnects when the next
  source event makes them attempt a send. Idle disconnected
  `ChainTipChange`, `NonFinalizedStateChange`, and `MempoolChange` subscribers
  can therefore retain spawned tasks until the next relevant chain or mempool
  event; the non-finalized-state stream also keeps its state-side listener task
  alive.
- The opt-in indexer `MempoolChange` stream also exposes this node's local
  mempool timing plus V5 authorization digests to every unauthenticated gRPC
  subscriber if `rpc.indexer_listen_addr` is enabled and reachable. This is
  privacy/deployment hardening, not consensus.
- `TrustedChainSync` imports non-finalized blocks from an indexer gRPC endpoint
  into a read-state mirror, but the current path trusts the streamed hash and
  lower-level commit path rather than recomputing `hash == block.hash()` and
  using the normal recent-chain contextual gate. This is public trusted-indexer
  boundary hardening, not default-node consensus.
- In the same trusted-mirror helper, the background best-tip forwarding task is
  unsupervised and exits permanently when an upstream best-tip hash is absent
  from the local finalized DB. Normal active-chain best tips are often
  non-finalized, so this is public mirror robustness hardening.
- Address-index JSON-RPC methods accept caller-supplied address sets and, for
  `getaddresstxids`, whole-chain default height ranges without method-level
  address-count, range-width, or returned-item caps.
- `getnetworksolps` and the deprecated `getnetworkhashps` alias accept
  `num_blocks = i32::MAX`, then ask state to walk ancestor headers until genesis.
  This is a configured-RPC CPU/database-read amplification hardening issue.
- `z_getsubtreesbyindex` treats explicit `start_index + limit` overflow the same
  as omitted `limit`, producing an unbounded subtree range. The `u16` subtree
  index type keeps practical severity low, so this is correctness/future-proofing
  hardening.
- The optional health endpoint has a per-interval accept counter but no global
  open-connection cap or request/header timeout. The counter is an accepted
  socket counter, not a handled-request counter, so keep-alive requests can
  share one counter unit. Repeated `/ready` probes can also amplify a WARN log
  while the node has no chain-tip estimate.
- Mempool `PendingOutputs` stores one broadcast sender per missing transparent
  outpoint waited on by transaction verification, and only prunes closed waiters
  on selected storage cleanup paths. A peer can likely create many unique
  missing-output waits over time through otherwise standard mempool candidates.
- `Block::chain_value_pool_change()` uses `flat_map` over
  `Transaction::value_balance()` results, which suppresses transaction-level
  `ValueBalanceError`s instead of propagating them. Normal semantic block
  verification appears to catch realistic remote cases earlier, but this is a
  consensus-adjacent state-accounting correctness bug and should be sent as a
  private maintainer heads-up.
- Value-pool migration/replay code also converts
  `chain_value_pool_change()` failure into a zero delta with
  `unwrap_or_default()`. That should fail loudly instead of writing derived
  `BlockInfo` from incomplete value accounting.
- Queued missing-parent block UTXOs can influence block/proposal semantic
  verification through `AwaitUtxo`, because the queued UTXO cache is global
  across the non-finalized queue. This does not appear to bypass state commit:
  contextual validation rebuilds spent UTXOs from the selected parent chain plus
  finalized state. The mempool path is eliminated because it uses
  `UnspentBestChainUtxo` and mempool `AwaitOutput`, not `AwaitUtxo`.
- The prior inbound misbehavior scoring downcast finding remains a public
  robustness issue.
- Direct pushed P2P `tx` messages drop source-peer metadata before mempool
  verification, so score-bearing invalid pushed transactions are rejected but
  cannot increase peer misbehavior score. This is public P2P/mempool hardening,
  not consensus.
- A tiny P2P `mempool` request can make Zebra enumerate and allocate the full
  local mempool transaction-ID set before the connection layer truncates the
  outbound `inv` response to the protocol cap. This is bounded by mempool size,
  but should return a limited iterator result from the mempool service.
- A large transaction `inv` message is capped on the wire, but Zebra forwards
  the full deduplicated set of unique `UnminedTxId`s into the mempool queue,
  creates per-entry response bookkeeping when the mempool is enabled, and only
  then rejects excess downloads at the downloader's small concurrency cap. This
  is bounded public P2P/mempool availability hardening.
- Mempool download/verify tasks remove their cancel-handle entries on success
  and ordinary verification/download errors, but not on the downloader's outer
  timeout. A local paused-time proof test shows timed-out txids leave stale
  `cancel_handles`, are permanently treated as `AlreadyQueued`, and unique later
  txids can continue accumulating stale retained requests after each timeout.
  Direct pushed transactions can retain full `Gossip::Tx` values. Treat as a
  private maintainer heads-up candidate until a full adversarial near-tip repro
  settles how easily remote peers can make the 73-second outer timeout win.
- Downloaded or gossiped blocks without a coinbase height are rejected before the
  verifier can return score-bearing `BlockError::MissingHeight`; the preflight
  paths do not preserve serving-peer attribution for misbehavior scoring. This is
  public P2P robustness hardening, not consensus.
- Misbehavior reports use ignored `try_send()` calls on a bounded channel before
  the address-book ban pipeline. A burst can silently drop otherwise-correct
  score reports if the channel is full. This is public peer-enforcement
  hardening, not consensus.
- Address-book ban handling can panic after a remote-influenced misbehavior
  score reaches the ban threshold when `network.max_connections_per_ip > 1`.
  That non-default configuration disables the `most_recent_by_ip` cache, but the
  ban branch still unwraps it while applying `UpdateMisbehavior`. This is a
  private maintainer heads-up candidate for network availability in supported
  multi-connection-per-IP deployments.
- Tip-local mempool rejection caches clear the entire exact-tip or same-effects
  tip map once a map exceeds 40,000 entries. This bounds memory, but lets enough
  unique rejected transaction identities erase the cache and force repeated
  download/verification work under the same chain tip. This is public
  availability hardening, not consensus; see
  `docs/analysis/mempool-tip-rejection-cache-thrash-note.md`.
- The prior mempool exact-tip rejection-cache behavior remains a private
  maintainer heads-up candidate, but it is separate from pass 5's new work and
  is availability/policy focused rather than consensus accepting invalid blocks.

## Private Disclosure Triage

### Already Privately Disclosed

**V5 `SIGHASH_SINGLE` missing corresponding output acceptance**

Simple severity description: a specially signed V5 transparent spend can be
accepted by Zebra when zcashd rejects it. If that transaction were mined or used
in a block path where Zebra accepts it, Zebra could disagree with Zcash network
consensus.

Confidence: high.

Evidence:

- `zebra-script/src/lib.rs:207-218` maps the C++ callback hash type to Zebra's
  ZIP-244 sighasher without explicitly rejecting a `SIGHASH_SINGLE` input whose
  index has no corresponding output.
- The full transaction verifier reaches script verification after branch-id,
  expiry, locktime, conflict, and value checks in
  `zebra-consensus/src/transaction.rs:404-456`.
- Current zcashd ZIP-244 `SignatureHash` rejects missing corresponding outputs
  for `SIGHASH_SINGLE` and `SIGHASH_SINGLE|ANYONECANPAY` before digest
  calculation.
- Local repro tests pass in both `zebra-script` and `zebra-consensus`, including
  the `ANYONECANPAY` variant.

### Private Maintainer Heads-Up Candidate

**Mempool state lookup errors collapse into cached exact-tip missing-input
rejections**

Confidence: medium-high from local repro. This is not a consensus divergence,
but it can make transient state errors sticky in mempool policy until the tip
changes. Treat as a private maintainer heads-up or security issue only if the
team wants operational availability issues to use the security inbox.

**Mining RPC verifier timeout gap**

Confidence: high that the timeout gap exists in `submitblock` and
`getblocktemplate` proposal mode; high that a real checkpoint-verifier wait path
can leave `submitblock` pending for an out-of-order checkpoint-era block;
medium-low on impact against ordinary fully synced mining deployments.

The sharper follow-up pass also found that this is a family of miner-facing
liveness/classification issues, not just a missing outer timeout:

- semantic post-checkpoint block/proposal verification can wait on
  block-context `AwaitUtxo` lookups, with the timeout owned by the transaction
  verifier rather than RPC,
- `ready().await` on the buffered verifier router is another externally visible
  wait point before dispatch,
- non-duplicate verifier/infrastructure errors after dispatch collapse to
  `Rejected` in `submitblock` or invalid-proposal strings in proposal mode,
- a block can be accepted by the verifier and then surface as a JSON-RPC error
  if the mined-block gossip notification channel is full or closed,
- `generate` and the experimental internal miner reuse `submit_block()` and
  inherit the same deadline/classification behavior.

Proposal validation itself appears state-safe: it validates against a cloned
non-finalized state and does not mutate canonical state.

This is not a consensus divergence. Treat as a private maintainer heads-up if
bundling with miner availability / Litecoin-shaped incident concerns; otherwise
it can be handled as public hardening.

**ZIP-235 miner-fee share intermediate overflow panic**

Confidence: high for the panic in the combined unstable build and direct
consensus helper; medium for full future-network exploitability because default
Mainnet/Testnet do not activate NU7 and source-controlled release/Docker
features do not enable the combined `nu7 + zip235 + tx_v6` path.

When compiled with `zcash_unstable = "nu7"`,
`zcash_unstable = "zip235"`, and `tx_v6`, an NU7-active network reaches the
ZIP-235 miner-fee share check in block validation. The check computes
`((block_miner_fees * 6).unwrap() / 10).unwrap()`. `Amount * u64` is checked
against `MAX_MONEY`, so a block fee above `MAX_MONEY / 6` can overflow the
intermediate multiplication even when the intended `floor(fees * 60 / 100)`
share would be representable. The same unwrap shape is in V6 coinbase
generation when `zip233_amount` is omitted.

Evidence:

- `zebra-consensus/src/block/check.rs:337-344` runs the ZIP-235 check at and
  after NU7 activation, and unwraps the intermediate `block_miner_fees * 6`.
- `zebra-chain/src/transaction/builder.rs:90-95` computes the default V6
  coinbase ZIP-233 amount with the same intermediate unwrap.
- `zebra-chain/src/amount.rs:377-394` makes `Amount * u64` return
  `MultiplicationOverflow` when the intermediate result cannot be represented
  as the same constrained `Amount`.
- `zebra-rpc/src/methods/types/get_block_template.rs:811-831` passes selected
  mempool miner fees into V6 coinbase generation under the combined NU7/V6
  build.

Triage: conservative private maintainer heads-up / future-activation blocker.
It is not an emergency current-mainnet disclosure item on current evidence, but
it is consensus-path code and should be fixed before any NU7/ZIP-235-capable
artifact or custom network can rely on it. See
`docs/analysis/zip235-miner-fee-share-intermediate-overflow-panic-note.md`.

**RPC `longpollid` Unicode panic**

Confidence: high on the parser panic and RPC parameter deserialization path.
`LongPollId::from_str()` checks byte length, then slices the original UTF-8
string at offsets 10, 18, 28, and 38. A 46-byte string with a multibyte
character crossing one of those offsets panics before normal invalid-parameter
handling. Local repro tests confirm both direct parsing and
`GetBlockTemplateParameters` deserialization panic today. Because the workspace
sets `panic = "abort"` for dev and release profiles, this is process-fatal in
the actual binary.

A follow-up RPC parser sweep did not find another parameter parser with the same
fixed-offset UTF-8 slicing shape. `WtxId::from_str()` already splits bytes and
then validates UTF-8, `BlockTemplateTimeSource` and `Zec` parsing return normal
errors, hex helpers return serde errors, and the HTTP compatibility middleware
assertions are framework-response invariants rather than attacker string
parsers. Treat as a private maintainer heads-up / coordinated disclosure
candidate for RPC availability. See
`docs/analysis/rpc-longpollid-unicode-panic-finding.md`.

**RPC `z_listunifiedreceivers` invalid Sapling receiver panic**

Confidence: high on the panic and direct RPC method path. The method first
decodes the caller-supplied Unified Address with `zcash_address`, then unwraps
`zebra_chain::primitives::Address::try_from_sapling()` for decoded Sapling
receivers. `zcash_address` receiver decoding enforces the Sapling typecode and
43-byte length, but does not prove those bytes form a valid Sapling payment
address. Durable current-behavior test
`rpc_z_listunifiedreceivers_panics_on_invalid_sapling_receiver_today` uses
`Receiver::Sapling([0; 43])` encoded as a Unified Address and confirms the RPC
method reaches `expect("using data already decoded as valid")`. Because the
workspace sets `panic = "abort"` for dev and release profiles, this is
process-fatal in the actual binary.

Treat as a private maintainer heads-up / coordinated disclosure candidate for
RPC availability, in the same bucket as the `longpollid` panic. It is not a
consensus issue and default RPC/auth configuration limits exposure, but the
payload is simple for any attacker who can reach JSON-RPC. See
`docs/analysis/rpc-z-listunifiedreceivers-invalid-sapling-panic-finding.md`.

**RPC height-based `getblock` snapshot/panic path**

Confidence: high for the code bug and local mock-state panic proof; medium-low
for practical exploitability because the trigger requires RPC access plus a
non-finalized reorg or best-chain switch affecting the queried height between
separate read-state requests.

For verbosity `1` and `2`, `get_block()` first calls `get_block_header()` and
gets a resolved block hash, height, and signed confirmations value. It then
builds the transaction follow-up request from the original `HashOrHeight`
before shadowing that variable with the resolved hash. Height callers therefore
continue to use `TransactionIdsForBlock(height)` or `BlockAndSize(height)`.
If the best chain switches from block `A` to block `B` at the queried height
between those reads, Zebra can combine header metadata for `A` with transaction
data for `B`.

The sharper availability variant is `getblock <height> 2`: if the header
subcall resolves `A`, `Depth(A)` later returns `None`, and `BlockAndSize(height)`
returns replacement block `B`, `get_block_header()` returns
`confirmations = -1` and `get_block()` panics while converting that value to
`u32` for verbose transaction objects. Local test
`rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today`
forces this mocked response order and passes as `#[should_panic]`.

Treat as a private maintainer heads-up rather than a public issue until ZF says
how they want RPC availability bugs handled. It is not a consensus acceptance
bug, and ordinary positive-height tip advancement is not enough; the block at
the queried height must change. See
`docs/analysis/rpc-getblock-height-snapshot-consistency-note.md`.

**RPC cookie rewrite preserves loose existing file permissions**

Confidence: high that the file mode is not tightened when an existing regular
`.cookie` is rewritten. Impact is local and deployment-dependent, but this
touches RPC authentication secret exposure and partially undermines the
cookie-file hardening guarantee for stale files. Treat as a private maintainer
heads-up unless ZF prefers local credential-hardening regressions to be public.

**RPC cookie cleanup is not reached on failed startup or task abort**

Confidence: high that the cleanup gap exists on the live path: local proof tests
showed an auth-enabled bind failure leaves `.cookie` behind, and aborting the
returned RPC server task also leaves `.cookie` behind. Impact is local and
deployment-dependent because stale cookies are not useful after the corresponding
server has stopped, but this composes with the loose-existing-file permission
issue: a stale or supervised-restart cookie path can keep exposing future fresh
credentials if its file mode is already unsafe. Treat as the same private
maintainer heads-up bucket as the existing-file permission issue.

**RPC `invalidateblock` chain-root panic**

Confidence: high for the panic condition and local reproducer; medium for
practical exploitability because it requires RPC access plus a target
non-finalized chain-root hash.

Durable current-behavior test
`invalidating_chain_root_panics_when_removing_existing_chain_today` commits a
two-block non-finalized chain, then calls
`state.invalidate_block(block1.hash())` where `block1` is the non-finalized
root. The call panics at `Chain::cmp` with
`Chain tip block hashes are always unique`; the panic comes from
`BTreeSet::remove` inside `NonFinalizedState::invalidate_block()`.
The test passes with:

```sh
cargo test -p zebra-state invalidating_chain_root_panics_when_removing_existing_chain_today --lib
```

The root cause is that the root-invalidation branch calls
`self.chain_set.remove(&chain)`. Since `chain_set` is a `BTreeSet<Arc<Chain>>`,
removal compares the lookup key against the stored chain using `Ord for Chain`.
For the same stored chain, equal tip hashes are normal, but `Chain::cmp` treats
that equality as unreachable. See
`docs/analysis/rpc-invalidateblock-chain-root-panic-finding.md`.

Treat as a private maintainer heads-up: it is not consensus acceptance and not
unauthenticated P2P, but it is a confirmed process-fatal trusted-RPC
availability issue with a simpler trigger shape than the same-height sibling
`invalidateblock` panic.

**RPC `invalidateblock` same-height fork panic**

Confidence: high for the panic condition and local reproducer; medium for
practical exploitability because it requires RPC access plus a non-finalized
state containing two valid same-parent fork tips.

Durable current-behavior test
`invalidating_same_height_fork_tips_panics_today` creates two different fake
children of the same non-finalized parent, commits both sibling forks,
invalidates one sibling, then invalidates the other. The second invalidation
panics at `Chain::cmp` with `Chain tip block hashes are always unique`. The test
passes with:

```sh
cargo test -p zebra-state invalidating_same_height_fork_tips_panics_today --lib
```

The root cause is that `invalidate_block()` inserts the shortened parent-only
chain before filtering the invalidated sibling chain out of the `BTreeSet`.
After the first sibling invalidation, that same parent-only chain already
exists. The second sibling invalidation therefore makes `BTreeSet::insert`
compare two chains with the same tip hash, hitting the duplicate-tip
`unreachable!`. See
`docs/analysis/rpc-invalidateblock-same-height-fork-panic-finding.md`.

Treat as a private maintainer heads-up: it is not consensus acceptance and not
unauthenticated P2P, but it is a confirmed process-fatal trusted-RPC
availability issue.

**RPC `reconsiderblock` stale invalidation replay panic**

Confidence: high for the stale live-entry bug and local reproducer; medium for
practical exploitability because it requires RPC access plus an
invalidate/reconsider sequence.

Durable current-behavior test
`reconsider_block_twice_replays_stale_invalidated_entry_today` invalidates a
non-finalized chain segment, reconsiders it successfully, confirms that the
invalidated entry still remains in `state.invalidated_blocks()`, then calls
`reconsider_block()` for the same hash again. The second reconsider panics at
`Chain::cmp` with `Chain tip block hashes are always unique`. The test passes
with:

```sh
cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib
```

The root cause is that `reconsider_block()` locates the target entry in
`self.invalidated_blocks`, but then calls
`self.invalidated_blocks.clone().shift_remove(height)`. That removes the entry
from a temporary clone, not from live state. A subsequent reconsider can replay
the same invalidated suffix and try to insert a chain whose tip already exists.
See `docs/analysis/rpc-reconsiderblock-stale-invalidated-entry-panic-finding.md`.

Treat as a private maintainer heads-up: it is not consensus acceptance and not
unauthenticated P2P, but it is a confirmed process-fatal trusted-RPC
availability issue with a simpler trigger shape than the same-height sibling
`invalidateblock` panic.

**Block chain value-pool calculation suppresses transaction errors**

Confidence: high on the code bug, low on direct remote exploitability through
normal semantic block verification.

`Block::chain_value_pool_change()` maps transactions through
`Transaction::value_balance(utxos)` and then uses `flat_map`. Because `Result`
is iterable, an `Err(ValueBalanceError)` produces no item and is silently omitted
from the block sum. That means callers expecting
`Result<ValueBalance<NegativeAllowed>, ValueBalanceError>` to fail on any
transaction-level calculation error can instead receive a partial block value
delta.

Durable current-behavior test
`chain_value_pool_change_drops_transaction_value_balance_errors_today` confirms
the behavior: a coinbase transaction with two individually valid `MAX_MONEY`
transparent outputs makes `Transaction::value_balance()` return `Err`, while
`Block::chain_value_pool_change()` currently returns `Ok` with a zero delta.
The test passes under
`cargo test -p zebra-chain chain_value_pool_change_drops_transaction_value_balance_errors_today --lib`.

Follow-up reachability testing reproduced the helper bug and then confirmed the
same concrete coinbase transparent-overflow shape is rejected by
`miner_fees_are_valid()` before state commit:
`cargo test -p zebra-consensus miner_fees_reject_coinbase_output_sum_over_max_money_probe --lib`.
That probe was also removed.

The normal semantic block path appears to catch realistic attacker-controlled
cases earlier through transaction fee calculation, contextual
`remaining_transaction_value()` checks, coinbase structural checks, and
`miner_fees_are_valid()`. The more realistic risk is checkpointed block
processing, finalized/non-finalized persistence invariants, and database
upgrade/replay. The migration replay fallback is silently corrupting if its
`chain_value_pool_change()` error branch is ever hit, but reachability from
normal finalized DB contents remains unproven.

Treat as a private maintainer heads-up because this touches validator
value-pool accounting and state persistence. See
`docs/analysis/value-pool-error-suppression-note.md`.

**Address-book misbehavior ban panic with `max_connections_per_ip > 1`**

Confidence: high for the panic condition and direct reproducer; medium for
practical exploitability because the affected configuration is supported but
non-default.

`AddressBook::new()` only creates `most_recent_by_ip` when
`max_connections_per_ip == 1`, matching the field comment that the cache does
not support larger per-IP connection limits. But when an
`UpdateMisbehavior` change pushes a peer to `MAX_PEER_MISBEHAVIOR_SCORE`, the
ban branch unconditionally unwraps `self.most_recent_by_ip` before removing the
banned IP. Nodes configured with `network.max_connections_per_ip > 1` therefore
have `most_recent_by_ip = None` and can panic when the address-book updater
applies a ban-triggering remote peer misbehavior report.

Durable current-behavior test
`misbehavior_ban_panics_with_max_connections_per_ip_above_one_today` constructs
an `AddressBook` with `max_connections_per_ip = 2`, applies an
`UpdateMisbehavior` change with
`score_increment = MAX_PEER_MISBEHAVIOR_SCORE`, and confirms the panic at the
documented `expect()` site. The test passes with:

```sh
cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one_today --lib
```

The live updater applies `AddressBook::update(event)` while holding the shared
address-book mutex, and later shared address-book helpers panic on poisoned
locks. So the confirmed panic can poison the address book and turn the original
misbehavior-ban event into broader peer-management panics rather than a single
dropped update.

Treat as a private maintainer heads-up. It is not a default-node crash and not
consensus-critical, but remote peers can influence misbehavior scoring through
normal invalid-transaction or invalid-block paths once the node is running this
supported multi-connection-per-IP configuration. See
`docs/analysis/address-book-misbehavior-ban-max-connections-panic-finding.md`.

### Public Hardening, Not Private

**Custom NU7 activation V5 serialization panic**

Confidence: high for the panic in normal builds with custom NU7 activation;
low for default deployment impact.

Evidence:

- `zebra-rpc/src/methods/types/get_block_template.rs:815-821` generates a V5
  coinbase for `NetworkUpgrade::Nu7` when not built with both
  `zcash_unstable = "nu7"` and `tx_v6`.
- `zebra-chain/src/transaction/builder.rs:172-178` tags the generated V5
  coinbase with `NetworkUpgrade::current(network, height)`.
- `zebra-chain/src/parameters/network_upgrade.rs:229-231` only includes the NU7
  placeholder branch ID under `#[cfg(any(test, feature = "zebra-test"))]`.
- `zebra-chain/src/transaction/serialize.rs:682-686` panics when a V5
  transaction's network upgrade has no branch ID.
- A temporary normal-build example with custom Regtest `nu7: Some(1)`,
  `Transaction::new_v5_coinbase(...)`, and `zcash_serialize_to_vec()` panicked
  at `zebra-chain/src/transaction/serialize.rs:686:26`; the probe source was
  removed afterward.

Suggested public fix direction: reject `ConfiguredActivationHeights::nu7` unless
the build has real NU7 branch-ID and transaction-version support, return a
clean GBT/internal-miner error for `Nu7` in non-NU7 builds, or make transaction
serialization return a typed error instead of panicking for internally
constructible branch-ID-less transactions. See
`docs/analysis/nu7-custom-activation-v5-serialization-panic-note.md`.

**Custom-network implicit NU6.1 lockbox boundary**

Confidence: medium-high for source behavior; low for default-network impact.
`NetworkUpgrade::activation_height()` intentionally falls forward to the next
configured upgrade when a requested upgrade has no explicit height. On custom
Regtest/Testnet networks, that means omitting `nu6_1` while setting a later
upgrade such as NU7 can make `NetworkUpgrade::Nu6_1.activation_height(network)`
return the later upgrade height.

NU6.1 lockbox helpers, subsidy validation, and GBT coinbase output generation
use that height directly. Regtest defaults missing `lockbox_disbursements` to
empty, so the inherited height can become unexpectedly unmineable with
`missing lockbox disbursements for NU6.1 activation block`. If custom
disbursements are configured, templates and block validation can instead
require those one-time outputs at a later upgrade height the operator did not
intend as NU6.1 activation. See
`docs/analysis/custom-network-implicit-nu6-1-lockbox-boundary-note.md`.

**High-cardinality metrics labels**

Confidence: medium. The code paths are clear, but the impact depends on metrics
being enabled and on deployment limits.

Evidence:

- Peer handshake metrics include a port-bearing `remote_ip` socket label and
  remote `user_agent` labels in
  `zebra-network/src/peer/handshake.rs:760-766` and
  `zebra-network/src/peer/handshake.rs:799-806`. `PeerSocketAddr::Display`
  redacts the IP but preserves the port.
- Inbound message error metrics use `err.to_string()` and the peer address as
  labels in `zebra-network/src/peer/handshake.rs:1065-1070`.
- Peer-cache metrics label actual cached `IP:port` strings in
  `zebra-network/src/config.rs:530-536`; this is bounded to
  `MAX_PEER_DISK_CACHE_SIZE = 75` peers per cache write, but the values can
  originate from proactive peer address discovery.
- Mempool failed verification metrics use `error.to_string()` as the `reason`
  label in `zebrad/src/components/mempool.rs:656-658`; several transaction
  errors include attacker-controlled or attacker-variable hashes/outpoints.
- RPC metrics label by JSON-RPC method in `zebra-rpc/src/server/rpc_metrics.rs:63-80`.
- The same RPC metrics middleware increments `rpc.active_requests` before
  awaiting the method future and decrements it only after completion in
  `zebra-rpc/src/server/rpc_metrics.rs:49-86`; cancellation while a method is
  pending can therefore leave the gauge inflated until restart.
- Metrics are disabled by default at runtime because
  `zebrad/src/components/metrics.rs:67-83` defaults `endpoint_addr` to `None`.
- If metrics are enabled, the exporter listener is another public hardening
  point: Zebra calls `PrometheusBuilder::new().with_http_listener(addr).install()`
  without a Zebra-side allowlist, connection semaphore, or request/header
  timeout. The upstream listener accepts TCP streams and spawns a Hyper
  `serve_connection()` task per stream. The user docs show a safe localhost
  binding, but the generated environment-variable examples include
  `0.0.0.0:9999`.

Suggested public fix direction: replace attacker-influenced metric labels with
bounded enum/category labels, such as transaction error variant, handshake
outcome, negotiated version bucket, and known RPC method or `unknown`. Keep the
metrics endpoint localhost-only unless an allowlist, open-connection cap, and
request/header timeout are added. Use a drop guard for active RPC gauges so
client disconnects and cancelled method futures cannot make observability drift.

**RPC tracing raw method attribute**

Confidence: medium-high as public observability hardening.

The RPC tracing middleware copies `request.method_name()` and records it as the
`rpc.method` span attribute on every RPC request. Zebra's HTTP compatibility
middleware applies a whole-request body cap before JSON-RPC parsing, so this is
not an unlimited single-span payload bug. The remaining issue is cardinality and
export churn across many distinct unknown method names when RPC is reachable and
OpenTelemetry or Sentry tracing/log export is configured. See
`docs/analysis/rpc-tracing-method-attribute-amplification-note.md`.

Suggested public fix direction: share the same bounded method classifier between
RPC metrics and RPC tracing: registered method names may be recorded as-is, and
unknown method names should become `unknown` or another static category.

**Sentry and OpenTelemetry privacy**

Confidence: medium-high on the activation gates and export filters; medium on
deployment sensitivity.

Official release builds include both `sentry` and `opentelemetry`, but Sentry is
only initialized when `SENTRY_DSN` is set, and OpenTelemetry only constructs an
exporter when `tracing.opentelemetry_endpoint` or
`OTEL_EXPORTER_OTLP_ENDPOINT` is configured. This eliminates the hypothesis of a
default-on off-host telemetry leak.

The hardening gap is that the user docs describe Sentry "production monitoring"
without clearly saying that opt-in Sentry export includes panic reports,
`ERROR` events/logs, `WARN` logs/breadcrumbs, `INFO` breadcrumbs, and structured
tracing fields. OpenTelemetry export similarly sends span data and defaults to
100% sampling once an endpoint is configured and no sample percentage is set.
The `getinfo.errors` RPC diagnostic field stores only the WARN/ERROR event
`message` text, not structured fields, so I did not find a broad remote-input
leak through that RPC field.

Suggested public fix direction: document the exact exported data, add a Sentry
redaction hook for known sensitive tracing keys before logs/events leave the
process, and consider a lower default OpenTelemetry sampling percentage for
environment-only configuration. See
`docs/analysis/sentry-opentelemetry-privacy-note.md`.

**Docker examples publish unauthenticated JSON-RPC**

Confidence: high. `docker/docker-compose.lwd.yml` and
`docker/docker-compose.observability.yml` both set
`ZEBRA_RPC__LISTEN_ADDR=0.0.0.0:8232`, set
`ZEBRA_RPC__ENABLE_COOKIE_AUTH=false`, and publish `"8232:8232"`.

This is public hardening because the examples are public, but it is
operationally important: copied deployments can expose state-changing and
expensive RPCs such as `sendrawtransaction`, `stop`, `getblocktemplate`, and
`submitblock` to untrusted networks. See
`docs/analysis/rpc-docker-unauthenticated-public-bind-note.md`.

**Inbound misbehavior score downcast**

Confidence: high from local unit test. This can make some score-bearing block
verification router errors fail to increase peer misbehavior score. It is a
network robustness issue, not an invalid-block acceptance path.

**Direct pushed transaction source attribution**

Confidence: high from direct code path review. Wire `tx` messages become
`Request::PushTransaction(UnminedTx)` without a source address, then
`Gossip::Tx(tx)` sets `advertiser_addr` to `None` in the mempool downloader. As
a result, score-bearing invalid pushed transactions can be rejected without
scoring the sending peer. See
`docs/analysis/mempool-direct-push-source-attribution-note.md`.

**Malformed block height source attribution**

Confidence: high from direct code path review. `BlockError::MissingHeight` has a
nonzero block misbehavior score, but sync maps no-height downloaded blocks to
`InvalidHeight { hash }` without `advertiser_addr`, and inbound maps gossiped
no-height blocks to a generic boxed error with `None` attribution. As a result,
malformed no-height blocks are rejected but do not score the serving peer. See
`docs/analysis/malformed-block-height-misbehavior-attribution-note.md`.

**Lossy misbehavior report transport**

Confidence: medium-high from direct code path review. The address-book batcher
drains misbehavior reports into a score map continuously and flushes every 30
seconds, but the three score-producing components all use ignored `try_send()` on
a bounded channel. If a burst fills that channel before the receiver drains it,
the report is silently lost. See
`docs/analysis/misbehavior-reporting-lossy-channel-note.md`.

**Mempool dependency and GBT selection work**

Confidence: medium-high on a GBT dependency-metadata mismatch; low-to-medium on
ancestor-depth CPU impact. The mempool has bounded transaction sizes, conflict
tracking, and verification limits, but dependency selection does not appear to
have an explicit ancestor-depth policy matching Bitcoin-style
ancestor/descendant caps.

The stronger follow-up is a `getblocktemplate` correctness issue. Zebra now
stores mempool transaction dependencies and the ZIP-317 selector can include a
dependent transaction after its parent has been selected. The existing unit test
`includes_tx_with_selected_dependencies` confirms that selected-dependent shape.
But `TransactionTemplate::from(&VerifiedUnminedTx)` still sets `depends:
Vec::new()` for every non-coinbase transaction and documents that Zebra's
mempool does not support dependencies. `getblocktemplate` also advertises
`"transactions"` as mutable, so clients are allowed to filter or reorder the
template transaction set. A miner or pool proxy that trusts the empty dependency
list can keep a child while dropping its parent and produce invalid block work.

This should be handled as public mining RPC compatibility hardening. It is not a
private consensus issue because Zebra's production selection order appears to
put selected parents before selected children, and miners that include the whole
template in order should be unaffected.

Suggested public fix direction: compute `depends` from the final selected
transaction vector using 1-based indexes into `transactions`, update the stale
comment saying dependencies are unsupported, and add a regression test where a
selected child transaction records its selected parent index.

**GBT testnet sync-gate bypass**

Confidence: high on the code/doc mismatch and affected call paths; medium-low on
practical severity because the issue depends on configured mining RPC and
testnet/custom-network automation.

`getblocktemplate` template mode and proposal mode call
`check_synced_to_tip()` before proceeding. The helper's documentation says it
returns early when Proof-of-Work is disabled on the provided network, but the
implementation returns early for every `network.is_a_test_network()`. Default
Testnet is a test network with PoW enabled, so default Testnet mining RPC users
skip the near-tip guard that Mainnet uses.

This does not affect Mainnet consensus acceptance and does not let Zebra mine a
valid block while unsynced. The risk is operator-facing readiness: pools,
testnet miners, or automation can receive templates or proposal responses from a
node that is not close to the consensus tip. See
`docs/analysis/testnet-gbt-sync-gate-bypass-note.md`.

Suggested public fix direction: use `network.disable_pow()` rather than
`network.is_a_test_network()` for the early return, and add template/proposal
tests for default Testnet rejection versus PoW-disabled Regtest/custom-network
allowance.

**GBT high-fee coinbase overflow panic**

Confidence: high for the direct panic in `standard_coinbase_outputs()`;
medium-low for realistic remote exploitability on default public networks.

`getblocktemplate` sums the fees of selected mempool transactions and passes the
total into `standard_coinbase_outputs()`. That helper checks the selected fee
sum itself, but then adds `miner_subsidy + miner_fee` and unwraps the result with
`expect("reward calculations are valid for reasonable chain heights")`. The
block verifier maps the corresponding arithmetic overflow into a subsidy error,
so this is a template-path panic rather than consensus acceptance.

Durable current-behavior test
`standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money_today`
locks in the helper panic for a custom NU6-active network with
`miner_fee = MAX_MONEY`.

The practical mainnet risk is low because an attacker would need to get
economically extreme high-fee transactions into the node's mempool before a
configured mining RPC caller asks for a template. The shape is more relevant on
custom networks, Regtest, test harnesses, and operator-controlled mining setups.
See `docs/analysis/getblocktemplate-high-fee-coinbase-overflow-panic-note.md`.

Suggested public fix direction: make `standard_coinbase_outputs()` return a
typed error instead of panicking, and cap selected total fees at
`MAX_MONEY - miner_subsidy(...)` during template transaction selection.

**Mempool cascading removal notifications**

Confidence: medium-high on code shape; medium-low on severity. Mempool removal
helpers remove direct and indirect dependents of a selected transaction. But
`VerifiedSet::evict_one()` returns only the randomly selected victim, dropping
the removed dependent set from its return value. `Storage::insert()` then caches
only that selected victim as `RandomlyEvicted` and leaves the insertion result
as `Ok(new_txid)` unless the selected victim is the newly inserted transaction.

If insertion of a dependent transaction pushes the mempool over its ZIP-401 cost
limit and eviction randomly selects one of the new transaction's ancestors,
Zebra can remove both ancestor and new dependent while still publishing a
`MempoolChange::added(new_txid)` for the dependent. Expiry cleanup has the same
reporting shape for non-expired dependents removed under an expired ancestor:
the dependent is removed by cascading dependency cleanup but omitted from the
returned invalidation set.

This is public mempool/indexer/gossip consistency hardening, not private
disclosure. It can mislead mempool-change subscribers and cause extra
re-download or missing-output verification churn, but it does not affect block
validation or let invalid transactions remain in storage. See
`docs/analysis/mempool-cascading-removal-notification-note.md`.

**GBT long-poll full-mempool polling amplification**

Confidence: medium-high as public hardening. The template-mode long-poll loop
fetches state chain info and then asks the mempool for `FullTransactions` before
calculating the server long-poll ID. If that ID still matches the client's
current ID, the request sleeps for `MEMPOOL_LONG_POLL_INTERVAL` and repeats. The
mempool service answers `FullTransactions` by cloning every verified transaction
and cloning the dependency map, so each waiting long-poll RPC performs an
independent full-mempool clone every five seconds until the tip, mempool
checksum, or max-time condition changes.

This is not a private consensus issue because it requires configured mining RPC
access, ordinary RPC is disabled/authenticated by default, and server-level
connection limits bound the number of live waiters. It is still useful to harden
for mining pools, proxies, and copied deployments by adding a waiter cap,
deduplicating matching long-poll waiters, or sharing short-lived
`FullTransactions` snapshots. See
`docs/analysis/gbt-longpoll-full-mempool-poll-amplification-note.md`.

**GBT long-poll max-time already reached**

Confidence: medium-high as public mining RPC correctness / availability
hardening. State clamps template `cur_time` into the valid block-time range, so
`cur_time == max_time` is a legitimate state output. The long-poll ID records
tip height, a tip-hash checksum, `max_time`, mempool count, and mempool
checksum, but not `cur_time` or whether max time has already been reached.

The template-mode long-poll loop returns when the generated server ID differs
from the client ID or when the max-time sleep future fired in the previous loop
iteration. But if `cur_time` is already clamped to `max_time`, Zebra deliberately
omits that zero-duration max-time future. A caller whose current `longpollid`
still matches the server ID can therefore keep waiting through five-second
mempool polls until an unrelated mempool or tip change occurs, instead of
receiving a prompt `submitold=false` template refresh.

This requires configured mining RPC access and is not consensus-critical. The
targeted fix is to treat `cur_time >= max_time` as already reached for long-poll
requests, either by returning immediately with `submitold=false` or by allowing
a zero-duration max-time future to fire. See
`docs/analysis/gbt-longpoll-max-time-already-reached-note.md`.

**Primitive verifier failure taxonomy and fallback coverage**

Confidence: medium-high as public hardening / test coverage. The code appears
to use the same single-item validator path for fallback verification, so no
acceptance divergence was found. But several attacker-reachable invalid
proof/signature failures currently surface as `InternalDowncastError` because
`TransactionError::from(BoxError)` only recognizes a subset of boxed primitive
errors. Groth16 worker or channel infrastructure errors can also be mapped into
`TransactionError::Groth16(...)`, which looks like a consensus-invalid proof
rather than verifier infrastructure failure. Ed25519, RedJubjub, RedPallas, and
Halo2 also still have panic-on-verifier-drop branches in their watch-channel
wait paths. These do not create acceptance of invalid transactions, but they are
worth tightening so invalid proof/signature, malformed proof/signature, and
verifier-unavailable failures remain distinct.

Suggested public fix direction: add a dedicated primitive-verifier
infrastructure error variant, preserve Groth16 invalid-proof errors separately,
replace verifier-drop panics with infrastructure errors, and add fallback/error
taxonomy tests for RedJubjub, RedPallas, Halo2, and Groth16. See
`docs/analysis/primitive-verifier-failure-taxonomy-note.md`.

**Tracing filter-reload endpoint body limit and auth**

Confidence: medium-high on the hardening issue. With the optional
`filter-reload` feature enabled and `tracing.endpoint_addr` configured, the
endpoint accepts unauthenticated filter updates and collects `POST /filter`
bodies without an explicit body limit. This is not part of the default release
feature set and should be treated as public hardening unless a production profile
intentionally exposes the endpoint.

**RPC batch request count limit**

Confidence: medium-high on the missing batch-count cap; medium-low on practical
impact because RPC is disabled by default and cookie authentication is enabled by
default.

Zebra bounds the total request body before jsonrpsee handles it, but it does not
configure jsonrpsee's batch policy. jsonrpsee-server 0.24.10 defaults to
`BatchRequestConfig::Unlimited`, parses batch bodies into `Vec<&JsonRawValue>`,
and sequentially calls the RPC service for each batch entry. It also defaults to
a 100-permit `ConnectionGuard`, and Zebra calls `.http_only()`, so this is not
an unbounded connection or subscription issue. The missing control is the direct
batch-count cap, which can amplify RPC work and the existing high-cardinality
`method` metric label in deployments that expose or share RPC access.

Suggested public fix direction: disable JSON-RPC batches if compatibility does
not require them, or set a conservative `BatchRequestConfig::Limit(N)` and
normalize unknown RPC method labels to `unknown`.

**V5 mempool `MSG_WTX` exactness**

Confidence: high on current behavior from local repro; medium-low on security
severity.

`TransactionsById` accepts `UnminedTxId`, and `MSG_WTX` maps to a V5
`UnminedTxId::Witnessed(WtxId)`. But mempool storage currently implements
`transactions_exact()` by looking up only `tx_id.mined_id()`. The local test
`transactions_exact_matches_v5_by_mined_id_not_wtxid_today` mutates only the
auth digest in the requested WTXID and still gets the stored transaction back.

Suggested public fix direction: keep the mined-ID index for lookup, but compare
the stored `UnminedTxId` to the requested `UnminedTxId` before returning a
transaction.

**V5 same-effects mempool pending amplification**

Confidence: high on current downloader admission behavior from local repro;
medium-low on practical exploitability.

The downloader deduplicates in-flight work by exact `UnminedTxId`, but V5
`WtxId` includes both the mined/effects ID and the authorization digest. A local
test `same_mined_id_v5_wtxids_queue_separately_today` constructs 25 witnessed
IDs with the same mined ID and different auth digests. With the network,
verifier, and state services held pending, all 25 are admitted and occupy the
full inbound mempool download cap before a 26th distinct same-effects WTXID gets
`FullQueue`.

This is public mempool availability hardening, not consensus. Settled mempool
storage blocks accepted same-effects variants by mined ID, and verifier
`Invalid` results are exact-tip rejected by exact WTXID rather than pending
same-effects identity, but there is no same-mined-ID pending reservation while
download/verification work is in flight. See
`docs/analysis/mempool-v5-same-effects-pending-amplification-note.md`.

**Mempool tip-local rejection cache thrash**

Confidence: high on the source behavior and existing property tests; medium-low
on practical severity.

The exact-tip and same-effects-tip rejection maps are memory-limited by clearing
the whole map when it grows beyond `MAX_EVICTION_MEMORY_ENTRIES = 40_000`.
Existing property tests assert that over-limit exact-tip, non-standard, and
same-effects-tip rejection insertion drops the tip-local rejected count to zero.
That is memory-safe, but it means a burst of unique rejected transaction
identities can erase prior bad-transaction cache entries before the next block,
letting repeated submissions redo download, state lookup, verification, or
standardness work that the cache was supposed to skip.

This is not private consensus material. Normal mempool admission limits,
near-tip activation, and block-driven cache clearing reduce practical severity.
The targeted hardening is to use bounded FIFO/LRU eviction for tip-local
rejection maps instead of clearing the whole map. See
`docs/analysis/mempool-tip-rejection-cache-thrash-note.md`.

**Mempool downloader timeout stale cancel-handle retention**

Confidence: high on source behavior and local paused-time proof; medium on
practical default-network exploitability.

The mempool downloader stores each active `Gossip` request in
`cancel_handles`. `Downloads::poll_next()` removes that entry when the task
returns success or an ordinary download/verification error, but the outer
`RATE_LIMIT_DELAY` timeout returns only `Elapsed`, so there is no txid available
for cleanup. The local test
`timed_out_downloads_accumulate_cancel_handles_today` stalls all downloader
services, advances Tokio time by `RATE_LIMIT_DELAY`, and confirms timed-out
txids leave stale `cancel_handles`, duplicate txids become permanently
`AlreadyQueued`, and new unique txids can keep accumulating stale retained
requests after each timeout.

This is a private maintainer heads-up candidate rather than a fully confirmed
remote exploit. Peer `tx` and `inv` paths reach the downloader once the mempool
is enabled, and direct pushed transactions can retain full `Gossip::Tx` values.
The transaction verifier timeout is 8 minutes and the downloader's state service
handle is not wrapped in a shorter timeout, so the 73-second outer timeout can
win before verifier completion if best-chain UTXO lookup work or service backlog
is slow enough. The open question is how cheaply a remote peer can make that
happen on a near-tip node. See
`docs/analysis/mempool-downloader-timeout-cancel-handle-retention-finding.md`.

**V5 `getrawtransaction` blockhash exactness**

Confidence: medium as public RPC correctness hardening. On the
caller-supplied-`blockhash` path, `getrawtransaction` first checks whether the
named block contains the supplied mined `txid`, then separately fetches the
transaction body through `AnyChainTransaction(txid)`. For V5 transactions, that
mined ID identifies effects but not authorizing data. If two competing
non-finalized chains contain V5 transactions with the same mined ID and different
auth digests, Zebra can return the raw transaction/authdigest from the first
matching chain while reporting the caller-supplied block hash and active-chain
flag from the earlier block-context check.

This is not a private consensus issue; it requires RPC access and a
non-finalized sibling-chain auth-data variant, and the transaction effects are
still tied to the supplied mined ID. It is worth fixing for clients that use
`getrawtransaction(..., blockhash)` as an exact block-membership or
auth-commitment oracle. See
`docs/analysis/rpc-getrawtransaction-v5-blockhash-exactness-note.md`.

**Verbose RPC response-shape correctness**

Confidence: medium-high on code shape, low-to-medium on security severity. The
RPC builders have several response-accuracy issues: `getrawtransaction` mempool
hits pass `in_active_chain: Some(false)` despite having no caller-supplied
`blockhash`; `TransactionObject::from_transaction()` always emits an `orchard`
object even when the transaction has no Orchard shielded data; Orchard action
serialization re-finds `spend_auth_sig` by equality instead of using the
protocol's already-paired `AuthorizedAction`; and `getrawmempool(true)` derives
descendant counts, sizes, and fees from one direct-dependents map lookup, not a
transitive descendant closure.

This should be treated as public compatibility hardening rather than private
disclosure. The reviewed paths can mislead indexers, explorers, exchanges, or
mining infrastructure that use verbose Zebra RPC output as an exact
zcashd-compatible interface, but they do not crash Zebra, bypass validation, or
change state/mempool acceptance. See
`docs/analysis/rpc-verbose-response-shape-correctness-note.md`.

**P2P transaction `getdata` count cap**

Confidence: medium-high on the missing pre-mempool count cap; medium on impact
because inbound timeout, load shedding, and protocol message-size limits reduce
exploitability.

The block `getdata` path caps work with `GETDATA_MAX_BLOCK_COUNT` before state
lookups. The transaction path maps all transaction inventory entries into
`Request::TransactionsById`, forwards the whole set to the mempool, and only
then applies the outbound byte cap while constructing available transaction
responses. A peer can therefore force tens of thousands of mempool lookup
attempts and a large `notfound` response with one protocol-valid message.

Suggested public fix direction: add a `GETDATA_MAX_TRANSACTION_COUNT` and
truncate transaction IDs before the mempool request.

**P2P mempool request full-enumeration work**

Confidence: high on current behavior; medium-low on practical severity because
the mempool and inbound service are bounded.

An unauthenticated peer can send the tiny P2P `mempool` message. Zebra forwards
that to the mempool service as a request for all local transaction IDs, and the
mempool response collects every verified `UnminedTxId` into a `HashSet`. The
connection layer later truncates the outbound `inv` response to
`MAX_TX_INV_IN_SENT_MESSAGE`, but that cap applies after the full local mempool
enumeration and allocation work has already happened. See
`docs/analysis/p2p-mempool-request-enumeration-note.md`.

Suggested public fix direction: add a bounded mempool request variant that
returns at most the protocol response cap without first collecting all IDs.

**P2P transaction inv queue amplification**

Confidence: high on current behavior; medium-low on practical severity because
the inventory message and downloader concurrency are bounded.

A peer can send one protocol-valid `inv` containing many unique transaction
inventory IDs, subject to overall protocol message-size limits. Zebra
deduplicates those IDs to unique `UnminedTxId`s, converts the full set into
mempool gossip entries, and the enabled mempool queue path creates per-entry
response-channel bookkeeping before the downloader rejects excess work at the
25-download inbound concurrency cap. For a single large request of new IDs, the
same-request overflow is `FullQueue`; `AlreadyQueued` requires prior queue
state for the same ID. The inbound service ignores the resulting per-entry queue
response, so much of this work is avoidable for transaction advertisements. See
`docs/analysis/p2p-transaction-inv-queue-amplification-note.md`.

Suggested public fix direction: cap transaction IDs from one inbound `inv`
before forwarding to the mempool queue, and add a fire-and-forget advertisement
API that does not create response channels for callers that ignore the result.

**P2P block locator length cap**

Confidence: high on the missing request-locator count cap and
scan-before-response-cap behavior from local proof tests; medium on impact
because protocol message size and inbound timeout/load-shed limits bound the
damage.

Inbound `getblocks` and `getheaders` locators are deserialized as
`Vec<block::Hash>` and forwarded unchanged to state. The state search then scans
the locator until it finds a chain intersection. Response sizes are capped at
500 hashes or 160 headers, but the request locator can contain tens of thousands
of hashes under the protocol message-size limit. Local proof tests show both
wire decode acceptance above the downstream response caps and state lookup
scanning beyond those caps to find a late tip-hash intersection.

Suggested public fix direction: add an explicit inbound locator length cap and
truncate or reject oversized locators before state lookup.

**P2P inventory routing poisoning**

Confidence: high on the unsolicited `notfound` state mutation; medium on
practical impact because the registry is bounded, self-scoped to the peer's
transient address, and expires after roughly one to two rotation intervals.

The earlier unbounded inventory-growth hypothesis remains eliminated: the
registry caps retained hashes and peers per hash. The sharper issue is routing
state correctness. `register_inventory_status()` records inbound
`Message::NotFound` values as missing inventory before the connection state
machine decides whether the message was a solicited response. The connection
handler later treats unsolicited `notfound` as unused, but by then the wrapper
has already had a chance to update the registry.

This lets peers mark themselves missing for attacker-chosen block or transaction
hashes, which can shrink the candidate set for single-hash `BlocksByHash` and
`TransactionsById` routing. If all ready peers are marked missing for a hash,
`route_inv()` can return synthetic `NotFoundRegistry` without trying a peer.

Durable current-behavior test
`unsolicited_notfound_registers_missing_inventory_today` sends a
`Message::NotFound` through `register_inventory_status()` without any request
correlation context and observes the sending peer in
`InventoryRegistry::missing_peers(block_hash)`.

Suggested public fix direction: only record `notfound` as missing inventory when
it is correlated with an outstanding inventory request, and keep the existing
`MissingInventoryCollector` path for Zebra-originated request failures.

**P2P peer-set unready retention**

Confidence: medium-high as public availability hardening.

The reviewed production paths do not show a remote-only way to force the
peer-set max-size panic, unbounded crawler demand, or unbounded inventory growth.
Inbound connections are gated before handshake, the `MorePeers` demand channel
is bounded by the outbound connection limit, demand is dropped while outbound
connections are at limit, and the inventory registry has explicit hash and
peer-per-hash caps.

The remaining lead is the unready peer set. `poll_peers()` expects connected
peers to become ready within a few minutes, timeout, or close, but also leaves a
TODO to drop peers that overload Zebra with inbound messages and never become
ready. The per-peer connection loop documents that inbound peer messages can
delay Zebra-originated requests to that same peer. A distributed attacker could
therefore occupy many bounded peer slots with handshaked peers that keep their
connections busy and reduce the ready peer pool until timeout or overload
handling clears them.

The inbound `mempool` message path was also checked. It collects all mempool
transaction IDs before the network layer sends an `inv` response. With default
configuration this is bounded by the 80,000,000 byte mempool cost limit and the
10,000 minimum transaction cost, so it is below the 25,000 transaction-inventory
message cap. If operators substantially raise `mempool.tx_cost_limit`, it
becomes an O(n) peer-triggered work item worth rate-limiting.

Suggested public fix direction: track unready-entry time in `PeerSet` and
disconnect peers that remain continuously unready beyond a conservative
threshold. Add tests for stale-unready eviction, over-producing internal
`Discover` streams, saturated crawler demand, and noisy inbound peers.

**P2P BIP37 filter message parsing**

Confidence: high on current parser/connection behavior from local proof tests;
medium on public availability impact.

Zebra documents BIP37 `filterload`, `filteradd`, and `filterclear` as ignored
because it does not implement bloom filters. The codec still parses those
messages. `filteradd` takes `min(body_len, 520)` as the data length, so a peer
can send a body up to the global protocol message limit and Zebra will parse the
first 520 bytes, accept the remaining bytes as generic extra data, and then
consume the ignored message. `filterload` validates body length, but does not
enforce the documented `hash_functions_count <= 50` rule. Empty-body messages
such as `mempool`, `getaddr`, `filterclear`, and `verack` also accept discarded
extra bytes under the codec's general extra-fields policy. Downstream handling
differs: `getaddr` and `mempool` still enter the inbound request service,
`verack` is treated as a duplicate handshake, and unsupported BIP37 filter
messages are consumed before inbound service readiness/timeout/load-shed
handling.

Local proof tests now cover oversized `filteradd` truncation, `filterload` with
`hash_functions_count = 51`, non-empty `filterclear` / `mempool` / `getaddr` /
`verack` messages, and BIP37 filter messages being consumed without any inbound
service request. See `docs/analysis/p2p-bip37-filter-message-hardening-note.md`.

Because ignored filter messages are marked `Consumed`, they do not pass through
the inbound request service, so they do not directly trigger the normal inbound
service readiness/timeout/overload handling used for request work. The global
message size cap and peer limits bound the impact, but Zebra does not need to
spend normal peer-processing work on unsupported filter messages.

Suggested public fix direction: reject unsupported BIP37 messages at the codec
or negotiated-services boundary, reject oversized `filteradd` instead of
truncating it, enforce `filterload`'s hash-function cap, and add exact empty-body
tests for request-like messages such as `mempool` and `getaddr`.

**P2P block, transaction, and header parse strictness**

Confidence: high for current behavior from local proof tests; medium on
practical exploitability because this is malformed-peer handling rather than
invalid-object acceptance.

`CountedHeader` deserialization reads and ignores the per-header transaction
count in `headers` messages even though Zebra serializes counted headers with a
zero count and the type documents that the count is always zero. The outer
`headers` vector count is capped, so this is protocol conformance rather than
allocation growth.

The fresher shape is that the P2P codec accepts extra bytes after a successfully
parsed `block` or `tx` payload. Block and transaction parsers read through
`reader.take(MAX_BLOCK_BYTES)`, while the P2P body limit is 2 MiB. The codec
computes remaining bytes after command-specific parsing but logs them as extra
data and still returns the parsed `Message::Block` or `Message::Tx`. A peer can
therefore send a valid block or transaction prefix plus trailing junk, including
a total body above the 2,000,000-byte object limit but below the 2 MiB P2P
message cap, as long as the parsed prefix object itself completes within the
2,000,000-byte object limit.

The parsed prefix is still handled by normal consensus or mempool checks, so
this is not consensus acceptance of the junk suffix. Suggested public fix:
reject nonzero counted-header transaction counts and require exact body
consumption for `block` and `tx` messages. See
`docs/analysis/p2p-block-header-parse-strictness-note.md`.

Local proof tests `counted_header_nonzero_transaction_count_is_accepted_today`,
`headers_message_nonzero_transaction_count_is_accepted_today`,
`block_message_with_trailing_bytes_is_accepted_today`,
`tx_message_with_trailing_bytes_is_accepted_today`,
`block_message_padded_past_max_block_bytes_is_accepted_today`, and
`tx_message_padded_past_max_block_bytes_is_accepted_today` confirm the current
permissive behavior.

**P2P getaddr empty-cache rescan**

Confidence: medium-high on current behavior; medium-low on severity.

The broad concern that every remote `getaddr` clones, filters, shuffles, and
truncates the whole address book is eliminated for the normal non-empty cache
path. Zebra caches a partial peer-address response for ten minutes and repeated
`Request::Peers` calls reuse that cached response.

The remaining hardening lead is the empty-refresh path. If
`CachedPeerAddrResponse::try_refresh()` finds no gossipable peers, it does not
advance `refresh_time`. The next inbound `getaddr` can therefore immediately
call `AddressBook::fresh_get_addr_response()` again. On a stale or isolated node
with a large retained address book but no currently gossipable peers, repeated
small `getaddr` requests can force repeated full address-book clone/filter and
shuffle attempts that return `Nil`.

Suggested public fix direction: briefly cache empty refreshes or advance the
next-refresh deadline after empty results, and add per-connection `getaddr`
response throttling. See
`docs/analysis/p2p-getaddr-response-amplification-note.md`.

**Address-book ban cleanup assumes same-IP entries are contiguous**

Confidence: high on the cleanup bug; medium on security impact.

After inserting a banned IP, `AddressBook::update()` tries to remove all entries
for that IP by walking `by_addr.descending_keys()` with `skip_while` and
`take_while`. That assumes all entries for the same IP appear contiguously.
They do not: `by_addr` is ordered by `MetaAddr` reconnection priority, and
`MetaAddr::Ord` compares state, timestamps, peer preference, and services before
IP/port.

A temporary unit test with two `127.0.0.1` entries separated in ordering by a
`127.0.0.2` entry failed as expected: after banning `127.0.0.1:8233`, the
address book still returned an entry for that same banned IP. The ban map still
blocks future updates and peer-set ban watching still drops active services, so
this is not currently a confirmed ban bypass. The residual effects are stale
address-book capacity use, wasted candidate-selection ticks, and possible stale
cache/gossip eligibility for leftover score-zero entries until they age out.

Suggested public fix direction: collect all keys whose `addr.ip() == banned_ip`
before removing, and filter banned IPs in cache/gossip/reconnection paths as
defense in depth. See
`docs/analysis/address-book-ban-noncontiguous-ip-cleanup-note.md`.

**P2P compact-block relay**

Result: eliminated as an implemented parser/reconstruction vulnerability.
Zebra's P2P `Message` enum, external wire codec, and internal request/response
types do not implement `sendcmpct`, `cmpctblock`, `getblocktxn`, or `blocktxn`.
Repo-wide symbol search only found lightwalletd test protobuf `CompactBlock`
definitions, not Zebra P2P compact-block relay code.

Residual public hardening: add byte-level tests proving compact-block-family
wire commands are consumed by the unknown-command path without creating typed
messages or retained connection state, including when they arrive while a peer
request is pending.

**P2P unsolicited full-block decode**

Confidence: medium-high as public availability hardening.

The external codec eagerly decodes any incoming `block` frame into
`Message::Block(Arc<Block>)` using `block_in_place()` plus Rayon before the
connection layer decides whether the block is requested or useful. In
`AwaitingRequest`, unsolicited full blocks are logged as unsolicited/canceled
responses and returned as `Unused`, after the expensive parse has already
happened. During `BlocksByHash`, a mismatched decoded block is ignored and the
handler keeps waiting for the requested hashes until completion or
`REQUEST_TIMEOUT`.

Unsolicited `tx` has the same eager decode property, but after decode it maps to
`PushTransaction` and enters the inbound service and mempool queue/verification
limits. The full-block path is weaker because unsolicited full blocks are not
mapped into inbound block-download load shedding; that inbound queue only
applies when Zebra first accepts an advertised block hash and downloads the
block through the normal inbound downloader.

Suggested public fix direction: count unexpected full `block` messages per
connection and disconnect or penalize peers after a small threshold; fail a
pending `BlocksByHash` request after repeated mismatched full blocks rather than
waiting until timeout; consider a lazy/raw body boundary for expensive message
types so the state machine can reject useless full blocks before deserialization.

**Indexer gRPC exposure and stream limits**

Confidence: medium-high on the missing server-level auth/concurrency limits;
medium on practical impact because the server is opt-in and documented as unsafe
to bind publicly.

The indexer gRPC server is disabled by default, but when `indexer_listen_addr` is
configured it builds a plain tonic `Server::builder()`, exposes reflection, and
adds `IndexerServer::new(...)` without an auth interceptor,
`concurrency_limit_per_connection`, `timeout`, `max_concurrent_streams`, TLS, or
load shedding. The indexer API exposes three server-streaming methods; each
client stream spawns a task and gets a 64-message RPC response buffer. The
non-finalized-state stream serializes full blocks and is backed by a state-side
listener with a 1,000-entry block-reference buffer.

A sharper cancellation edge exists inside those stream tasks: `ChainTipChange`,
`NonFinalizedStateChange`, and `MempoolChange` all wait on their source event
before attempting `response_sender.try_send(...)`. They do not race those waits
against `response_sender.closed()`. If a client opens a stream and disconnects
while the node is idle, the task can remain parked until the next relevant
event. For `NonFinalizedStateChange`, the parked RPC task also holds the
state-side listener receiver, keeping the listener task alive until the next
non-finalized-state update lets the RPC task observe the closed response
channel. See
`docs/analysis/indexer-idle-stream-disconnect-retention-note.md`.

The same note tracks an adjacent `MempoolChange` reliability edge: the stream's
`while let Ok(change) = mempool_change.recv().await` loop treats broadcast lag
as terminal channel closure, while Zebra's internal mempool gossip subscriber
logs lag and keeps running.

Suggested public fix direction: keep this localhost-only unless auth/TLS is
added; apply conservative tonic connection/stream/concurrency limits; consider
gating reflection behind a development option; revisit per-client buffers for
full-block streams; and race source-event waits against response-channel closure
so idle disconnects are cleaned up promptly. Treat `MempoolChange` lag as a gap
or recoverable warning rather than upstream closure.

**Indexer MempoolChange privacy**

Confidence: high on the data exposed by the stream; medium on practical impact
because the server is opt-in, feature-gated, and exposure depends on binding the
indexer port beyond trusted local clients.

The optional `MempoolChange` gRPC method subscribes each caller to Zebra's live
mempool-change broadcast channel and streams the change kind, mined transaction
hash, and V5 authorization digest when present. The API is useful for trusted
indexers, but if `rpc.indexer_listen_addr` is exposed to a shared network, any
unauthenticated subscriber can observe this node's local mempool contents and
timing in real time. This is sharper than generic node-state exposure because
`UnminedTxId` deliberately redacts witnessed IDs in logs, while the indexer
stream sends the authorization digest component to remote clients.

Suggested public fix direction: document `MempoolChange` as a local mempool
privacy feed, not only a state-query API; keep it localhost-only unless auth/TLS
is added; and consider splitting low-privacy mined-ID notifications from a
privileged detailed stream that includes V5 authorization digests. See
`docs/analysis/indexer-mempool-change-privacy-note.md`.

**TrustedChainSync indexer validation boundary**

Confidence: medium-high on the code path and missing checks; medium on practical
severity because this is an opt-in read-state mirror whose name already signals
a trusted upstream.

`TrustedChainSync` connects to a caller-supplied plain HTTP indexer endpoint and
imports non-finalized `BlockAndHash` stream messages into a read-state mirror.
`BlockAndHash::decode()` separately deserializes the supplied block bytes and
the supplied hash bytes; it does not recompute the block hash and reject a
mismatch. The syncer then wraps the pair with
`SemanticallyVerifiedBlock::with_hash(...)` and calls lower-level
`NonFinalizedState::commit_new_chain()` / `commit_block()` directly. That skips
the normal write-service `validate_and_commit_non_finalized()` entry point,
which first calls `initial_contextual_validity()` for recent-chain PoW,
difficulty, time, height, parent, and finalized-nullifier checks.

This does not let untrusted P2P peers corrupt a normal Zebra node. It matters
for deployments that treat a remote indexer endpoint as merely "another Zebra"
rather than fully trusted infrastructure: the local mirror can answer read/index
queries from a poisoned non-finalized view if the upstream is malicious,
compromised, or misconfigured. See
`docs/analysis/trusted-chain-sync-indexer-validation-boundary-note.md`.

There is a separate best-tip forwarding issue in the same trusted-mirror helper:
`TrustedChainSync::spawn()` creates an unsupervised task that subscribes to the
upstream indexer's `ChainTipChange` stream, treats the streamed best-tip hash as
if it should be present in the local finalized DB, and returns permanently when
that hash is absent. On an active chain, best tips are normally non-finalized, so
an ordinary upstream best-tip update can stop this forwarding task for the rest
of the mirror lifetime. The separate non-finalized-state sync loop often masks
that failure, so this remains public mirror robustness hardening. See
`docs/analysis/trusted-chain-sync-best-tip-forwarder-note.md`.

Suggested public fix direction: recompute and check `hash == block.hash()` in
`BlockAndHash::decode()`, route trusted-sync commits through the normal
contextual validation helper or explicitly call `initial_contextual_validity()`,
make the finalized-tip updater supervised by the returned sync handle, treat
`ChainTipChange` messages as catch-up triggers rather than finalized-tip data,
and document the indexer endpoint as authenticated/local/trusted infrastructure.

**RPC address-index query bounds**

Confidence: medium-high on missing method-level caps; medium-low on practical
impact because JSON-RPC is disabled by default and cookie-authenticated by
default.

`getaddressbalance`, `getaddressutxos`, and `getaddresstxids` all accept
caller-supplied transparent address vectors. The validation path parses and
deduplicates them, but does not enforce a maximum original vector length or
maximum final address set size. `getaddresstxids` defaults missing `start` to
height 0 and missing or zero `end` to the current chain tip, so callers can ask
for whole-chain indexed searches. The state helpers use address indexes, but
collect all matching UTXOs or transaction IDs into maps/sets without a
`LIMIT`-style cap.

Suggested public fix direction: add explicit per-method limits for address
count, height-range width, and returned item count; reject over-limit requests
with a clear JSON-RPC error before state lookup.

**RPC solution-rate window bounds**

Confidence: high on missing cap; medium-low on practical impact because
JSON-RPC is disabled by default, cookie-authenticated by default, and the scan is
bounded by chain length.

`getnetworksolps` and `getnetworkhashps` accept `num_blocks: Option<i32>`.
Missing values default to 120, and zero or negative values use the 17-block
proof-of-work averaging window. But any positive `i32`, including `i32::MAX`, is
converted to `usize` and sent to `ReadRequest::SolutionRate`. The state read
path calls `read::difficulty::solution_rate()`, which performs
`any_chain_ancestor_iter::<block::Header>(...)` and takes
`num_blocks.checked_add(1)` headers, reading backward by height until the limit
or genesis. Existing vector tests accept `Some(i32::MAX)` on a small test chain,
and the snapshot tests still have a TODO for excessive `num_blocks` coverage.

Suggested public fix direction: add a method-level maximum solution-rate window,
reject larger values with an invalid-parameter JSON-RPC error before state
lookup, and apply the same cap to the deprecated alias.

**RPC subtree limit overflow**

Confidence: high on the overflow branch behavior; low practical severity because
`NoteCommitmentSubtreeIndex` is a `u16` type and omitted `limit` already
intentionally allows full-tail reads.

`z_getsubtreesbyindex` accepts `start_index` and `limit` as
`NoteCommitmentSubtreeIndex`, then forwards them into
`ReadRequest::SaplingSubtrees` or `ReadRequest::OrchardSubtrees`. State computes
`end_index` with `start_index.0.checked_add(limit.0)`. If that returns `None`,
state uses an unbounded `start_index..` range, the same branch used for omitted
`limit`. Because both inputs are `u16`, an overflowing explicit limit can only
read the remaining suffix of the subtree index space. Still, explicit bounded
requests should not share the omitted-limit branch.

Suggested public fix direction: keep `limit=None` as the only unbounded case, use
widened arithmetic for explicit limits, clip overflow to `start_index..=u16::MAX`
or reject it with an invalid-parameter error, and add regression tests for the
overflow cases.

**Health endpoint connection retention**

Confidence: medium-high on missing open-connection and request-timeout guards;
medium-low on practical impact because the endpoint is disabled by default and
intended for internal probes.

The optional health endpoint is unauthenticated by design and serves small
HTTP/1 responses for `/healthy` and `/ready`. It has a burst counter of
`MAX_RECENT_REQUESTS = 10_000` over five seconds, but the counter tracks
accepted streams in the current interval, not currently open streams. Each
accepted stream is handed to `http1::Builder::new().serve_connection(...)` in a
spawned task, and I did not find a semaphore or request/header timeout around
that task. Slow clients can therefore hold accepted connection tasks open across
counter resets if the endpoint is exposed outside a trusted network. The counter
is also not called from `handle_request()`, so repeated HTTP/1 keep-alive
requests over one socket are counted once at accept time rather than once per
handled request. A separate `/ready` edge can emit the same WARN log on every
request while peer/sync checks pass but `remaining_sync_blocks` is still `None`;
with Sentry configured, WARN events are exported as Sentry logs and breadcrumbs.

Suggested public fix direction: add a global connection semaphore and a short
request/header timeout around health connections, disable keep-alive or count
handled requests, rate-limit the repeated `/ready` warning, and keep documenting
the endpoint as internal-only unless stronger controls are added.

**Mempool pending-output waiter retention**

Confidence: medium-high as a public availability hardening lead. Local unit
tests now confirm the core retention primitive; a broader stress test would
still be useful to quantify growth under full peer/mempool queue limits.

Mempool transaction verification first asks finalized/non-finalized state for
spent transparent outpoints. For mempool transactions whose outpoints are not in
the best chain, the verifier waits up to 60 seconds on
`mempool::Request::AwaitOutput(outpoint)`. `PendingOutputs::queue()` creates or
reuses a `broadcast::Sender` for each missing outpoint. Duplicate waiters for
the same outpoint share one sender, but unique missing outpoints create unique
map entries. Closed waiters are removed only when `PendingOutputs::prune()` is
called. Today that prune is tied to selected storage cleanup paths, such as
mined/conflicting transaction removal, not to every verifier timeout,
cancellation, rejection, or ordinary `CheckForVerifiedTransactions` poll.

Active verifier pressure is bounded by `MAX_INBOUND_CONCURRENCY = 25`, the
73-second download/verify task timeout, and the verifier's serial
`AwaitOutput` loop. Stale sender retention is not bounded by that active-task
count; it is bounded by unique missing outpoints since the last matching
`respond()`, `prune()`, or `clear()`.

Suggested public fix direction: add explicit global and per-outpoint waiter caps,
return a bounded mempool-unavailable error when the cap is reached, and prune
closed pending-output channels regularly in `poll_ready()` or after verifier
timeouts.

## Additional Eliminated-Lead Details

**Coinbase-height serialization panic**

Follow-up concern: `write_coinbase_height()` panics if asked to serialize a
`block::Height` greater than `Height::MAX`, and `Input::new_coinbase()` calls
`height.coinbase_zcash_serialized_size()` before checking the remaining
coinbase data length.

Result: eliminated as a current externally reachable parser or consensus
finding. Untrusted coinbase inputs are deserialized through
`Input::zcash_deserialize()`, which bounds the coinbase script length before
allocation and then calls `parse_coinbase_height()`. The parser only constructs
4-byte heights in `8_388_608..=Height::MAX`; encoded heights above `Height::MAX`
return a parse error. Public height conversions from strings, `u32`, `u64`,
`usize`, `i32`, and `BlockHeight` also route through `FromStr`, `TryFrom`, or
`TryIntoHeight` bounds checks. The remaining panic is therefore an internal
constructor invariant: code that manually creates `Height(Height::MAX_AS_U32 +
1)` and then builds or serializes a coinbase can panic, but I did not find a
node, consensus, mempool, RPC, or P2P path where attacker-controlled bytes
construct that invalid height and then serialize it.

Residual public hardening: make `write_coinbase_height()` return
`io::Error::other("invalid coinbase height")` instead of panicking, and consider
making `Height`'s field private or adding a checked constructor for new
non-test call sites.

**Defaulting and error-suppression recheck**

Follow-up concern: four `unwrap_or_default()` / default-to-zero paths could
hide attacker-influenced errors: transparent address-balance `received`
decoding, ZIP-317 unpaid-action calculation, GBT proposal helper time selection,
and verbose `getrawmempool` descendant-fee aggregation.

Result: eliminated for private disclosure. Transparent address-balance
defaulting is local disk-format compatibility, ZIP-317 defaulting is the
intended negative-to-zero clamp, and the proposal helper default is not used by
external proposal validation. Verbose `getrawmempool` descendant-fee overflow is
the only remotely influenced sink, but it appears unreachable from valid mempool
contents under Zebra's value/conflict invariants and would affect RPC accounting
only. See `docs/analysis/defaulting-error-suppression-recheck-note.md`.

**RPC compatibility middleware response buffering**

Follow-up concern: Zebra's JSON-RPC 1.0 compatibility middleware collects the
full jsonrpsee response body before rewriting `"jsonrpc":"2.0"` to
`"jsonrpc":"1.0"`.

Result: eliminated as an unbounded-buffering bypass. Zebra installs the
compatibility middleware before configuring jsonrpsee's maximum response body
size, but jsonrpsee applies that limit while serializing method responses:
`handle_rpc_call()` receives `max_response_size`,
`BatchResponseBuilder::new_with_limit()` caps batch response accumulation, and
`MethodResponse::response()` serializes through `BoundedWriter`. So the
compatibility layer buffers the already-limited response or the already-limited
oversized-response error.

Residual public hardening: the compatibility layer has no local response-size
guard and does not re-check the serialized size after legacy response rewriting.
That means a response close to jsonrpsee's cap could be emitted slightly over
Zebra's configured cap after the JSON-RPC 1.0 rewrite delta. This does not look
like private disclosure material, but a middleware-local `Limited` collection
and post-rewrite size check would make the cap self-contained. This also does
not eliminate the separate unlimited batch-count issue.

**RPC `z_gettreestate` response size**

Follow-up concern: `z_gettreestate` serializes Sapling and Orchard note
commitment trees, so a mature chain might force a response proportional to all
historical note commitments.

Result: eliminated as a large-response issue. The method does fetch the full
block first, then fetches the Sapling and Orchard trees by block hash and calls
`to_rpc_bytes()`. But Zebra stores those trees as incremental frontiers, and the
legacy RPC `CommitmentTree` serialization contains only an optional left node,
an optional right node, and at most `MERKLE_DEPTH - 1` optional parent nodes.
With Sapling/Orchard depth 32 and 32-byte nodes, the worst-case raw
`finalState` is 1090 bytes per pool, or 2180 hex characters before JSON
overhead. The response therefore scales with tree depth, not total note
commitment count. See
`docs/analysis/rpc-z-gettreestate-response-bound-note.md`.

Residual public hardening: `z_gettreestate` still fetches the full block where a
header lookup would be enough, as the code TODO notes. If a reorg removes the
block between the initial block lookup and later active-pool tree lookups, the
RPC can also serialize empty commitments where an internal state-race error
would be clearer. These are ordinary RPC correctness/efficiency hardening, not
an unbounded-response vulnerability.

**RPC `sendrawtransaction` retry queue**

Follow-up concern: `sendrawtransaction` stores transactions for later retry, so
an attacker-reachable RPC service might accumulate submitted transactions.

Result: eliminated as an unbounded retry-queue memory issue. The retry channel
and queue both use `CHANNEL_AND_QUEUE_CAPACITY = 20` in `zebra-rpc/src/queue.rs`,
and `Queue::insert()` removes the oldest transaction when insertion takes the
queue over that cap. Existing queue property tests cover the size limit and
oldest-item eviction behavior. The direct RPC call still awaits the mempool
queue result, but orphan-output waits are bounded by the mempool output lookup
timeout and the mempool download/verification timeout. That makes this a
long-lived-request hardening consideration for exposed RPC deployments, not a
separate unbounded retry-queue finding.

**Non-finalized spent-output panic guards**

Follow-up concern: `Chain` panics in non-test builds if a transparent input is
applied or reverted without a corresponding entry in
`ContextuallyVerifiedBlock.spent_outputs`.

Result: eliminated as a private remote DoS lead. Ordinary P2P block verification,
RPC `submitblock`, and `getblocktemplate` proposal validation all route through
`validate_and_commit_non_finalized()` or the same lower-level non-finalized
commit path. Before `Chain` mutation, `NonFinalizedState::validate_and_commit()`
calls `check::utxo::transparent_spend()`, which walks every transparent input and
either fills the spent-output map or returns typed validation errors for missing,
duplicate, or early spends. `ContextuallyVerifiedBlock::with_block_and_spent_utxos()`
then extends the map with same-block `new_outputs`.

`TrustedChainSync` still bypasses `initial_contextual_validity()` and preserves
the public trusted-indexer boundary concern, but it does not bypass the
spent-output builder that protects this specific invariant. See
`docs/analysis/non-finalized-spent-output-panic-reachability-note.md`.

**Exact finalized-boundary downloader filter**

Follow-up concern: sync and inbound downloader early-height filters might admit
stale blocks exactly at the finalized boundary, doing avoidable decode and
verification work before later finalized-state rejection.

Result: confirmed as public resource hardening. The write service finalizes
while the best non-finalized chain length is greater than
`MAX_BLOCK_REORG_HEIGHT`, so in steady state a best tip at height `T` implies a
finalized tip around `T - MAX_BLOCK_REORG_HEIGHT`. Both sync and inbound
downloaders compute `min_accepted_height = tip_height - MAX_BLOCK_REORG_HEIGHT`,
but reject only blocks with `block_height < min_accepted_height`, admitting the
exact boundary height.

Later verifier/state checks should reject same-height alternates to finalized
history, so this is bounded download/decode/verification work rather than a
consensus split. See
`docs/analysis/finalized-boundary-downloader-height-filter-note.md`.

**Transparent spent-output alignment**

Follow-up concern: `spent_utxos()` stores previous transparent outputs as
`Vec<Option<transparent::Output>>` and then calls `flatten()`. If a mixed
best-chain plus mempool UTXO path left a non-coinbase input slot empty, the
previous-output vector could become short or misaligned before script
verification or `VerifiedUnminedTx` storage.

Result: eliminated as a live vulnerability in the current code. Valid
non-coinbase transactions have only `PrevOut` inputs, and every successful
current branch fills `spent_outputs[input_idx]` by the original input index.
Best-chain and known UTXOs fill their slot during the first pass; mempool-only
outputs are recorded as `(input_idx, outpoint)` and filled into the same slot
after `AwaitOutput`. Missing or unavailable outputs return
`TransparentInputNotFound` before `VerifiedUnminedTx` is constructed. Downstream
script verification also fails closed on length mismatch, and mempool storage
rejects mismatched vectors before standardness policy.

Residual public hardening: replace the final `flatten()` with an explicit
missing-slot check, remove the stale TODO that says mempool outputs are appended
last, and add a two-input mixed source regression test. See
`docs/analysis/transparent-spent-output-alignment-note.md`.

**State block-write queues**

Follow-up concern: `zebra-state` uses unbounded MPSC channels for finalized and
non-finalized block writes, and the non-finalized out-of-order block queue has
no explicit global length cap.

Result: eliminated as a separate unbounded externally reachable queue. The
state write channels are internal, and the attacker-facing paths that feed them
go through bounded layers first: the consensus router buffer is small, gossiped
block downloads are capped by `full_verify_concurrency_limit` and
`MAX_INBOUND_CONCURRENCY`, inbound gossip enforces one in-flight block download
per advertiser IP, and sync downloads pause at their configured lookahead.
Checkpoint verification also has a per-height queued-block cap and rejects
heights outside the checkpoint range. RPC `submitblock` can still hold a request
open when verifier/state work waits for a missing parent, but that is the
existing miner-RPC timeout finding rather than unbounded state queue growth.

Residual public hardening: the internal `unbounded_channel()` calls rely on
cross-component invariants. Replacing them with sized channels derived from the
sync/inbound lookahead limits, or adding debug metrics/assertions for queue
capacity expectations, would make the bound local and easier to audit.

**Queued-block `AwaitUtxo` scope**

Follow-up concern: because `AwaitUtxo` checks queued missing-parent block UTXOs
before committed-chain state, an output from an unrelated queued block might
help another block pass semantic verification and reach state commit.

Result: confirmed as semantic-stage influence, eliminated as an acceptance
bypass, and eliminated for mempool. `QueuedBlocks::known_utxos` is queue-global,
so a queued missing-parent block can satisfy a later block/proposal
`AwaitUtxo`. But `SemanticallyVerifiedBlock` does not carry those spent-output
lookups into state, and the write/proposal path reruns
`check::utxo::transparent_spend()` against the selected parent chain plus
finalized state before constructing a `ContextuallyVerifiedBlock`. If the queued
UTXO is not valid in that chain context, the block is rejected before state
mutation. Mempool transaction verification uses `UnspentBestChainUtxo` and
optional mempool `AwaitOutput`, not `AwaitUtxo`, so queued block UTXOs do not
affect mempool admission. See
`docs/analysis/queued-block-awaitutxo-scope-note.md`.

**RPC auth before body collection**

Follow-up concern: an unauthenticated caller might force Zebra to collect and
rewrite a large JSON-RPC request body before cookie authentication runs.

Result: eliminated. `HttpRequestMiddleware::call()` checks the
`Authorization` header with `check_credentials()` and returns an authentication
error before it fixes headers or calls `request_to_json_rpc_2()`. Body
collection happens only inside `request_to_json_rpc_2()`, where the body is
wrapped in `http_body_util::Limited` using Zebra's `submitblock`-sized request
limit. The remaining RPC exposure issues are the public batch-count and
long-running method hardening items, not unauthenticated body buffering.

**RPC cookie existing-file permissions**

Follow-up concern: the cookie-file hardening might only protect newly created
files, not stale regular files left by older versions, crashes, or custom
volume setups.

Result: confirmed as a local credential-exposure hardening issue.
`cookie::write_to_disk()` rejects symlinks, then opens the path via
`OpenOptions` with `write(true).create(true).truncate(true)` and
`mode(0o600)`. On Unix, the mode is only used for newly created files. Durable
test `cookie_write_preserves_existing_regular_file_permissions_today`
pre-creates `.cookie` as `0644` and shows the file remains `0644` after the
fresh cookie secret is written. See
`docs/analysis/rpc-cookie-existing-file-permissions-note.md`.

**RPC cookie lifecycle cleanup**

Follow-up concern: the cleanup code for cookie auth might not be reached on the
actual `zebrad` startup and shutdown path.

Result: confirmed as a local credential-lifecycle hardening issue.
`RpcServer::start()` writes the cookie before `Server::builder().build()` can
fail, then returns only a spawned `JoinHandle` on success. The `RpcServer`
struct has `shutdown()` / `Drop` cleanup that calls `cookie::remove_from_disk()`,
but the live path does not construct or retain that struct. `zebrad` stores the
returned task handle and aborts it during shutdown. Durable tests
`rpc_server_start_failure_leaves_cookie_today` and
`rpc_server_task_abort_leaves_cookie_today` show both an auth-enabled bind
failure and an aborted auth-enabled server task leave `.cookie` on disk. See
`docs/analysis/rpc-cookie-lifecycle-cleanup-note.md`.

**Docker RPC exposure examples**

Follow-up concern: pass-5 repeatedly treats RPC as disabled by default and
cookie-authenticated by default, but shipped examples might remove both
mitigations.

Result: confirmed as public docs/config hardening.
`docker/docker-compose.lwd.yml` and `docker/docker-compose.observability.yml`
bind RPC to all interfaces, disable cookie auth, and publish the host RPC port.
The mining compose file disables auth and binds all interfaces inside its
Compose network, but does not publish the RPC port by default. See
`docs/analysis/rpc-docker-unauthenticated-public-bind-note.md`.

**RPC `text/plain` browser-origin request forgery**

Follow-up concern: Zebra's compatibility middleware might preserve old
bitcoind/lightwalletd client behavior by accepting HTTP request shapes that
browsers can also send cross-origin without CORS preflight.

Result: confirmed as public RPC hardening. `HttpRequestMiddleware::call()` checks
cookie credentials before body collection, which protects the default
cookie-authenticated path. But when cookie auth is disabled, the middleware
rewrites missing `Content-Type` and `Content-Type: text/plain...` to
`application/json` before handing the body to jsonrpsee. The nearby security
comment correctly rejects `application/x-www-form-urlencoded` so browser forms
cannot target a local RPC port, but `text/plain` is also a browser-simple
request content type. A malicious web page that can reach an auth-disabled
local, container, or public RPC endpoint can therefore try to trigger
side-effecting or expensive RPC calls without reading the response.

This does not bypass the same-origin policy for response reads, and modern
private-network access enforcement can reduce practical public-to-localhost
reachability. The hardening gap is that operators may believe localhost plus
disabled cookie auth is isolated from browser-origin traffic. See
`docs/analysis/rpc-text-plain-csrf-hardening-note.md`.

**RPC jsonrpsee connection and subscription defaults**

Follow-up concern: Zebra might rely on jsonrpsee defaults for connection,
request-concurrency, or subscription limits in a way that leaves a broader
unbounded RPC exposure than the batch-count finding.

Result: eliminated as a separate issue. Zebra's JSON-RPC server calls
`.http_only()`, so WebSocket subscriptions are disabled for this service.
jsonrpsee-server 0.24.10 defaults `max_connections` to 100 and enforces that
with a `ConnectionGuard` permit before handling each HTTP request. Zebra does
not override the value, but it inherits the cap. The direct missing RPC knob is
still `set_batch_request_config(...)`: within one permitted request, jsonrpsee
defaults to `BatchRequestConfig::Unlimited` and loops through every batch
element sequentially.

**RPC pre-guard HTTP connection retention**

Follow-up concern: the previous jsonrpsee connection-limit conclusion might not
cover Zebra's outer HTTP compatibility middleware and accepted TCP streams that
have not yet produced a complete HTTP request.

Result: confirmed as public RPC availability hardening. Zebra's compatibility
middleware checks cookie credentials before body collection, so wrong-auth
requests do not reach large-body JSON parsing or method dispatch. But for
auth-disabled requests or clients with valid credentials, the middleware then
collects the request body with Zebra's compatibility limit and rewrites the
JSON-RPC body before calling the inner jsonrpsee service. Source review of
jsonrpsee-server 0.24.10 shows the `ConnectionGuard` permit is acquired inside
that inner service path, after Zebra's middleware has already started handling
the request.

At the accept layer, jsonrpsee also accepts TCP connections and spawns Hyper
connection tasks before a request reaches Zebra's auth middleware or the inner
guard. So headerless or very slow clients can retain connection tasks, and
valid-auth/auth-disabled clients can hold body-collection futures, before the
guard relied on by the earlier batch finding runs. RPC is disabled by default
and this does not bypass cookie auth; the public fix direction is an outer
Zebra-side connection semaphore plus request/header/body timeouts. See
`docs/analysis/rpc-pre-guard-http-connection-retention-note.md`.

**Debug-string `NotFound` error classification**

Follow-up concern: some peer and sync paths classify `NotFound` conditions by
formatted debug strings rather than typed variants, so attacker-controlled error
text might alter restart, retry, or inventory behavior.

Result: eliminated as a current attacker-exploitable issue. The brittle sites
are real: `ChainSync::should_restart_sync()` treats `DownloadFailed` errors as
non-restarting if `format!("{error:?}").contains("NotFound")`, and
`MissingInventoryCollector` suppresses registry feedback if
`SharedPeerError::inner_debug().contains("NotFoundRegistry")`. But the matched
strings are currently produced by local `PeerError::NotFoundResponse` and
`PeerError::NotFoundRegistry` variants. A remote `notfound` message can produce
the intended `NotFoundResponse` behavior, and local inventory routing can
produce the intended `NotFoundRegistry` behavior, but this pass did not find a
free-form remote error-string path that can spoof those classifications. This
should still be converted to typed predicates when `SharedPeerError` no longer
hides the inner variant behind `TracedError`.

**Single-item inventory download panic assertions**

Follow-up concern: sync, inbound gossip, and mempool download tasks assume a
single-item `BlocksByHash` or `TransactionsById` request cannot return the
wrong response type, an empty list, multiple items, or `Missing` status. In the
block paths this is enforced with `assert_eq!(blocks.len(), 1)` and
`available().expect(...)`; in the mempool path, empty transaction responses are
converted to a download error, but `Missing` still hits
`available().expect(...)`.

Result: eliminated as a remote peer panic path. These are internal service
invariants, not directly P2P-decoded messages. The subtle distinction is that
Zebra's direct inbound service can internally return
`Response::Blocks([Missing(_)])` or `Response::Transactions([Missing(_)])`, but
the responder-side connection serializes those missing entries as a wire
`notfound` message. The requester-side connection then converts an all-missing
singleton `notfound` response into `Err(PeerError::NotFoundResponse(_))`, not a
successful `Response::*([Missing(_)])` delivered to the downloader. For a single
requested transaction, no matching transaction plus an unrelated `tx` or
`notfound` becomes `Err(PeerError::NotFoundResponse(_))`, while a matching
transaction finishes as a one-item `Available` response. For a single requested
block, a matching block finishes as a one-item `Available` response; `notfound`
with no blocks becomes `Err(PeerError::NotFoundResponse(_))`; unrelated blocks
are ignored until the request times out. If the local inventory registry says
every ready peer is missing the single item, `PeerSet::route_inv()` returns
`Err(PeerError::NotFoundRegistry(_))` without calling a peer service.

Evidence:

- `zebra-network/src/peer/connection.rs:226-239` and
  `zebra-network/src/peer/connection.rs:276-289` handle transaction no-match,
  partial, and matching response cases.
- `zebra-network/src/peer/connection.rs:343-348` and
  `zebra-network/src/peer/connection.rs:385-396` handle block matching and
  `notfound` cases.
- `zebra-network/src/peer/connection.rs:1475-1520` serializes internal
  `Missing` transaction/block responses as outbound `notfound` messages on the
  P2P wire.
- `zebra-network/src/peer_set/set.rs:991-1063` returns a synthetic
  `NotFoundRegistry` error when all ready peers are locally marked missing.
- Targeted existing tests passed:
  `cargo test -p zebrad inbound_block_empty_state_notfound --lib`,
  `cargo test -p zebrad inbound_tx_empty_state_notfound --lib`, and
  `cargo test -p zebra-network peer_set_route_inv_all_missing_fail --lib`.

**Peer response-sender invariant panics**

Follow-up concern: the peer client/connection boundary has process-fatal
internal assertions around `poll_ready()`/`call()` sequencing and the
`MustUseClientResponseSender` exactly-once response invariant. A malicious peer
might try to force cancellation, timeout, disconnect, heartbeat, or backpressure
timing into an unused or double-used sender drop.

Result: eliminated as a current remote peer panic path. The relevant panic
sites are real internal invariants, but the remote peer does not directly drive
the invalid states. `PeerSet::route_p2c()`, `route_inv()`, and broadcast paths
remove a client from `ready_services`, call it once, then put it into
`unready_services`; `UnreadyService` only returns it to `ready_services` after
`Client::poll_ready()` succeeds again. `LoadTrackedClient` only forwards
`poll_ready()` and `call()` into `PeakEwma<Client>`, so it does not add a second
concurrent caller.

The connection state machine also gates the response sender. In
`State::AwaitingRequest`, the run loop can accept either a peer message or one
client request. In `State::AwaitingResponse`, it no longer polls `client_rx`;
it only waits for cancellation, timeout, or peer messages. Finished handlers
take and send the `tx`; non-ping timeouts send an error and return to
`AwaitingRequest`; ping timeouts send an error then fail the connection; peer
disconnect/serialization errors call `fail_with()` / `shutdown()`, which sends
an error on the in-flight `tx` and drains queued requests. Client cancellation
drops the sender only after the receiver is canceled, satisfying the sender's
drop assertion.

One subtle checked race was the heartbeat task: it owns a clone of the same
bounded request sender and can queue `Ping` requests directly. A temporary proof
test showed that, for the real zero-capacity futures mpsc channel, a heartbeat
sender clone cannot consume readiness already observed by the client sender; the
client sender can still `try_send()` after its own `poll_ready()` succeeds. So
the `Client::call()` `"called call without poll_ready"` panic remains a local
service-contract violation rather than a remote-peer timing primitive.

Evidence:

- `zebra-network/src/peer/client.rs:258-334` enforces exactly-once
  `MustUseClientResponseSender` use, panicking only if the sender is dropped
  while still live and uncanceled or if it is used more than once.
- `zebra-network/src/peer/client.rs:614-668` checks the connection/heartbeat
  tasks and request sender readiness before `call()`, and the `Full` panic is
  reached only if the same sender is called while not ready.
- `zebra-network/src/peer/connection.rs:751-779` accepts client requests only
  from `State::AwaitingRequest`.
- `zebra-network/src/peer/connection.rs:827-943` handles in-flight response
  cancellation, timeout, and peer-message paths without polling another client
  request.
- `zebra-network/src/peer/connection.rs:1733-1800` shutdown-flushes the
  in-flight response sender and then all queued client requests.
- `zebra-network/src/peer/handshake.rs:1004` creates the real connection
  request channel with zero explicit buffer slots, and
  `zebra-network/src/peer/handshake.rs:1142-1148` gives the heartbeat task a
  cloned sender.
- Temporary proof test
  `zero_capacity_mpsc_clone_send_does_not_invalidate_other_sender_ready` passed
  with
  `cargo test -p zebra-network zero_capacity_mpsc_clone_send_does_not_invalidate_other_sender_ready --lib`;
  the probe source was removed after the run.

Residual public hardening: converting these invariant panics into typed
per-peer connection failures would reduce the blast radius if a future wrapper
or refactor violates the service contract.

**Sapling validating-key parser panic**

Result: eliminated as a current remote parser-abort finding. Direct P2P `tx`
messages and transactions inside P2P `block` messages both reach
`Transaction::zcash_deserialize()`, and V4/V5 Sapling spend `rk` bytes are parsed
through `TryFrom<[u8; 32]> for ValidatingKey`. That conversion first calls
`redjubjub::VerificationKey::<SpendAuth>::try_from(value)`, whose locked
`reddsa` dependency validates canonical point decoding before returning `Ok`.
The later `jubjub::AffinePoint::from_bytes(key.into()).unwrap()` is therefore
guarded by that dependency invariant before it checks small-order points.

Evidence:

- `zebra-network/src/protocol/external/codec.rs:450` and `:457` dispatch P2P
  `block` and `tx` bodies into the eager block/transaction deserializers.
- `zebra-chain/src/sapling/spend.rs:217-220` and `:269-272` parse V4/V5 Sapling
  `rk` bytes through `try_into().map_err(SerializationError::Parse)?`.
- `zebra-chain/src/sapling/keys.rs:371-374` calls
  `redjubjub::VerificationKey::<SpendAuth>::try_from(value)` before the
  follow-up small-order check.
- `zebra-chain/src/sapling/keys.rs:417-436` now includes a property test that
  arbitrary bytes accepted by `redjubjub` always re-decode as a Jubjub affine
  point and do not panic in Zebra's `ValidatingKey` conversion.
- Targeted tests passed:
  `cargo test -p zebra-chain validating_key_rejects_malformed_and_small_order_bytes_without_panicking --lib`
  and
  `cargo test -p zebra-chain redjubjub_validating_key_success_implies_affine_decode_success --lib`.

Remaining public hardening: replace the `unwrap()` with explicit error
propagation so the invariant remains local even if dependency behavior changes.
See `docs/analysis/sapling-validating-key-panic-sweep-note.md`.

**Sapling transmission-key public API panic**

Result: confirmed as a public library API panic, but not as a current node-level
remote vulnerability. `zebra_chain::sapling::keys::TransmissionKey::try_from`
documents a fallible parse for malformed, non-canonical, or non-prime-subgroup
Jubjub bytes, but calls `jubjub::AffinePoint::from_bytes(bytes).unwrap()` before
checking whether the `CtOption` is present.

Evidence:

- `zebra-chain/src/sapling/keys.rs:210-226` implements the fallible parser.
- `zebra-chain/src/sapling/keys.rs:219` unwraps the Jubjub decode result before
  checking `is_torsion_free()`.
- A throwaway local repro using
  `TransmissionKey::try_from([0xff; 32])` under `catch_unwind` printed
  `panicked` and the backtrace pointed at `zebra-chain/src/sapling/keys.rs:219`.
- Source search did not find a current validator/RPC call site that feeds
  untrusted bytes into this constructor. Sapling address validation paths use
  `sapling_crypto::PaymentAddress::from_bytes(&data)` instead.

Suggested hardening: mirror `EphemeralPublicKey::try_from` by checking
`possible_point.is_none()` before unwrapping, then add a regression test that
malformed bytes return `Err` without panicking.
See `docs/analysis/sapling-transmission-key-public-api-panic-note.md`.

**Proof and signature batch verifier failure semantics**

Result: eliminated for consensus acceptance. Ed25519, RedJubjub, RedPallas, and
Halo2 global verifiers all wrap their batch verifier in `tower_fallback`.
Fallback retry goes through the single-item verifier, and worker-panic or
dropped-channel paths do not become success. The remaining useful work here is
failure-taxonomy hardening and regression coverage symmetry: Ed25519 has a
fallback test; RedJubjub, RedPallas, Halo2, and Groth16 would benefit from
equivalent tests that distinguish invalid proofs/signatures from verifier
infrastructure failures.

**Value-pool and time consensus checks**

Result: eliminated. `Amount` and `ValueBalance` constrain values through checked
arithmetic before returning consensus-facing balances, and
`miner_fees_are_valid()` maps arithmetic failures into subsidy/overflow errors.
For time, mined transaction locktime uses candidate block time; mempool
locktime asks state for the best-chain next median-time-past; state retries MTP
queries if the finalized tip changes during the read; and candidate block time
is checked against median-time-past in state contextual validation. The adjacent
mining RPC template-time envelope has public correctness mismatches; see
`docs/analysis/time-consensus-parity-note.md` and
`docs/analysis/gbt-time-envelope-mismatch-note.md`.

**State migration and read consistency**

Result: eliminated for private disclosure. Disk format upgrades are ordered and
mark each version only after `prepare()`, `run()`, and `validate()` complete.
The current v27 block-info/address-received upgrade is idempotent across
interruption by skipping already-created `BlockInfo` heights and using per-height
write batches. Indexer spend/nullifier indexes use per-height atomic batches and
leave build metadata set while a rebuild or drop is incomplete, causing the
operation to resume on reopen.

Read-side consistency is handled case-by-case rather than with a global RocksDB
snapshot. Consensus-sensitive MTP and difficulty reads check the finalized tip
before and after multi-step queries and retry on movement. Address-index reads
explicitly compensate for overlap between the cloned non-finalized chain and the
concurrently updated finalized database. That keeps this out of private
disclosure, but it supports the existing public hardening recommendation to add
method-level address/range/result caps.

**Experimental Elasticsearch transport and panic behavior**

Result: public hardening only. The feature is explicitly experimental and not
part of default release binaries, but if an operator compiles it, the live
read-write state service enables Elasticsearch indexing unconditionally. The
client uses Basic auth and `CertificateValidation::None`, so a remote or
shared-network endpoint is a fully trusted boundary despite `https://...`
configuration. The finalized-block bulk indexing path pings the endpoint and
then panics on bulk send errors, JSON parse errors, or Elasticsearch response
errors.

Suggested public fix direction: keep TLS certificate validation on by default,
require an explicit runtime enable knob, and turn indexing request/response
failures into logged exporter failures rather than process panics. See
`docs/analysis/elasticsearch-feature-transport-and-panic-note.md`.

## Workstream Results

| ID | Result | Notes |
| --- | --- | --- |
| A1 ZIP-244 sighash matrix | Existing private finding confirmed; no additional wrapper divergence found | Undefined V5 hash types are rejected, shielded/no-transparent paths use `SIGHASH_ALL`, and script-code handling is delegated through the C++ interpreter callback. |
| A2 Network upgrade activation | Public hardening lead | Mempool uses the next height from state tip and transaction verification checks branch IDs against request height. Proposal validation uses the same semantic block verifier and a cloned state commit. A custom Regtest/Testnet with NU7 activation can still hit a normal-build GBT/internal-miner serialization panic because NU7 V5 coinbase construction is possible while the NU7 branch ID is test-only. Custom networks can also silently inherit a later configured upgrade height as the NU6.1 activation height for one-time lockbox accounting if `nu6_1` is omitted. |
| A3 Anchors/nullifiers across reorgs | Eliminated | Contextual checks use parent-chain plus finalized state; fork/pop tests cover anchor/nullifier rollback behavior. The finalization publication race is eliminated because read requests use the watch-published pre-finalization snapshot, not the writer-private post-finalize/pre-DB state; see `docs/analysis/anchor-nullifier-reorg-a3-revisit-note.md` and `docs/analysis/shielded-reorg-finalization-read-order-note.md`. |
| A4 Checkpoint vs full verification boundary | Eliminated for default networks; custom Regtest/resource hardening | Finalization retains the non-finalized reorg window and pops roots only beyond the configured reorg depth. Mainnet/default Testnet checkpoints cover the pre-Canopy rules Zebra skips, but custom Regtest can be configured with a pre-Heartwood Sapling/Blossom interval and only a genesis checkpoint. Canonical commit has a mandatory-height assert, while proposal validation lacks the equivalent gate; see `docs/analysis/pre-heartwood-sapling-root-checkpoint-coverage-note.md`. Checkpoint NU5/V5 auth-data binding is also eliminated for bad-state persistence: the checkpoint verifier defers the check, but finalized-state commit validates `hashBlockCommitments` before `write_block()`; see `docs/analysis/checkpoint-auth-data-binding-note.md`. Sync and inbound downloaders should also tighten exact finalized-boundary early filtering to avoid bounded stale-block work. |
| A5 Value-pool arithmetic | Eliminated for current default networks; future-gated private heads-up | `Amount` and `ValueBalance` use checked arithmetic; state pool updates reject invalid non-negative balances. The exception is future NU7/ZIP-235 code: under the combined `nu7 + zip235 + tx_v6` build on an NU7-active network, the ZIP-235 miner-fee share check can panic on high but representable block miner fees because it unwraps an intermediate `block_miner_fees * 6` `Amount` multiplication before dividing by 10. |
| A6 `zebra-script` FFI safety | Eliminated beyond known sighash issue | Input index and previous-output lengths are preflighted, callback failure returns a per-call random dummy digest, and no new attacker-controlled panic/acceptance path was found. |
| A7 Batch verifier failure semantics | Eliminated for acceptance; public taxonomy hardening | `tower-fallback` retries failed batch paths through single-item verification; dropped-worker/panic paths do not accept invalid proofs/signatures, but Groth16 infra errors and verifier-drop panics should be cleaned up as public hardening. |
| B1 P2P deserialization/rate limits | Public hardening plus private heads-up candidate | Prior addrv2 allocation class has explicit caps/tests; inventory registry, peer-count panic, crawler demand growth, gossiped-address services unwrap, coinbase-height serialization panic, and single-item inventory download panic hypotheses were eliminated as unbounded remote paths. However, unsolicited `notfound` can still poison bounded inventory-routing state for the sending peer, and unrelated in-flight `notfound` can complete active block/transaction download requests as missing. Compact-block relay is not implemented in this checkout. V5 `MSG_WTX` request exactness should be tightened, transaction `getdata` should cap IDs before mempool lookup, P2P `mempool` requests should not enumerate the full local mempool before response truncation, transaction `inv` advertisements should be capped before mempool queue bookkeeping, `getblocks` / `getheaders` should cap locator length before state lookup, non-empty junk `FindBlocks` responses should not clear stall tracking or dominate sync download order, long-lived unready peers should be age-evicted, repeated empty-cache `getaddr` refreshes should be throttled, ignored BIP37 filter messages should be rejected or tightly size-checked, header-only maximum-body frames should not reserve the full declared body before body bytes arrive, counted headers should reject nonzero transaction counts, `block` / `tx` message parsing should reject trailing junk bytes, malformed no-height block attribution should be preserved for scoring, lossy misbehavior report transport should be made reliable, address-book ban cleanup should remove all same-IP entries independent of ordering, and unsolicited full-block decode work should be penalized or made lazy. The confirmed private heads-up candidate in this workstream is the non-default `max_connections_per_ip > 1` address-book ban panic on remote-influenced misbehavior updates. |
| B2 RPC auth and exposure | Public hardening plus private heads-up candidate | JSON-RPC auth is checked before body collection, request/response bodies are size-limited, new cookie files use restrictive permissions, symlinks are rejected, jsonrpsee supplies a 100-permit inner request/connection guard, and JSON-RPC is disabled by default. `getblocktemplate` `longpollid` parsing can panic on a 46-byte non-ASCII UTF-8 string before invalid-parameter handling, and Zebra's binary profiles use `panic = "abort"`. Height-based `getblock <height> 2` can panic if a non-finalized reorg makes the header/depth and block-body subrequests observe different blocks at the same height. `invalidateblock` can panic when the target is a non-finalized chain root, and can also panic when sequentially invalidating same-height sibling fork tips in non-finalized state. `reconsiderblock` can panic when repeated after a successful reconsider because the invalidated entry is removed from a clone instead of live state. Existing loose regular cookie files are not chmod-tightened when rewritten, so stale insecure files can expose fresh RPC credentials to local readers. Cookie cleanup is also not reached on auth-enabled bind failure or normal `zebrad` task abort, leaving stale auth material on disk. Some Docker examples bind RPC to `0.0.0.0`, disable cookie auth, and publish the RPC port. The `text/plain` HTTP compatibility rewrite should be tightened or gated because browsers can send no-preflight cross-origin POSTs to auth-disabled reachable RPC endpoints. Zebra should add an outer RPC admission guard and request/header/body timeouts before compatibility body collection, because jsonrpsee's guard is reached inside the inner service. Batch request count still inherits jsonrpsee's unlimited default. `getrawtransaction` should fetch V5 transaction bodies from the caller-supplied block context instead of a later mined-ID any-chain lookup. Verbose RPC response builders should tighten `in_active_chain`, Orchard field presence, Orchard action/signature pairing semantics, and height-based `getblock` snapshot consistency. Address-index RPCs should cap address counts, ranges, and returned items. Solution-rate RPCs should cap `num_blocks` before state ancestor scans. Subtree RPCs should distinguish omitted `limit` from explicit range overflow. `z_gettreestate` is eliminated as a large-response issue because tree `finalState` serialization is depth-bounded. The opt-in indexer gRPC server has no auth or tonic concurrency/stream limits if bound to a shared network, idle disconnected indexer streams can retain tasks until the next source event, `MempoolChange` exposes local mempool timing plus V5 authorization digests to each subscriber and treats broadcast lag as stream closure, and `TrustedChainSync` should recompute streamed block hashes plus use the normal recent-chain contextual gate before committing trusted-indexer blocks into a read-state mirror; its best-tip forwarding task should also be supervised and should not exit permanently on normal non-finalized best-tip hashes. The opt-in health endpoint should add an open-connection cap and request/header timeout if exposed beyond internal probes. |
| B3 Mempool DoS/policy parity | Public hardening plus private/future-gated heads-up candidates | GBT dependency metadata, GBT testnet sync-gate policy, GBT high-fee coinbase overflow handling, GBT long-poll full-mempool polling, GBT long-poll max-time refresh behavior, dependency policy, V5 WTXID exactness, V5 same-effects pending amplification, verbose mempool transitive descendant accounting, cascading removal notification/rejection, transaction `getdata` lookup amplification, direct pushed transaction source attribution, pending-output waiter caps/pruning, stale cancel-handle retention after downloader outer timeouts, and tip-local rejection-cache thrash remain follow-up areas. The private-leaning items are the stale cancel-handle timeout-retention candidate, pending practical remote-timeout validation, and the future-gated ZIP-235 miner-fee share intermediate overflow panic, because it is consensus-path code for a future upgrade rather than a current default-network issue. |
| B4 Time/locktime/MTP | Eliminated for consensus; public GBT time-envelope hardening | Block and mempool locktime checks use mined block time or best-chain next MTP as appropriate. `getblocktemplate` can still advertise mutable block-time bounds that disagree with local future-time, network max-time gating, or Testnet target-spacing activation behavior; see `docs/analysis/time-consensus-parity-note.md` and `docs/analysis/gbt-time-envelope-mismatch-note.md`. |
| B5 State migration/integrity | Eliminated for new private issue; public hardening remains | Disk format upgrades are ordered and mark completion after validation; roundtrip tests pass. Queued missing-parent block UTXOs can influence block/proposal semantic verification through `AwaitUtxo`, but state commit/proposal validation rebuilds spent UTXOs from the selected parent chain and finalized DB before mutation. Mempool is not affected because it uses `UnspentBestChainUtxo` and mempool `AwaitOutput`. |
| B6 Logging/metrics hygiene | Public hardening lead | Attacker-influenced metric labels and RPC tracing method attributes should be bounded; the optional metrics endpoint needs exposure/connection-timeout hardening if bound broadly; optional health endpoint counters should limit handled requests or disable keep-alive and rate-limit repeated WARN logs; optional tracing filter endpoint needs body-limit/auth hardening if exposed; Sentry/OpenTelemetry docs and redaction should make opt-in exported data explicit; RPC batch count should be capped or disabled to reduce method-label and dispatch amplification. |
| B7 Feature/default-release audit | Eliminated for current private issue; release-variable confirmation remains | Experimental features such as `tx_v6`, `comparison-interpreter`, `internal-miner`, `tokio-console`, `elasticsearch`, and `filter-reload` are not part of `default-release-binaries`; `sentry` and `opentelemetry` are compiled by default but require runtime export configuration; `debug_force_finished_sync` is compiled into normal RPC config but defaults false. Maintainers should still confirm hidden `RUST_PROD_FEATURES` / `RUST_TEST_FEATURES` repository variables do not combine `tx_v6` with `zcash_unstable = "nu7"` / `"zip235"` in runtime artifacts. If the experimental Elasticsearch feature is compiled, it should keep TLS certificate validation on by default and avoid panicking on endpoint request/response failures. |

## Verification Evidence

Additional targeted checks run during pass 5:

- `cargo test -p zebra-script sighash_divergence_v5 --lib`
- `cargo test -p zebra-script is_valid_rejects_mismatched_previous_outputs_length --lib`
- `cargo test -p zebra-script is_valid_rejects_out_of_range_input_index --lib`
- `cargo test -p zebra-consensus fallback_verification --lib`
- `cargo test -p zebra-consensus batch_flushes_on_max_items --lib`
- `cargo test -p zebra-consensus correctly_err_on_invalid_joinsplit_proof --lib`
- `cargo test -p zebrad mempool_reject_too_many_sigops --lib`
- `cargo test -p zebra-rpc includes_tx_with_selected_dependencies --lib`
- `cargo test -p zebra-state check_sapling_anchors --lib`
- `cargo test -p zebra-state service::check::tests::anchors --lib`
- `cargo test -p zebra-state service::check::tests::nullifier --lib`
- `cargo test -p zebra-state service::non_finalized_state::tests::prop::forked_equals_pushed --lib`
- `cargo test -p zebra-state all_upgrades_and_wrong_commitments_with_fake_activation_heights --lib`
- `cargo test -p zebra-chain checkpoint_list_hard_coded_mandatory --lib`
- Temporary proof test
  `proof_pre_heartwood_final_sapling_root_is_structural_only_today` passed with
  `cargo test -p zebra-state proof_pre_heartwood_final_sapling_root_is_structural_only_today --lib`;
  the probe source was removed after the run.
- `cargo test -p zebra-state reject_duplicate_sapling_nullifiers_in_chain --lib`
- `cargo test -p zebra-state reject_duplicate_orchard_nullifiers_in_chain --lib`
- `cargo test -p zebra-state forked_equals_pushed --lib`
- `cargo test -p zebra-chain value_balance --lib`
- `cargo test -p zebra-consensus miner_fees_validation --lib`
- `RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' cargo test -p zebra-consensus miner_fees_validation_succeeds_when_zip233_amount_is_correct --features tx_v6 --lib`
- Durable proof test
  `miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows_today`
  passed as `#[should_panic]` under
  `RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' cargo test -p zebra-consensus miner_fees_validation_panics_when_zip233_fee_share_intermediate_overflows_today --features tx_v6 --lib`.
- The surrounding unstable miner-fee filter also passed:
  `RUSTFLAGS='--cfg zcash_unstable="nu7" --cfg zcash_unstable="zip235"' cargo test -p zebra-consensus miner_fees_validation --features tx_v6 --lib`.
- `cargo test -p zebra-consensus funding_stream_validation --lib`
- `cargo test -p zebra-consensus v5_consensus_branch_ids --lib`
- `cargo test -p zebra-chain branch_id_consistent --lib`
- `cargo test -p zebrad mempool_cancel_downloads_after_network_upgrade --lib`
- `cargo test -p zebrad pending_outputs --lib`
- `cargo test -p zebra-state best_tip_is_latest_non_finalized_then_latest_finalized --lib`
- `cargo test -p zebra-state finalize_pops_from_best_chain --lib`
- `cargo test -p zebra-state finalized_equals_pushed --lib`
- `cargo test -p zebra-network poc_remote_addrv2_resource_exhaustion --lib`
- `cargo test -p zebra-network inv_registry_limit --lib`
- Durable current-behavior test
  `unsolicited_notfound_registers_missing_inventory_today` passed with
  `cargo test -p zebra-network unsolicited_notfound_registers_missing_inventory_today --lib`.
- Surrounding inventory-registry vector tests passed with
  `cargo test -p zebra-network peer_set::inventory_registry::tests::vectors --lib`.
- Local proof tests
  `unrelated_notfound_completes_active_block_request_today` and
  `unrelated_notfound_completes_active_transaction_request_today` passed with
  `cargo test -p zebra-network unrelated_notfound_completes_active --lib`.
- Local proof test
  `mempool_queue_reports_every_gossiped_id_before_download_cap` passed with
  `cargo test -p zebrad mempool_queue_reports_every_gossiped_id_before_download_cap --lib`.
- Local proof tests
  `block_message_with_trailing_bytes_is_accepted_today` and
  `tx_message_with_trailing_bytes_is_accepted_today` passed with
  `cargo test -p zebra-network trailing_bytes_is_accepted_today --lib`.
- Local proof tests
  `block_message_padded_past_max_block_bytes_is_accepted_today` and
  `tx_message_padded_past_max_block_bytes_is_accepted_today` passed with
  `cargo test -p zebra-network padded_past_max_block_bytes_is_accepted_today --lib`.
- Local proof test
  `headers_message_nonzero_transaction_count_is_accepted_today` passed with
  `cargo test -p zebra-network headers_message_nonzero_transaction_count_is_accepted_today --lib`.
- Local proof test
  `counted_header_nonzero_transaction_count_is_accepted_today` passed with
  `cargo test -p zebra-chain counted_header_nonzero_transaction_count_is_accepted_today --lib`.
- Local proof tests
  `getblocks_locator_longer_than_response_cap_is_accepted_today` and
  `getheaders_locator_longer_than_response_cap_is_accepted_today` passed with
  `cargo test -p zebra-network locator_longer_than_response_cap_is_accepted_today --lib`.
- Local proof test
  `find_blocks_scans_large_locator_before_response_cap_today` passed with
  `cargo test -p zebra-state find_blocks_scans_large_locator_before_response_cap_today --lib`.
- Local proof tests for codec-level `accepted_today` parser behavior passed with
  `cargo test -p zebra-network accepted_today --lib`.
- Local proof tests for non-empty bodyless messages passed with
  `cargo test -p zebra-network message_with_body_is_accepted_today --lib`.
- Local proof test
  `bip37_filter_messages_are_consumed_without_inbound_request_today` passed with
  `cargo test -p zebra-network bip37_filter_messages_are_consumed_without_inbound_request_today --lib`.
- `cargo test -p zebra-network connection_run_loop_receive_timeout --lib`
- Temporary proof test
  `zero_capacity_mpsc_clone_send_does_not_invalidate_other_sender_ready` passed
  with
  `cargo test -p zebra-network zero_capacity_mpsc_clone_send_does_not_invalidate_other_sender_ready --lib`;
  the probe source was removed after the run.
- `cargo test -p zebra-network inv_hash_max_allocation_is_correct --lib`
- `cargo test -p zebra-consensus mempool_request_with_invalid_lock_time_is_rejected --lib`
- `cargo test -p zebra-consensus transaction_is_rejected_based_on_lock_time --lib`
- `cargo test -p zebra-chain time_check_now --lib`
- `cargo test -p zebra-consensus time_is_valid_for_historical_blocks --lib`
- `cargo test -p zebrad sync_block_too_high_obtain_tips --lib`
- `cargo test -p zebrad obtain_tips_queues_fast_junk_hashes_before_later_honest_hashes_today --lib`
- `cargo test -p zebra-network missing_inv_collector_ignores_local_registry_errors --lib`
- `cargo test -p zebra-network inv_registry_prefer_missing_ok --lib`
- `cargo test -p zebrad inbound_block_empty_state_notfound --lib`
- `cargo test -p zebrad inbound_tx_empty_state_notfound --lib`
- `cargo test -p zebra-network peer_set_route_inv_all_missing_fail --lib`
- `cargo test -p zebra-network filteradd_message_too_large_is_truncated_and_accepted_today --lib`
- `cargo test -p zebra-network header_only_max_body_len_reserves_full_body_capacity_today --lib`
- `cargo test -p zebra-consensus time_is_valid_for_historical_blocks --lib`
- `cargo test -p zebra-chain max_block_times_correct_enforcement --lib`
- `cargo test -p zebra-state format_upgrades_are_in_version_order --lib`
- `cargo test -p zebra-state test_block_db_round_trip --lib`
- `cargo test -p zebra-state roundtrip_value_balance --lib`
- `cargo test -p zebra-network version_user_agent_size_limits --lib`
- `cargo test -p zebra-rpc oversized_request_body_is_rejected --lib`
- `cargo test -p zebra-rpc cookie_file_has_restrictive_permissions --lib`
- `cargo test -p zebra-rpc cookie_write_rejects_symlink --lib`
- Durable proof test
  `cookie_write_preserves_existing_regular_file_permissions_today` passed: an
  existing `0644` `.cookie` remained `0644` after `write_to_disk()` wrote a
  fresh cookie secret.
- Durable proof tests `rpc_server_start_failure_leaves_cookie_today` and
  `rpc_server_task_abort_leaves_cookie_today` passed: an auth-enabled bind
  failure and an aborted auth-enabled RPC server task both left `.cookie` on
  disk.
- Durable proof test
  `invalidating_chain_root_panics_when_removing_existing_chain_today` passed as
  `#[should_panic]` with
  `cargo test -p zebra-state invalidating_chain_root_panics_when_removing_existing_chain_today --lib`.
- Durable proof test `invalidating_same_height_fork_tips_panics_today` passed
  as `#[should_panic]` with
  `cargo test -p zebra-state invalidating_same_height_fork_tips_panics_today --lib`.
- Durable proof test `reconsider_block_twice_replays_stale_invalidated_entry_today`
  passed as `#[should_panic]` with
  `cargo test -p zebra-state reconsider_block_twice_replays_stale_invalidated_entry_today --lib`.
- Durable proof test
  `misbehavior_ban_panics_with_max_connections_per_ip_above_one_today` passed as
  `#[should_panic]` with
  `cargo test -p zebra-network misbehavior_ban_panics_with_max_connections_per_ip_above_one_today --lib`.
- `cargo check -p zebrad --features filter-reload --bin zebrad`
- `cargo test -p zebra-rpc rpc_submitblock_waits_without_timeout --lib`
- `cargo test -p zebra-rpc rpc_getblocktemplate_proposal_waits_without_timeout_when_block_verifier_hangs_today --lib`
- `cargo test -p zebra-rpc rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today --lib`
- `cargo test -p zebra-rpc non_ascii_long_poll_id --lib`
- `cargo test -p zebra-chain transaction_wtx_id_string_parse_roundtrip --lib`
- `cargo test -p zebrad transactions_exact_matches_v5_by_mined_id_not_wtxid_today --lib`
- `cargo test -p zebrad same_mined_id_v5_wtxids_queue_separately_today --lib`
- `cargo test -p zebrad timed_out_downloads_accumulate_cancel_handles_today --lib`
- `cargo check -p zebra-state --features elasticsearch`
- `cargo test -p zebra-rpc rpc_server_spawn --lib`
- `cargo test -p zebra-rpc rpc_getnetworksolps --lib`
- `cargo test -p zebrad --no-default-features --features default-release-binaries --bin zebrad config::tests::generate_with_no_args -- --exact`

Previously established local repro checks still stand:

- `cargo test -p zebra-script sighash_single --lib`
- `cargo test -p zebra-consensus v5_sighash_single --lib`
- `cargo test -p zebrad score_bearing_router_error_does_not_downcast_to_verify_block_error --lib`
- `cargo test -p zebra-consensus mempool_request_with_state_lookup_error_is_currently_missing_input --lib`
- `cargo test -p zebrad transparent_input_not_found_is_exact_tip_rejected_today --lib`
