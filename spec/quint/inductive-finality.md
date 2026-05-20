# Inductive Multi-Height Finality for the Baseline Crosslink

This note records the inductive structure behind the baseline Crosslink's
multi-height finality argument, as implemented in
`CrosslinkBaselineInductiveFinality.qnt`. It is the A3 deliverable that
"pushes finality beyond a bounded fixture" called for in
`baseline-completeness-audit.md` item 8 and in the
"Inductive multi-height finality proof" row of `baseline-upstream-crosswalk.md`.

## Would-Be Inductive Invariant

The baseline BFT-height model finalizes a sequence of PoW snapshots
`(d_0, d_1, d_2, ...)` at consecutive BFT consensus heights
`(h_0, h_1, h_2, ...)`. The multi-height finalized-prefix property we want is:

> For every pair of consecutive finalized BFT heights `(h, h+1)`:
>
> 1. The height-`(h+1)` decision extends the height-`h` finalized prefix.
> 2. No Tenderlink agreement at height `h+1` finalizes a value outside that
>    prefix.
> 3. The sticky baseline at `h+1` cannot finalize a fork that competes with
>    height `h`.
> 4. The successor height is exactly `h + 1` (no skipped consensus heights).
> 5. Every previously recorded decision at height `<= h+1` is an ancestor of
>    the current `latestFinal`.

If every consecutive-pair clause holds at every step of the transition system,
the finalized prefix is linear and monotone by simple induction on the number
of decisions taken.

## Named Lemmas

`CrosslinkBaselineInductiveFinality.qnt` reshapes the monolithic
`BaselineBftHeightSafety` into five named lemmas. Each is a standalone `val`
expressing one clause of the inductive invariant and is individually checkable
as a bounded invariant:

- `BaselineHeightSuccessorExtendsPrefix` — `Extends(latestFinal, priorFinal)`.
  Clause (1).
- `BaselineNoForkFinalityAtSuccessorHeight` —
  `finalized.forall(v => Agrees(latestFinal, v))`. Clause (2).
- `BaselineStickyBaselineDoesNotFinalizeForkAtSuccessor` — the new
  `latestFinal` either equals `priorFinal` or strictly extends it. Clause (3).
- `BaselineSuccessorHeightIsConsecutive` — `consensusHeight` is `priorHeight`
  or `priorHeight + 1`. Clause (4).
- `BaselineRecordedDecisionsAreAncestors` — every entry of `decisionAt` is a
  snapshot and is extended by `latestFinal`. Clause (5).

The compound invariant `BaselineInductiveFinalitySafety` is the conjunction of
those clauses plus the base model's `Safety`. The model is wired into both the
quick (`run_model`) and symbolic (`verify_model`) baseline gates in
`check.sh`, with the symbolic check at depth 5.

## Positive and Negative Witnesses

`CrosslinkBaselineInductiveFinality.qnt` also includes:

- `inductiveConsecutiveHeightsExtendPrefixTest` — a positive witness that
  drives two consecutive scheduled decisions on the stable stream and asserts
  every clause at every intermediate state.
- `falseHeightSuccessorExtendsPrefixWitnessTest`,
  `falseNoForkFinalityAtSuccessorHeightWitnessTest`,
  `falseStickyBaselineDoesNotFinalizeForkAtSuccessorWitnessTest`, and
  `falseSuccessorHeightIsConsecutiveWitnessTest` — Rust-backed `.fail()`
  witnesses that inject a fork-finality attempt or a skipped height and then
  assert the lemma still holds. The test fails as expected, demonstrating
  that the lemma rejects the bad shape. This is the same pattern used by
  `falseForkFinalityAttemptIsValidTest` in
  `CrosslinkBaselineBftHeights.qnt`.

## Not a Machine-Checked Induction

This decomposition does NOT close an inductive proof. Apalache cannot prove
the induction natively; it only checks the lemma conjunction at the model's
depth bound. What the decomposition gives us is:

1. A reviewable statement of the inductive invariant as a set of small,
   named lemmas, each tied to a specific consecutive-pair preservation
   obligation.
2. Individual bounded gates for each lemma, so a regression that breaks one
   clause is reported under the lemma's own name rather than as a failure
   somewhere inside a monolithic safety invariant.
3. A starting point for a future fully inductive proof in a tool that
   supports it (for example Coq, Lean, or TLAPS over a TLA+ translation).
   The lemma decomposition is the same shape that proof would use.

## Tractability

At the current parameter shell (`MaxConsensusHeight = 3`, `MaxPowHeight = 4`,
`Sigma = 1`, eight-snapshot fork tree), Apalache verifies
`BaselineInductiveFinalitySafety` at `--max-steps=5` in roughly 7 seconds
locally on the patched Node 26 CLI. The same lemmas should remain tractable
under modest parameter growth; if a future deeper instance OOMs at depth 5,
the natural fallback is to keep the quick-only gate and document the
tractability limit, matching the pattern used by the full-powerset
faulty-init harnesses in `CrosslinkBaselineAccountability.qnt`.
