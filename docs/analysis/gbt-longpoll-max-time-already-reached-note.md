# GetBlockTemplate Long-Poll Max-Time Already Reached Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Finding

`getblocktemplate` long polling can fail to return when the caller's
`longpollid` still matches the current tip/mempool state and the template time is
already clamped to `maxtime`.

This is not a consensus acceptance issue. It is a mining RPC
availability/correctness hardening issue: a long-poll request that should return
so the miner can refresh work can instead remain pending until a mempool or tip
change occurs.

## Preconditions

- Mining RPC is enabled and `miner_address` is configured.
- The caller can invoke `getblocktemplate`.
- The caller supplies a `longpollid` matching the current server long-poll ID.
- The chain tip has not changed and the mempool checksum/count has not changed.
- The block-template `cur_time` is already clamped to `max_time`.

This can happen on mainnet after an extended period without a new block, or on
testnet/custom networks around minimum-difficulty and maximum-time boundaries.
It can also happen if a client obtains or reuses a template whose ID contains the
same `max_time`, then starts a long-poll request after the local time has already
reached that boundary.

## Evidence

State computes block-template time by taking local time and clamping it into the
valid range:

- `zebra-state/src/service/read/difficulty.rs:214`
- `zebra-state/src/service/read/difficulty.rs:227-239`

So `cur_time == max_time` is an intended state output when the local clock is at
or past the maximum allowed block time.

The long-poll ID includes `tip_height`, a tip-hash checksum, `max_time`, mempool
transaction count, and a mempool transaction checksum:

- `zebra-rpc/src/methods/types/long_poll.rs:68-115`

It does not encode `cur_time`, nor whether `max_time` has already been reached.
`submit_old()` likewise compares only tip height, tip hash checksum, and max
timestamp:

- `zebra-rpc/src/methods/types/long_poll.rs:198-212`

The `getblocktemplate` loop returns when the server ID differs from the client
ID or when a previous loop iteration has set `max_time_reached`:

- `zebra-rpc/src/methods.rs:2328-2345`
- `zebra-rpc/src/methods.rs:2350-2359`

But the only path that sets `max_time_reached = true` is the selected
`wait_for_max_time` future:

- `zebra-rpc/src/methods.rs:2477-2490`

When `cur_time` has already been clamped to `max_time`, Zebra deliberately does
not install that future:

- `zebra-rpc/src/methods.rs:2388-2397`

The comment says the zero-duration case should "wait for another change, and
ignore this timeout." That means if the client's old ID still equals the current
ID, the loop does not return at the max-time boundary. It waits only for the
five-second mempool polling interval or a best-tip notification:

- `zebra-rpc/src/methods/types/get_block_template/constants.rs:10-20`
- `zebra-rpc/src/methods.rs:2363-2379`

On each mempool polling wake, the same state can be fetched again, with the same
`cur_time == max_time`, same `max_time`, same tip, and same mempool checksum. The
server ID remains equal to the client ID, `max_time_reached` remains false, and
the zero-duration max-time future is again omitted.

## Existing Tests

The snapshot long-poll test uses an all-zero `longpollid`, so the server ID
differs and the RPC returns immediately with `submitold: false`:

- `zebra-rpc/src/methods/tests/snapshot.rs:1250-1295`

I did not find a test where:

- the supplied `longpollid` equals the freshly generated server ID,
- `cur_time == max_time`, and
- no tip or mempool change occurs.

The existing no-timeout tests cover proposal-mode and `submitblock` verifier
waits, not the template-mode long-poll loop:

- `zebra-rpc/src/methods/tests/vectors.rs:2433-2490`
- `zebra-rpc/src/methods/tests/vectors.rs:2492-2587`

The RPC server builder also does not configure an application-level method
timeout around long-running RPC futures:

- `zebra-rpc/src/server.rs:138-153`

## Impact

Expected impact is low-to-medium, depending on deployment:

- legitimate mining clients can wait longer than intended for refreshed work
  when max time is already reached;
- callers with mining RPC access can keep long-poll RPC futures open across the
  max-time boundary until some unrelated chain or mempool change occurs;
- this composes with the broader RPC long-running-request hardening notes, but
  it requires mining RPC access and is not reachable on default non-mining nodes.

This should be treated as public hardening on current evidence, not private
disclosure. It does not let an attacker create invalid consensus state, mint
funds, or crash the process by itself.

## Suggested Fix

Treat `cur_time >= max_time` as "max time reached" for long-poll requests instead
of suppressing the max-time future:

- If the caller supplied a matching `longpollid` and `cur_time >= max_time`,
  return a fresh template immediately with `submitold: false`.
- Alternatively, allow the zero-duration max-time future to fire immediately and
  set `max_time_reached = true`, then loop once and return through the existing
  branch.

Add a unit test using mocked state and mempool services:

1. Generate the expected current `LongPollId` from the fake tip hash, height,
   `max_time`, and empty mempool.
2. Configure `GetBlockTemplateChainInfo` with `cur_time == max_time`.
3. Call `get_block_template()` with that matching `longpollid`.
4. Assert the RPC returns promptly and sets `submitold` to `Some(false)`.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "longpoll" "max_time" "already reached"'
gh api repos/ZcashFoundation/zebra/issues/9301
gh api repos/ZcashFoundation/zebra/issues/9727
```

No exact issue hits were returned.

Closest broad overlap: #9301 ("DoS vulnerability in `getblocktemplate` RPC")
and #9727 ("Respond quickly to long-polled `getblocktemplate` RPC on new chain
tips") are open and cover general GBT DoS / long-poll responsiveness themes,
but not this exact zero-duration max-time behavior.

Current-behavior proof added and rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc getblocktemplate_matching_longpollid_waits_when_maxtime_already_reached_today --lib
cargo test -p zebra-rpc getblocktemplate --lib
```

Result: passed.

The new test lives in `zebra-rpc/src/methods/tests/vectors.rs`. It constructs a
matching current `LongPollId` from the mocked tip, empty mempool, and `max_time`,
then returns `GetBlockTemplateChainInfo` with `cur_time == max_time`. Current
code does not return within a 200 ms timeout, confirming that the
zero-duration max-time path stays pending instead of immediately returning a
fresh template with `submitold=false`.

## Confidence

Confidence: medium-high.

The control-flow evidence is direct. Remaining uncertainty is practical impact:
the edge requires mining RPC access and a rare mainnet timing condition, and the
long-poll design intentionally holds requests under normal conditions. The
current zero-duration branch nevertheless contradicts the documented
`submitold=false` behavior when max time is reached and is cheap to harden.
