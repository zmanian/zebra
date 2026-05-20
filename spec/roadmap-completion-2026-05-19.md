# Canonical Roadmap Completion Status — 2026-05-19

This document records the state of the canonical zebra-crosslink roadmap as of
the work landed on `quint-crosslink-spec` ending at commit `3aac1bece`.

## Scope

The canonical roadmap is the nutshell Crosslink design + Quint baseline spec
+ dynamic-sigma controller. The additive Crosslink sketch
(`spec/zaki-additive-crosslink.md`) is explicitly out of scope.

## Roadmap items executed

| Item | What | Status | Commit |
| --- | --- | --- | --- |
| B1 | Name production participation marker, four-check verifier contract, contract test | DONE | `46ea80482` |
| A1(b) | n4_f2 full-powerset symbolic re-probe at 16 GB heap | DONE (verdict: still OOMs; selected-evidence is the symbolic standard) | `5ce7d2834` |
| CI | Gate `dynamic_sigma::tests::*` in CI | DONE | `9461d53f2` |
| A3 | Inductive multi-height finality lemmas (5 named consecutive-pair lemmas, symbolic verification at depth 5) | DONE | `f21493ea5` |
| B2 | Live `BestTipTransitionRecorder` hook | DONE (with one follow-on for non-monotonic reorgs) | `8ff1801c3` |
| B3 | Persisted hysteresis state source (`DurableLocal` / `ProposalCarried`) | DONE | `8ff1801c3` |
| B4 | Economic-exposure policy decision (`ServiceLocal` default, opt-in `ConsensusCritical`) | DONE | `8ff1801c3` |
| B1-leftover | Concrete production verifier in `lib.rs` wired through the prototype proposer's source-window assembly | DONE (with one follow-on for live wiring into proposal plan) | `8ff1801c3` |
| A2 | Full upstream transition port: first-class `BaselineNext`, `BaselineUponPrecommitQuorumEvidence` with upstream evidence-set parameter shape, 5 new tests | DONE | `e8260a582`, `184a98a01` |
| A4 | Three arbitrary-evidence Crosslink false-invariant counterexample harnesses (nil-precommit clears lock, amnesia, equivocation) | DONE | `3aac1bece` |
| A5 | CI maintenance | Continuous — `symbolic-baseline-core` split + new gates land under the 20-minute timeout |
| B5 | Proposal-evidence validity rules + adversarial-telemetry failure modes | Deferred — gated on B2-B4 follow-ons |

## Quint baseline gate status

- `quick-baseline`: 40 test modules pass, all green.
- `symbolic-baseline-core`: 15 invocations, all `Outcome: NoError`. Includes
  the new A3 inductive-finality lemmas at depth 5 and the A2 first-class
  `BaselineNext` transitions for `n4_f1` / `n4_f2` / `n5_f2` / `n7_f2`
  instances.
- `symbolic-baseline-accountability`: f=2 selected-evidence harnesses
  (single/pair/triple at `n4_f2` / `n5_f2` / `n7_f2`) all green. Full powerset
  remains quick-check-only per the A1(b) verdict.

## Rust test status

- `cargo test -p zebra-crosslink --lib dynamic_sigma::`: 97 pass.
- `cargo test -p zebra-crosslink --lib`: 143 pass.
- `cargo clippy -p zebra-crosslink --tests`: clean (71 pre-existing warnings,
  no new warnings from B1 / B2-B4).
- CI: `shieldedlabs-tests.yml` now gates both the original `crosslink_` filter
  and the dynamic_sigma module tests.

## Baseline completion-standard checklist (`baseline-completeness-audit.md`)

- [x] Focused baseline witnesses pass
- [x] Upstream-shaped parameterized baseline model typechecks
- [x] Every baseline model instance has quick witness coverage
- [x] Bounded symbolic checks pass for agreement, validity, accountability,
      fixed-sigma sampling, finalized-prefix safety
- [x] Counterexample harnesses produce expected failures (including the new
      A4 arbitrary-evidence ones)
- [x] CI runs the quick and symbolic baseline gates on the personal fork
- [ ] Issue and gist link to current spec folder and audit — out of scope for
      this engineering work; lives in project-management surface.

## Deliberate follow-ons (not blockers, but tracked)

Three deferred items from B2-B4 — flagged as deliberate non-blockers by the
B2-B4 agent's report:

1. **State-service common-ancestor lookup for reorgs.** Best-tip recorder
   hook drops genuine reorgs failure-closed today. Needs a state-service-
   backed common-ancestor lookup to feed rollback-depth telemetry on
   non-monotonic tip changes.

2. **Live-wire production verifier into proposal plan.**
   `prototype_dynamic_sigma_telemetry_components_with_production_verifier`
   exists but isn't called by `tenderlink_proposal_plan_from_hysteresis_state`
   yet. Needs async snapshot of roster + bft_blocks plumbed into the sync
   proposal-plan path.

3. **Consume best-tip transitions in proposals.**
   `dynamic_sigma_best_tip_transitions` window is recorded but not yet
   consumed for rollback-depth telemetry in actual proposal evidence.

4. **B5 — proposal-evidence validity rules.** Gated on follow-ons 1-3 so real
   telemetry flows through proposals before validity rules harden the
   consensus surface.

## Memory pointers

- `[[project-roadmap]]` — three-track project state.
- `[[quint-n4f2-tractability]]` — A1(b) verdict; do not re-run.

## Commits this engineering arc (oldest first)

```
46ea80482 Name production participation marker for dynamic sigma
5ce7d2834 Record N4 F2 full-powerset tractability verdict
9461d53f2 Run zebra-crosslink dynamic_sigma tests in CI
f21493ea5 Add inductive multi-height finality lemmas
8ff1801c3 Wire dynamic-sigma controller into live state and policy sources
e8260a582 Promote baseline shell to first-class transition system
184a98a01 Mark A2 transition port complete in tracking docs
3aac1bece Add arbitrary-evidence Crosslink counterexample harnesses
```

Plus a handful of background user commits interleaved (`22eed573f`,
`bf20748dc`) for pair/triple N7 F2 faulty-init harnesses.
