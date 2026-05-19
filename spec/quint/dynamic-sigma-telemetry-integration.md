# Dynamic Sigma Telemetry Integration

This note maps the dynamic-sigma Quint model inputs to production telemetry and
calls out the pieces that are not present in the current prototype.

The core rule is that the controller should select a sigma at least as large as
each independently required floor:

- a hash-participation floor
- a recent Tenderlink round-failure floor
- a block-interval and rollback-depth risk floor
- an economic rollback-risk and expected-loss floor

The Quint models intentionally keep these as bounded fixture inputs. A
production controller needs a shared, conservative source for each input before
it can change finality depth.

## Current Prototype Boundary

The live Rust proposal path still treats sigma as a fixed protocol parameter:

- `zebra-crosslink/src/chain.rs` defines
  `ZcashCrosslinkParameters::bc_confirmation_depth_sigma`.
- `BftBlock::try_from(params, ...)` checks that a fixed-sigma BFT block carries
  exactly that many PoW headers.
- `zebra-crosslink/src/lib.rs` proposes and validates the current
  `tip - bc_confirmation_depth_sigma` candidate.
- `zebra-crosslink/src/viz.rs` exposes chain, BFT, and finality state to the
  visualizer, but it is not a production telemetry source.

The branch now also includes `zebra-crosslink/src/dynamic_sigma.rs`, a pure
Rust controller that derives conservative coverage and round-failure estimates
from raw counters, validates a telemetry window, and selects the same sigma
floor as the Quint telemetry fixture. It also has a proposal-carried evidence
verifier that rejects a selected sigma outside the configured ladder or below
the controller-required floor.

`BftBlock::try_from_with_confirmation_depth` is available as the selected-sigma
block-construction hook, and `BftBlock::try_from_with_dynamic_sigma_evidence`
now composes the evidence verifier with block construction. It validates
proposal-carried evidence first, then checks the header count against the
selected sigma. The existing `BftBlock::try_from(params, ...)` path still uses
the fixed `bc_confirmation_depth_sigma`.

A production implementation must replace the fixed parameter at proposal and
validation time with this consensus-safe controller output, serialize or commit
the evidence in proposals, and populate the controller input from
consensus-visible or proposal-verifiable telemetry.

## Production Inputs

| Quint input | Production meaning | Current source | Missing production work |
| --- | --- | --- | --- |
| `TotalHashWork` | Total PoW work observed in the calibration window. | Block headers and chain work can be derived from validated PoW headers. | Define the exact window and whether competing side-branch work is included or only best-chain work. |
| `CrosslinkParticipatingHashWork` | PoW work from blocks whose miners are participating in Crosslink. | No complete source in the current prototype. | Add an objectively verifiable participation marker or derive participation from valid Crosslink-finality content in blocks. |
| `EstimatedCoverageRiskPct` | Conservative upper bound on the non-participating or unseen-work share. | Can be computed from total and participating work once both are defined. | Add safety margin for hidden work, delayed propagation, peer eclipse, and incomplete fork visibility. |
| `TotalTenderlinkRounds` | Count of Tenderlink rounds in the measurement window. | The prototype tracks BFT event flags and blocks. | Add durable round-start, timeout, nil-precommit, and decision counters. |
| `FailedTenderlinkRounds` | Rounds that do not decide a value and require recovery. | Not exposed as a production metric. | Define failure labels: timeout, nil-precommit certificate, stale proposal, invalid proposal, or mixed evidence. |
| `EstimatedRoundFailureRatePct` | Conservative upper bound on failed-round frequency. | Derived from round counters once available. | Decide smoothing, hysteresis, and window size so transient jitter does not create unstable sigma changes. |
| `MeasuredBlockIntervalVariancePct` | PoW timing instability over the same window. | Header times are available from validated blocks. | Define a robust estimator that handles timestamp manipulation and difficulty-adjustment lag. |
| `MeasuredObservedReorgDepth` | Maximum rollback depth observed across best-tip changes in the window. | Zebra state can observe best-tip transitions, but Crosslink-specific rollback-depth telemetry is not present. | Add a metric that records replaced prefix depth for best-tip changes and side-branch releases. |
| `RollbackRiskPpmAtSigma` | Modelled rollback probability for each candidate sigma. | Not a direct node metric. | Build an offline or deterministic estimator from participation, observed work competition, variance, and historical reorg data. |
| `ValueAtRiskUnits` | Economic value exposed to rollback if a finalized point is wrong or delayed. | Not available in the protocol implementation. | Define a policy input or service-facing exposure model. |
| `MaxAcceptableExpectedLossUnits` | Governance or operator budget for expected loss. | Not available in the protocol implementation. | Decide whether this is protocol policy, finalizer policy, or service-local policy. |

## Hash-Participation Rule

