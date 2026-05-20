# Dynamic Sigma Production Participation Marker

**Status:** Decided
**Author:** Zaki Manian
**Date:** 2026-05-19

## Decision

The production participation marker for the dynamic-sigma controller is the
**Crosslink fat pointer in each PoW header** (`Header.fat_pointer_to_bft_block`,
type `zebra_chain::block::header::FatPointerToBftBlock`).

A header is classified as **Crosslink-participating** in the dynamic-sigma
source window if and only if a *production marker verifier* (defined below)
returns `true` for that header's fat pointer.

This is a **source-selection policy**, not a consensus rule. PoW block validity
is unchanged. The marker is only used to compute the participating-hash-work
numerator that feeds the dynamic-sigma controller, which in turn selects a
proposal-carried confirmation depth.

## Why this marker

The PoW header is the only place where every full node sees, for every block,
an objectively verifiable per-block participation signal. Alternatives
considered:

- **Coinbase commitment to a recent BFT cert.** Would require a coinbase rule
  change. Rejected: dynamic sigma is meant to ship without consensus changes,
  and a participation signal that ships through the coinbase is one
  consensus-fork away from being a participation *requirement*.
- **Off-chain attestation gossip.** No objective per-header signal, and the
  source window cannot tell who is participating from the header stream.
  Rejected: the dynamic-sigma input is a percentage of *hash work*, which
  requires a header-anchored signal.
- **Derive from valid Crosslink finality content embedded later in the chain.**
  Lossy and lag-prone. The fat pointer field already exists in the header, so
  this is strictly weaker than the chosen approach.

The fat pointer is already present in `Header` today (zero-cost change) and is
already validated at BFT-block-decision time (see
`new_decided_bft_block_from_malachite` in `zebra-crosslink/src/lib.rs`). The
production marker verifier reuses that same validation.

## The production marker verifier contract

A production marker verifier is a function

```rust
fn(&FatPointerToBftBlock) -> bool
```

passed into the `_with_verifier` helpers in
`zebra-crosslink/src/dynamic_sigma.rs`. It MUST satisfy these properties:

1. **Null marker is non-participating.** If
   `*fat_pointer == FatPointerToBftBlock::null()`, return `false`. This is
   already guarded by `default_header_participation_marker_verifier` before the
   custom verifier is consulted.

2. **Invalid signatures are non-participating.** The verifier MUST inflate the
   fat pointer to its richer form (`FatPointerToBftBlock2`) and call
   `validate_signatures()`. A failing batch ed25519 verification returns
   `false`.

3. **Sub-quorum signature sets are non-participating.** The verifier MUST call
   `fat_pointer_has_roster_quorum(&fp2, &roster)` against the roster active at
   the height where the source header was mined. A signature set that does not
   satisfy `3 * signed_voting_power > 2 * total_voting_power` returns `false`.
   The roster lookup MUST be by *source-header height*, not by the
   controller's current height — historical headers in the source window MAY
   reference older rosters.

4. **Unknown referenced BFT blocks are non-participating** (recommended).
   `fat_pointer.points_at_block_hash()` SHOULD be checked against the local
   node's known finalized BFT blocks. Failing this check returns `false`. This
   prevents an attacker from minting fat pointers that satisfy quorum against
   an active roster but reference a BFT block the network has not seen.

5. **Failure-closed.** Any error during lookup (roster not available for the
   height, BFT block index unavailable, etc.) MUST return `false`. The
   controller MUST NOT silently classify uncheckable headers as participating;
   that would let a degraded node raise its own dynamic-sigma input and lower
   confirmation depth.

The four checks compose as conjunction: a header is participating only if all
four succeed.

## What the verifier MUST NOT do

- It MUST NOT mutate any node state. The verifier is pure with respect to the
  fat pointer; any cached lookups are read-only.
- It MUST NOT perform network I/O. Source-window assembly is on the proposal
  path; remote calls would block proposal emission. All lookups MUST be against
  in-memory state populated by normal chain processing.
- It MUST NOT depend on the controller's current proposal round, sigma value,
  or hysteresis state. The verifier is shaped to be deterministic given a fat
  pointer plus the historical roster at the source-header height.

## Bootstrap and roster transitions

Before genesis-plus-one BFT decision, no headers carry a non-null fat pointer
and the controller selects the maximum sigma floor by construction (no
participating work). This is the correct behavior: the BFT layer is not yet
operational, so conservative sigma is appropriate.

When the roster changes at a height `H`, source headers mined at heights
`<= H` are verified against the pre-`H` roster; headers mined at heights
`> H` are verified against the post-`H` roster. The verifier is responsible
for the lookup. If the roster history is unavailable for any source-header
height (e.g., pruned state), the verifier returns `false` for that header
(failure-closed).

## Consensus-safety boundary

This decision intentionally does NOT make the participation marker a consensus
rule. Concretely:

- Header validity does not require a valid fat pointer. Null markers and
  malformed markers are both valid headers under existing PoW rules.
- Dynamic sigma's *selected confirmation depth* is consensus-relevant only via
  the proposal-carried evidence path (see
  `dynamic-sigma-telemetry-integration.md` § "Failure Modes"). A validator
  rejecting a proposal because its evidence-selected sigma is below the floor
  is a validity check on the *proposal*, not on the *headers*.
- A node MAY change its production verifier implementation (e.g., add a
  stricter referenced-BFT-block check) without forking, as long as it remains
  conservative — stricter verifiers reduce the participating numerator, which
  raises sigma, which is safe.

## Reference implementation surface

`zebra-crosslink/src/dynamic_sigma.rs` already exposes the verifier hook
through:

- `hash_work_observation_from_header_with_verifier`
- `hash_work_observations_from_headers_with_verifier`
- `telemetry_components_from_header_observation_window_with_verifier`
- `telemetry_components_from_timed_header_observation_window_with_verifier`
- `telemetry_components_from_header_observation_window_with_hash_work_policy_and_verifier`

The default verifier `default_header_participation_marker_verifier` is the
prototype bridge — non-null implies participation. **Production deployments
MUST pass a verifier that performs all four checks listed above.** Calling the
non-`_with_verifier` helpers in a production build SHOULD be treated as a
configuration error.

## Verifier-contract test

`dynamic_sigma::tests::production_marker_verifier_contract_chains_checks`
exercises the verifier contract by composing four mock per-check predicates
(non-null, valid-signatures, has-quorum, known-bft-block) and asserting:

- All four pass → header is `VerifiedParticipating`.
- Any one fails → header is `NotVerifiedParticipating`.
- The verifier is not consulted for a `FatPointerToBftBlock::null()` marker
  (the upstream guard short-circuits to `NotVerifiedParticipating`).

This test pins the conjunction shape so future refactors of the verifier hook
cannot accidentally drop one of the four checks.

## Acceptance criteria checklist

The participation-marker work is complete when:

- [x] This decision document is committed.
- [x] The verifier-contract test is in the dynamic_sigma test suite.
- [ ] A concrete production verifier (composing
      `FatPointerToBftBlock2::validate_signatures`,
      `fat_pointer_has_roster_quorum`, and the known-BFT-block lookup) lives in
      `zebra-crosslink/src/lib.rs` and is passed to the prototype proposer's
      source-window assembly.
- [ ] The dynamic-sigma telemetry integration document is updated to reference
      this decision instead of describing the marker question as open.

The third and fourth boxes are part of step B2-B4 (the Rust integration PR).
