# GetBlockTemplate Time-Envelope Mismatch Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Finding

`getblocktemplate` derives a miner-visible block-time envelope that can disagree
with the rules Zebra later applies to the submitted block or proposal.

There are three related subcases:

- `maxtime` is derived from median-time-past plus 90 minutes, but is not
  intersected with the node-local "not more than two hours in the future" rule
  that proposal and `submitblock` validation later enforce.
- `maxtime` is computed as median-time-past plus 90 minutes without checking
  `Network::is_max_block_time_enforced(candidate_height)`, so custom/pre-rule
  test networks can receive a tighter template bound than contextual validation
  would require.
- on Testnet or custom test networks, the minimum-difficulty time-range
  adjustment uses the previous block height when choosing the standard-vs-minimum
  difficulty split; at a target-spacing activation boundary, that can make
  `mintime` / `maxtime` inaccurate for the returned `bits`.

This is not a consensus acceptance issue. Full block validation computes the
candidate block's difficulty and time rules from the candidate height. The risk
is mining RPC correctness: a miner that mutates the block time inside the
advertised range can produce a proposal that Zebra later rejects as invalid for
the returned `bits`.

## Evidence

The block-template chain-info path derives its time envelope in state:

- `zebra-state/src/service/read/difficulty.rs:202-260` receives the current tip
  height, computes `min_time`, `max_time`, `cur_time`, and
  `expected_difficulty`, then adjusts template times for Testnet
  minimum-difficulty behavior.
- `zebra-state/src/service/read/difficulty.rs:214` uses local wall-clock time
  only to choose `cur_time`.
- `zebra-state/src/service/read/difficulty.rs:227-239` sets `min_time` to
  median-time-past plus one second and `max_time` to median-time-past plus 90
  minutes.
- `zebra-state/src/service/read/difficulty.rs:231-237` documents the
  network/height-gated MTP+90-minute rule, but the template calculation applies
  it unconditionally.

Full block and proposal validation use additional checks:

- `zebra-consensus/src/block.rs:242-245` checks the block header time against
  `Utc::now()` during semantic verification, including proposals and
  `submitblock`.
- `zebra-chain/src/block/header.rs:107-126` accepts only headers with
  `time <= now + 2 hours`.
- `zebra-state/src/service/check.rs:267-321` performs contextual validation,
  rejecting `candidate_time <= median_time_past` and rejecting
  `candidate_time > median_time_past + 90 minutes` only when
  `network.is_max_block_time_enforced(candidate_height)` is true.
- `zebra-chain/src/parameters/network.rs:230-235` gates the MTP+90-minute rule
  by network and height.

The candidate difficulty calculation itself uses the next block height:

- `zebra-state/src/service/check/difficulty.rs:130-164` derives
  `candidate_height = previous_block_height + 1`.
- `zebra-state/src/service/check/difficulty.rs:188-203` uses
  `candidate_height` when deciding whether the candidate is a Testnet
  minimum-difficulty block.
- `zebra-chain/src/parameters/network_upgrade.rs:473-483` says the Testnet
  minimum-difficulty gap depends on `block_height`, and calls
  `minimum_difficulty_spacing_for_height(network, block_height)`.

But the Testnet template time-range adjustment uses the previous block height
when choosing the standard/minimum-difficulty split:

- `zebra-state/src/service/read/difficulty.rs:268-272` names the argument
  `previous_block_height`.
- `zebra-state/src/service/read/difficulty.rs:305-307` calls
  `NetworkUpgrade::minimum_difficulty_spacing_for_height(network,
  previous_block_height)`.
- `zebra-state/src/service/read/difficulty.rs:316-323` derives
  `std_difficulty_max_time` and `min_difficulty_min_time` from that spacing.
- `zebra-state/src/service/read/difficulty.rs:338-365` then adjusts `min_time`,
  `max_time`, `cur_time`, and sometimes `expected_difficulty` for the template.

That normally matches the candidate height. It can diverge at Blossom activation,
where target spacing changes from 150 seconds to 75 seconds:

