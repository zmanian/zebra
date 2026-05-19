# Dynamic Sigma Telemetry Integration

This note maps the dynamic-sigma Quint model inputs to production telemetry and
calls out the pieces that are not present in the current prototype.

The core rule is that the controller should select a sigma at least as large as
each independently required floor. One of those floors is the percentage of PoW
hash power that is observably participating in Crosslink:

- a hash-participation floor
- a recent Tenderlink round-failure floor
- a block-interval and rollback-depth risk floor
- an economic rollback-risk and expected-loss floor

The Quint models intentionally keep these as bounded fixture inputs. A
production controller needs a shared, conservative source for each input before
it can change finality depth.

## Current Prototype Boundary

By default, the live Rust proposal path still treats sigma as a fixed protocol
parameter:

- `zebra-crosslink/src/chain.rs` defines
  `ZcashCrosslinkParameters::bc_confirmation_depth_sigma`.
- `BftBlock::try_from(params, ...)` checks that a fixed-sigma BFT block carries
  exactly that many PoW headers.
- `zebra-crosslink/src/lib.rs` proposes and validates the current
  `tip - bc_confirmation_depth_sigma` candidate.
- `zebra-crosslink/src/viz.rs` exposes chain, BFT, and finality state to the
  visualizer, but it is not a production telemetry source.

When `dynamic_sigma_prototype` is explicitly enabled, the proposer uses the
prototype dynamic-sigma controller output and emits the tagged dynamic-sigma
envelope with prototype evidence. This is only a live wire-path exercise. The
evidence source is a fixed fixture, not production telemetry, and must be
replaced before the dynamic variant can be enabled by default.

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
| `TotalHashWork` | Total PoW work observed in the calibration window. | Block headers and chain work can be derived from validated PoW headers; the Rust telemetry assembly boundary now requires explicit total-work evidence before raw telemetry can be built. | Define the exact window and whether competing side-branch work is included or only best-chain work. |
| `CrosslinkParticipatingHashWork` | PoW work from blocks whose miners are participating in Crosslink. | No complete source in the current prototype; the Rust telemetry assembly boundary rejects missing participating-work evidence instead of assuming healthy participation. | Add an objectively verifiable participation marker or derive participation from valid Crosslink-finality content in blocks. |
| `EstimatedCoverageRiskPct` | Conservative upper bound on the non-participating or unseen-work share. | Can be computed from total and participating work once both are defined. | Add safety margin for hidden work, delayed propagation, peer eclipse, and incomplete fork visibility. |
| `TotalTenderlinkRounds` | Count of Tenderlink rounds in the measurement window. | `DynamicSigmaRoundEvent` can accumulate started rounds into `DynamicSigmaRoundCounters`, but live Tenderlink event hooks are not wired yet. | Wire durable round-start events from Tenderlink into the counter window. |
| `FailedTenderlinkRounds` | Rounds that do not decide a value and require recovery. | `DynamicSigmaRoundEvent` can accumulate nil-precommit, stale-proposal, timeout, invalid-proposal, and mixed-evidence failure labels, and validation rejects reason counters that outnumber failed rounds. | Wire those labels to live Tenderlink recovery and timeout paths. |
| `EstimatedRoundFailureRatePct` | Conservative upper bound on failed-round frequency. | Derived from assembled round counters, with conservative margins applied by the raw telemetry conversion. | Decide smoothing, hysteresis, and window size so transient jitter does not create unstable sigma changes. |
| `MeasuredBlockIntervalVariancePct` | PoW timing instability over the same window. | Header times are available from validated blocks. | Define a robust estimator that handles timestamp manipulation and difficulty-adjustment lag. |
| `MeasuredObservedReorgDepth` | Maximum rollback depth observed across best-tip changes in the window. | `DynamicSigmaBestTipTransition` can derive rollback depth from old-tip, new-tip, and common-ancestor heights, but live state hooks are not wired yet. | Add a metric that records replaced prefix depth for best-tip changes and side-branch releases. |
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

The Rust prototype now has a pure source-side aggregation boundary for this
metric. `observed_hash_work_participation` takes explicit PoW work observations
tagged as either verified-participating or not verified-participating, sums all
observed work into the denominator, and only sums verified-participating work
into the numerator. Empty observation windows are rejected. This still does not
define the production marker; it prevents the next source producer from treating
unknown or unverified work as healthy participation.

