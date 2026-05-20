# N4 F2 Full-Powerset Faulty-Init Tractability

**Date:** 2026-05-19
**Outcome:** Symbolic checking of the full N4 F2 faulty-init powerset at depth 1
is intractable on commodity hardware. Quick-check coverage is the standard for
the full-powerset surface; symbolic gates use the selected-evidence abstractions
(single / pair / triple faulty).

## Why this experiment

`baseline-completeness-audit.md` noted that a prior local Apalache probe of
`CrosslinkBaselineFullFaultyInitN4F2ForkingModel` at depth 1 exhausted the
default 4 GB JVM heap. The audit listed this as the largest open symbolic
tractability question for the baseline spec.

The A1(b) experiment re-ran the same probe with 16 GB heap to settle whether
the OOM was a 4 GB ceiling or a deeper intractability.

## What was run

```text
JVM_ARGS=-Xmx16384m \
  quint verify spec/quint/CrosslinkBaselineAccountability.qnt \
    --main=CrosslinkBaselineFullFaultyInitN4F2ForkingModel \
    --max-steps=1 \
    --init=InitWithFaultyEvidence \
    --step=Next \
    --invariant=BaselineFullN4F2ForkingFaultyInitSafety \
    --server-endpoint=localhost:8871
```

Toolchain: Apalache 0.51.1 (bundled with Quint 0.31.0), OpenJDK 25, 12+ CPU
cores available.

## Result

```text
State 0: Checking 25 state invariants
State 0: state invariant 0 holds.
... (invariants 1..11 hold) ...
State 0: state invariant 11 holds.
Ran out of heap memory (max JVM memory: 17179869184)
error: Ran out of heap memory: Java heap space
```

Wall clock to OOM: ~12 minutes. Peak resident memory observed: ~29 GB (JVM
allowed to spill past the 16 GB max via off-heap allocations).

The 4 GB baseline OOM'd much earlier — the audit doc records it failing before
clearing invariant 11. So 16 GB does deepen the symbolic frontier (got past 11
invariants), but not enough to clear State 0 even at depth 1.

## Decision

The full N4 F2 faulty-evidence powerset is treated as a **quick-check-only**
surface for the baseline spec. Symbolic gates remain the selected-evidence
abstractions:

- `BaselineSingleN4F2ForkingFaultyInitSafety` (one arbitrary faulty
  proposal / prevote / precommit from the full domain)
- `BaselinePairN4F2ForkingFaultyInitSafety` (two)
- `BaselineTripleN4F2ForkingFaultyInitSafety` (three)

plus the representative bounded harness and the same-shape progressions for
N5 F2 and N7 F2.

Quick-check coverage of the full powerset is retained through
`CrosslinkBaselineFullFaultyInitN4F2ForkingModel` and its peers in
`quick-baseline` and `quick`. The Rust-backed witness gate still exercises
the full domain.

## What this is not

This decision does NOT downgrade the symbolic safety guarantee for the f=2
surface. The selected-evidence harnesses each range over the full faulty-init
domain — they bound the *size* of the selected evidence set, not the *set
membership*. Single/pair/triple-faulty `n4_f2` therefore symbolically cover
every possible 1-, 2-, and 3-element selection from the full powerset.

This decision also does NOT settle whether a symmetry/quotient abstraction
over faulty values could collapse the powerset enough to make the full-domain
symbolic surface tractable. That remains an open option for future work, but
it is no longer a baseline-completeness blocker — the selected-evidence
abstractions already give symbolic coverage of every n-tuple selection at
n ≤ 3, and the audit's completion standard treats the bounded symbolic
agreement / validity / accountability checks as the required surface.

## Tracking-doc updates

- `baseline-completeness-audit.md` § "Remaining Work" item 3 and § "Coverage
  Matrix" "Nondeterministic faulty message injection in `Init`" row: replace
  the "not yet lifted into symbolic gates for the full-powerset larger
  instances" phrasing with "selected-evidence abstractions are the symbolic
  standard; full powerset is intractable at depth 1 with 16 GB JVM heap (see
  `n4f2-full-powerset-tractability.md`)."
- `baseline-upstream-crosswalk.md` § "Proof Gate Crosswalk" row "Faulty init
  symbolic checking": update the same way.
- § "Remaining Upstream-Quality Gaps" item 2 in the crosswalk: reframe as a
  forward-looking optionality (symmetry-quotient research) rather than a gap.

These edits are deferred until the A3 agent's tracking-doc updates land, to
avoid file-level merge conflicts.