Hash participation is not a Tenderlink voting threshold. It measures whether
the PoW stream that finalizers are sampling is representative of the global
best-chain race.

The production controller should compute participation as a work-weighted ratio:

```text
participating_hash_work / total_hash_work
```

The numerator should only include objectively verifiable Crosslink-participating
work. Self-reported pool share is not enough. A future implementation could use
valid Crosslink finality-update content, a consensus-valid participation marker,
or another marker that full nodes can verify from block data.

The denominator must be conservative. If a node cannot see all competing work,
the estimator should bias toward lower participation, not higher participation.
This is important because hidden or delayed PoW work is exactly the risk that
requires a larger sigma.

The controller rule should match the Quint model shape:

- if participation is at or above the target threshold, hash participation does
  not raise sigma by itself
- if participation is below target but above critical, raise sigma to the
  degraded floor
- if participation is below the critical threshold, force max sigma and expose a
  critical status

## Consensus-Safety Requirement

Dynamic sigma cannot be an unconstrained local heuristic if it changes what
validators are willing to prevote or precommit.

Every validator that evaluates a proposal must be able to derive the same
required sigma for the same BFT height and PoW view, or the required telemetry
must be included in the BFT proposal and objectively validated. Otherwise two
honest validators could disagree on whether the same `head - sigma` value is
valid, creating a liveness failure that looks like a stream change.

This suggests two viable production shapes:

- Deterministic controller: all inputs are derived from consensus-visible chain
  data and fixed parameters, so validators recompute the same sigma.
- Proposal-carried controller evidence: the proposer includes measurement
  evidence and the BFT validity rules verify that the selected sigma is at
  least the required floor.

The Rust controller now prototypes the second shape for raw telemetry counters:
the verifier reconstructs the conservative telemetry window, rejects selected
sigma values below the required floor, and can be composed with `BftBlock`
construction so the selected sigma controls header depth. A production
deployment still needs precise validity rules for the source of each raw
measurement and live proposal plumbing for carrying or committing the evidence.
The proposal evidence structs now have deterministic Zcash serialization, and
`DynamicSigmaBftBlockPayload` defines a tagged payload envelope containing the
evidence followed by the BFT block. The remaining live-wiring step is adopting
that envelope in the Tenderlink proposal, validation, and decided-block
callbacks. The envelope also has a validation helper that replays evidence
validation and rejects carried blocks that do not match the evidence-selected
block.

The live Tenderlink callbacks now decode proposal bytes through an explicit
payload router. Legacy fixed-sigma `BftBlock` bytes are still accepted by the
current prototype path. Tagged dynamic-sigma payloads are recognized but
rejected until shared dynamic-sigma parameters and proposal-verifiable telemetry
are wired into the callback path. This preserves backward compatibility while
preventing a dynamic-sigma payload from being silently treated as a fixed-sigma
block.

## Failure Modes

The production controller needs guardrails for adversarial telemetry:

- Fake high participation is unsafe because it can keep sigma too low. This is
  why participation must be objectively derived from block data.
- Fake low participation is a liveness attack because it can force max sigma.
  The protocol should still prefer safety, but operators need observability and
  hysteresis to distinguish degraded participation from measurement failure.
- Short windows can oscillate sigma around the threshold. Use explicit windows,
  hysteresis, and bounded rate of sigma decrease.
- Local-only value-at-risk estimates can make validators disagree. If expected
  loss affects consensus validity, the exposure model must be shared or
  proposal-carried.
- Hidden hash power cannot be proven absent. The estimator should treat
  participation as an upper-confidence-bound problem and choose conservative
  sigma when coverage is uncertain.

## Implementation Acceptance Criteria

A production implementation of the dynamic-sigma variant should provide:

- a consensus-safe definition of Crosslink-participating PoW work
- a deterministic or proposal-verifiable computation of total observed work
- round-start, round-failure, nil-precommit, stale-proposal, and decision
  counters
- best-tip rollback-depth telemetry derived from actual fork transitions
- an explicit rollback-risk estimator for each allowed sigma
- an economic exposure model or a clear decision that expected loss is
  service-local rather than consensus-critical
- tests showing that lower hash participation never lowers sigma; the pure Rust
  controller now covers the bounded Quint telemetry fixture and raw-counter
  estimate construction, but production source integration still needs tests
- tests showing that dynamic sigma changes do not make honest validators reject
  each other's otherwise valid proposals; the pure Rust proposal-evidence
  verifier and BFT block-construction helper cover identical evidence
  determinism, evidence serialization, tagged payload encoding, payload/block
  mismatch rejection, fixed-vs-dynamic payload routing, below-floor rejection,
  and selected-sigma header depth, but live dynamic proposal acceptance still
  needs shared parameters and telemetry-source tests
- Quint coverage connecting the implemented telemetry rules back to
  `CrosslinkDynamicSigmaTelemetry.qnt`