- `zebra-chain/src/parameters/network_upgrade.rs:391-407` returns 150-second
  spacing before Blossom and 75-second spacing from Blossom onward.
- `zebra-chain/src/parameters/network_upgrade.rs:439-453` multiplies the current
  height's target spacing by the Testnet minimum-difficulty gap multiplier.
- Default Testnet activates Blossom at height 584,000
  (`zebra-chain/src/parameters/constants.rs:43-44`), and custom test networks can
  configure activation heights through their parameter builder
  (`zebra-chain/src/parameters/network/testnet.rs:607-623`).

The template exposes the affected values to miners:

- `zebra-rpc/src/methods/types/get_block_template.rs:147-158` serializes
  `target` and `mintime`.
- `zebra-rpc/src/methods/types/get_block_template.rs:175-207` serializes
  `curtime`, `bits`, and `maxtime`.
- `zebra-rpc/src/methods/types/get_block_template/constants.rs:25-33` includes
  `"time"` in the template's mutable fields.
- `zebra-rpc/src/methods/types/get_block_template/proposal.rs:109-123` uses
  `mintime`, `maxtime`, or clamped times when building proposals from a template.
- `zebra-rpc/src/methods/types/get_block_template/proposal.rs:224-235` reuses the
  template's `difficulty_threshold` with the selected block time.

## Concrete Boundary Shape

At the Testnet Blossom activation block:

- the real candidate-height minimum-difficulty gap is `75 * 6 = 450` seconds;
- the previous-height gap used by the template adjustment is `150 * 6 = 900`
  seconds.

If the returned `bits` are standard difficulty, a `maxtime` based on the
previous-height 900-second gap can include times that are actually
minimum-difficulty times for the Blossom candidate height.

If the returned `bits` are minimum difficulty, a `mintime` that remains at the
ordinary MTP+1 value can include times that are still standard-difficulty times
for the Blossom candidate height.

In both cases, the block verifier should reject the malformed proposal. The
issue is that the mining RPC response can describe those times as valid for the
template's fixed `bits`.

## Local Boundary Proofs

Added focused state-unit proofs on 2026-05-09:

```sh
cargo test -p zebra-state gbt_maxtime --lib
cargo test -p zebra-state testnet_gbt_time_adjustment_uses_previous_upgrade_boundary_today --lib
cargo test -p zebra-state service::read::difficulty::tests --lib
```

Result: passed.

`gbt_maxtime_can_exceed_local_future_time_limit_today` builds a Mainnet
template context where the previous block time is 90 minutes ahead of local
time. Current code returns `maxtime = median_time_past + 90 minutes`, which is
later than `now + 2 hours`; a synthetic block using that advertised `maxtime`
is rejected by `Header::time_is_valid_at()` with the same local clock. This
proves the node-local future-time subcase.

`gbt_maxtime_is_set_when_max_time_rule_is_inactive_today` builds a custom
Testnet at candidate height 299,000, below
`TESTNET_MAX_TIME_START_HEIGHT`. It confirms
`network.is_max_block_time_enforced(candidate_height)` is false, but the
template still advertises `maxtime = median_time_past + 90 minutes`. This proves
the height-gated max-time subcase.

`testnet_gbt_time_adjustment_uses_previous_upgrade_boundary_today` builds a
custom Testnet with Blossom at height 299,189 and Canopy at the following
height, so the candidate block is at Blossom while the previous block is still
pre-Blossom. It then calls `adjust_difficulty_and_time_for_testnet()` with a
five-minute candidate time. Current code keeps `maxtime` at the pre-Blossom
900-second standard-difficulty boundary, while
`NetworkUpgrade::is_testnet_min_difficulty_block()` says the Blossom candidate
height starts minimum-difficulty blocks at 451 seconds.

Together, these tests prove all three concrete subcases in this note. The
common shape is still mining-RPC correctness rather than invalid block
acceptance: the miner-visible time envelope can include values that later
validation does not accept for the same template.