`hash_work_observation_from_header` is the first concrete source adapter for
that boundary. It converts a validated PoW header's compact difficulty into
work, classifies non-null Crosslink fat pointers as the current objective
participation marker, and counts null-marker headers as observed but
non-participating work. Invalid header difficulty fails closed instead of being
counted as zero or healthy participation. This adapter is intentionally a
prototype marker bridge: production still needs to decide whether the marker
must validate the fat pointer contents, signatures, quorum, or referenced BFT
block before assigning verified-participating status.

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

The live Tenderlink callbacks now route proposal bytes through an explicit
payload encoder/decoder. Legacy fixed-sigma `BftBlock` bytes are still emitted
and accepted by the default prototype path. Tagged dynamic-sigma payloads are
rejected by default, but a prototype-only config flag enables the proposer to
emit the tagged envelope and enables validation callbacks to check the envelope
against shared prototype dynamic-sigma parameters before accepting the carried
BFT block. The proposer now derives `selected_sigma` by running the
dynamic-sigma controller over prototype telemetry components instead of
hard-coding the base sigma or bypassing assembly with raw counters. The decoded
payload also carries its selected confirmation depth into the voting-time
current-stream staleness check, so prototype dynamic proposals are compared
against `head - selected_sigma` instead of the fixed-sigma sample. This
preserves backward compatibility while preventing a dynamic-sigma payload from
being silently treated as a fixed-sigma block, and it keeps the dynamic variant
behind an explicit opt-in until production telemetry exists. If telemetry
assembly or telemetry-to-evidence selection fails, the proposal path now fails
closed instead of falling back to the base sigma.

In that prototype-gated path, hash participation already affects payload
validity through the carried evidence: if the Crosslink-participating work share
is below the configured target, the selected sigma must be at least the degraded
floor; if it is below the critical threshold, the selected sigma must be the max
floor.

The branch also now has a pure production-shaped telemetry assembly contract.
`DynamicSigmaTelemetryComponents::try_into_raw_telemetry` requires explicit total
hash work and explicit Crosslink-participating hash work, rejects inconsistent
round counters, and only then builds `DynamicSigmaRawTelemetry`. This does not
solve the source-of-truth problem by itself; it makes the next source-integration
step fail closed instead of letting unknown participation or contradictory round
metrics look like a healthy calibration window.

Hash-work participation has the same source-shaping pattern:
`DynamicSigmaHashWorkObservation` records the observed work and whether that work
has objective Crosslink participation evidence, while
`observed_hash_work_participation` derives the component pair used by
`DynamicSigmaTelemetryComponents`. The tests now cover healthy, degraded, and
critical participation shares through this path, so lower verified
participation raises or preserves the selected sigma floor instead of lowering
it.

Header-derived observations now feed that same path:
`hash_work_observations_from_headers` derives per-header work from
`difficulty_threshold.to_work()`, uses the current non-null Crosslink fat pointer
as the participation marker, and keeps all valid header work in the denominator.
That makes the "percentage of hash power participating in Crosslink" input
work-weighted rather than block-count-weighted.

The source contracts are now composed by
`telemetry_components_from_observation_window`. It accepts hash-work
observations, already accumulated Tenderlink round counters, best-tip
transitions, block-variance telemetry, rollback-risk estimates, and economic
exposure inputs. It derives the participation numerator/denominator, validates
round-counter consistency, derives maximum observed rollback depth, and produces
`DynamicSigmaTelemetryComponents`. This still leaves the live production marker,
state source, and economic/risk estimators open, but it gives those producers a
single pure assembly target.

`CrosslinkDynamicSigmaTelemetry.qnt` now mirrors that source boundary in the
production-shaped telemetry harness: source hash-work samples derive the
total-work denominator and participating numerator, source round counters are
checked for consistency, and source best-tip transition heights derive observed
rollback depth before the controller checks the sigma floor.

