# RPC gettxout snapshot consistency note

Date: 2026-05-03

Last updated: 2026-05-07

Status: already publicly tracked as
https://github.com/ZcashFoundation/zebra/issues/10550 before the local-only
posting stop instruction. Do not post further public comments without explicit
re-authorization.

## Summary

`gettxout` builds one response from multiple independent mempool/read-state
queries. On the state path, it reads the best block hash, then reads the
transaction, then asks whether the outpoint is spent. Those state requests can
observe different best-chain snapshots while Zebra is syncing, finalizing, or
handling a reorg.

This can make `gettxout` return a `bestblock` that does not correspond to the
transaction/output/spent-status snapshot used for the rest of the response. Zebra
already has an inline TODO for the `bestblock` part of this issue. This is not a
consensus bug, but it is RPC correctness hardening for clients that treat
`gettxout` as an atomic UTXO proof against a specific best block.

## Evidence

- `zebra-rpc/src/methods.rs:3064-3110` first checks the optional mempool path
  and may return mempool-created or mempool-spent status immediately.
- `zebra-rpc/src/methods.rs:3113-3114` has a TODO saying Zebra should ensure
  the returned tip hash is valid for the response.
- `zebra-rpc/src/methods.rs:3116-3127` reads `ReadRequest::Tip` and stores the
  `best_block_hash`.
- `zebra-rpc/src/methods.rs:3129-3135` separately reads
  `ReadRequest::Transaction(txid)`.
- `zebra-rpc/src/methods.rs:3147-3162` separately reads
  `ReadRequest::IsTransparentOutputSpent(outpoint)`.
- `zebra-rpc/src/methods.rs:3168-3177` builds the `OutputObject` from the
  transaction read while using the earlier `best_block_hash`.
- `zebra-rpc/src/methods/types/transaction.rs:356-366` shows that the response
  object includes both `bestblock` and `confirmations`.
- `zebra-state/src/service.rs:1418-1424` handles `ReadRequest::Transaction`
  using the current `state.latest_best_chain()` snapshot for that individual
  request.
- `zebra-state/src/service.rs:1718-1722` handles
  `ReadRequest::IsTransparentOutputSpent` using the current
  `state.latest_best_chain()` snapshot for that individual request.
- `zebra-state/src/service/read/block.rs:141-158` computes transaction
  confirmations from the transaction's containing height and the tip height seen
  by that transaction read.
- `zebra-state/src/service/read/block.rs:43-66` explicitly allows tip reads to
  return either non-finalized or finalized tips when the two overlap during
  delayed/heavy updates.

## Impact

During active sync or reorg/finalization races, an RPC client can observe a
`gettxout` response where:

- `bestblock` was read before the transaction/output became visible;
- `confirmations` was computed from a later transaction-read snapshot;
- spent status was checked against yet another snapshot.

The most obvious bad result is a response that says an output exists at
`bestblock = H`, while the transaction/output is only present in a later best-tip
snapshot. The opposite race can also produce a conservative `null` result if the
spent check observes a newer chain where the outpoint is absent or spent.

This is mostly a correctness and client-safety issue. It does not let an
untrusted peer create invalid chain state, and it does not by itself spend or
create coins. But clients that use `gettxout` to make low-confirmation deposit,
swap, or proof decisions can be confused if they assume the response is atomic
with respect to the returned `bestblock`.

## Existing mitigations

- JSON-RPC is disabled by default.
- Cookie authentication is enabled by default when RPC is enabled.
- The inconsistency is race-dependent and most visible while the best chain is
  changing.
- The response remains derived from locally verified state/mempool data, not
  arbitrary remote input.

## Suggested fix direction

- Add a single read-state request for `gettxout` that returns the tip hash,
  transaction/output, spent status, and confirmations from one cloned
  non-finalized chain plus one finalized DB view.
- Alternatively, read the tip after the transaction and spent-status checks and
  retry if the tip changed between the first and last state request.
- Include tests with a mock read-state service that changes tip/transaction/spent
  answers between calls and assert `gettxout` retries or returns a consistent
  error.
- Document that the mempool path returns an unconfirmed local-mempool view when
  `include_mempool` is true.

## Duplicate Check

Read-only duplicate searches performed on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'gettxout bestblock snapshot consistency in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'gettxout returned tip valid response in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'gettxout IsTransparentOutputSpent Transaction Tip in:title,body' --state all --limit 100
```

Closest hit:

- #10550, open, directly covers `gettxout`'s separate `Tip`, `Transaction`, and
  `IsTransparentOutputSpent` reads as part of the already-public multi-query RPC
  snapshot-consistency issue.

## Disclosure triage

Public hardening. The issue is an RPC snapshot-consistency gap in a disabled by
default, authenticated-by-default endpoint. It is worth fixing for clients that
consume Zebra RPC as a zcashd-compatible source of UTXO state, but it is not a
private disclosure candidate on current evidence.

Confidence: medium-high on the non-atomic request sequence and resulting
possible inconsistency; medium-low on practical impact, because exploiting it
requires timing around active chain movement or reorg/finalization updates.