The node-local future-time mismatch has a simpler shape: if the best-chain
timestamps are sufficiently ahead of local time, `median_time_past + 90 minutes`
can be later than `now + 2 hours`. Zebra can return that later value as
`maxtime`, but the same node's semantic verifier rejects a submitted block with
that timestamp until local time catches up.

## Impact

Expected impact is public mining RPC correctness / availability hardening:

- only mining RPC users are affected;
- the node-local future-time subcase can affect any network when recent chain
  timestamps are far enough ahead of the node's local clock;
- the Blossom activation subcase is mostly custom/regtest or replay-style
  testing because default Testnet's Blossom activation is historical;
- an attacker does not get invalid block acceptance, inflation, or consensus
  divergence;
- miners using Zebra as their template source can waste work or see proposal
  rejection if they mutate time according to the advertised range.

## Suggested Fix

Use the same time-bound model for template generation and validation:

- expose a reusable `now + 2 hours` helper from the header-time check, or compute
  it through the same path used by `Header::time_is_valid_at()`;
- expose contextual MTP lower/upper-bound helpers from `AdjustedDifficulty`, with
  the upper bound returning `None` when `Network::is_max_block_time_enforced()` is
  false;
- make the miner-visible `maxtime` the intersection of the chain-derived bound
  and the node-local future-time ceiling;
- keep a separate internal template-expiry deadline for long polling, so
  wall-clock movement does not churn long-poll IDs unnecessarily;
- compute `candidate_height = previous_block_height + 1` in
  `adjust_difficulty_and_time_for_testnet()`;
- call `NetworkUpgrade::minimum_difficulty_spacing_for_height(network,
  candidate_height)`;
- keep `AdjustedDifficulty::new_from_header_time(..., previous_block_height,
  ...)` unchanged, because that API already derives the candidate height
  internally.

Suggested regression coverage:

- Build a synthetic custom Testnet where the best tip is immediately before a
  Blossom activation height at or after the Testnet minimum-difficulty start
  height.
- Stub recent block times so the next block has a clear standard-difficulty and
  minimum-difficulty split.
- Assert that every time source returned by
  `BlockTemplateTimeSource::valid_sources()` creates a proposal whose
  `difficulty_threshold` and `time` agree with contextual validation.
- Add a narrower state-level unit test that checks the advertised `mintime` and
  `maxtime` use the candidate-height spacing at activation.
- Add an RPC/state-level test where chain times make `median_time_past + 90
  minutes` exceed `now + 2 hours`, and assert the returned `maxtime` is not a
  timestamp the same node would immediately reject.
- Add a custom testnet/regtest test below the MTP+90-minute enforcement height,
  if GBT is expected to support that configuration, and assert the template
  bounds match contextual validation.

## Duplicate Check

Refreshed on 2026-05-09 with read-only GitHub searches for:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate maxtime mintime median time past 90 minutes'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra getblocktemplate testnet Blossom minimum difficulty time spacing'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "getblocktemplate" "maxtime" "mintime"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "minimum difficulty" "mintime"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "median time past" "90 minutes" "getblocktemplate"'
```

Relevant adjacent hits:

- closed #5871 reported an older Testnet `getblocktemplate` `mintime` /
  `maxtime` mismatch against `zcashd`;
- closed #5925 fixed that older Testnet min/max-time issue by using the tip
  block for `min_time` and keeping adjusted times within other consensus-rule
  ranges;
- closed #5659 originally populated several GBT block-header fields from state.

Those are useful provenance, but they do not cleanly cover the current
source-level gaps: intersecting the advertised `maxtime` with the node-local
future-time limit, respecting the height-gated MTP+90-minute rule for custom
test networks, or using the candidate height for the Testnet minimum-difficulty
spacing at activation boundaries.

## Triage

Public hardening.

This is adjacent to the eliminated B4 time-consensus lead, but it is not a
private consensus finding. The consensus verifier remains the backstop; the bug
is in the mining template's advertised mutable-time envelope.

Confidence: medium-high for the source-level mismatches; medium on practical
impact because the main effect is wasted mining/proposal work.