Round telemetry now has a matching event contract.
`DynamicSigmaRoundEvent` records started rounds, decisions, nil-precommit
recovery, stale proposals, timeouts, invalid proposals, and mixed evidence into
`DynamicSigmaRoundCounters`. Assembly still accepts direct counters for future
deterministic or proposal-carried evidence, but the event API gives live
Tenderlink hooks a single place to accumulate the durable window. Validation
rejects impossible totals and failure-reason overcounts.

The prototype proposal path now uses this same assembly boundary through
`dynamic_sigma_proposal_evidence_from_telemetry_components`: telemetry components
are assembled into raw telemetry, the controller selects the required sigma, and
the proposal carries the selected value in its evidence. This keeps prototype
fixtures aligned with the production-shaped contract while leaving the actual
source producers as explicit remaining work.

The fixture itself now enters through the same source-observation shape:
prototype hash-work observations are aggregated into total and verified
participating work, an empty best-tip transition window derives rollback depth
0, round events derive decided-round counters, and
`telemetry_components_from_observation_window` builds the components consumed by
proposal evidence selection. Live producers still need to replace those fixture
observations.

Rollback-depth telemetry has the same shape. `DynamicSigmaBestTipTransition`
represents a best-tip change by its previous tip height, new tip height, and
common ancestor height. The helper derives rollback depth as the replaced
previous-best-chain suffix and rejects impossible ancestor evidence. This gives
the controller a precise production-facing input contract for observed reorg
depth, but it still needs live state integration that can supply the actual
common ancestor for best-tip transitions and side-branch releases.

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

The Rust controller now has a pure hysteresis helper for the short-window
oscillation case. `apply_dynamic_sigma_hysteresis` raises immediately when a new
window requires a larger sigma, but only lowers one ladder level after a
configured number of stable lower-risk windows. The prototype evidence builder
can now apply that helper from an explicit hysteresis state before carrying
`selected_sigma` in proposal evidence. Proposal validation remains floor-based:
validators reject selected sigma below the telemetry-required floor, while a
hysteresis-selected sigma above that floor remains valid. The prototype service
now stores in-process hysteresis state and advances it after a dynamic proposal
payload is successfully encoded. Production still needs a durable,
consensus-safe or proposal-verifiable source for the hysteresis state before
this becomes a deployed controller rule. The prototype proposal callback uses
one proposal plan for both candidate-depth selection and payload encoding, which
is the shape needed before that state is promoted beyond the prototype.

`CrosslinkDynamicSigmaHysteresis.qnt` mirrors that policy with bounded witnesses:
participation-driven or reorg-driven sigma increases apply immediately, while
recovery to a lower sigma requires stable lower-risk windows and descends one
ladder step at a time.

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
- a durable hysteresis state source if the dynamic variant should smooth sigma
  decreases across windows rather than selecting the raw required floor each
  time
- tests showing that lower hash participation never lowers sigma; the pure Rust
  controller now covers the bounded Quint telemetry fixture and raw-counter
  estimate construction, and the prototype-gated Tenderlink payload decoder now
  rejects dynamic payload evidence whose Crosslink-participating hash-power
  share requires a higher sigma than the proposer selected. The hash-work
  observation tests now derive the participation numerator from explicit
  verified-participating observations and cover healthy, degraded, and critical
  shares through telemetry assembly. The observation-window tests now compose
  hash-work observations, round counters, and best-tip transitions into
  telemetry components and reject invalid source counters or rollback evidence.
  The new pure telemetry assembly tests also reject missing participating-work
  evidence and inconsistent round counters, the event-counter tests reject
  failure-reason overcounts, and rollback-depth tests derive the observed
  reorg-depth input from explicit best-tip transition evidence, but live
  production source integration still needs tests
- tests showing that dynamic sigma changes do not make honest validators reject
  each other's otherwise valid proposals; the pure Rust proposal-evidence
  verifier and BFT block-construction helper cover identical evidence
  determinism, evidence serialization, tagged payload encoding, payload/block
  mismatch rejection, fixed-vs-dynamic payload routing, below-floor rejection,
  selected-sigma header depth, prototype-gated proposal emission, and
  selected-sigma voting-time stale checks. The live proposer now runs the
  controller over its prototype telemetry fixture before selecting sigma, but
  production telemetry-source tests are still needed
- Quint coverage connecting the implemented telemetry rules back to
  `CrosslinkDynamicSigmaTelemetry.qnt`
