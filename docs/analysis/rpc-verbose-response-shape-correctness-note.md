# RPC Verbose Response Shape Correctness Note

Date: 2026-05-03

Last updated: 2026-05-09

## Finding

Several verbose RPC response builders expose public correctness issues where the
JSON shape or accounting can imply more precision than Zebra actually selected:

- `getrawtransaction(..., verbose=1)` emits `in_active_chain: false` for mempool
  hits even though the field is documented as present only with an explicit
  `blockhash` argument.
- `TransactionObject::from_transaction()` always emits an `orchard` object, even
  for transactions without Orchard shielded data.
- Orchard action serialization recovers `spendAuthSig` by searching for an equal
  `Action`, instead of consuming the already-paired `AuthorizedAction` in
  protocol order.
- `getrawmempool(true)` reports descendant counts, sizes, and fees from direct
  dependents only, while the field names and zcashd-compatible semantics imply
  transitive descendants.

These are not consensus vulnerabilities. They are RPC compatibility and
downstream correctness hardening items for operators or clients that use Zebra
RPC output as an exact block, transaction, or mempool accounting interface.

## Evidence

The `TransactionObject` field comment says `in_active_chain` is only present
with an explicit `blockhash` argument:

- `zebra-rpc/src/methods/types/transaction.rs:153-159`

The mempool fast path for `getrawtransaction` has no block context but still
passes `Some(false)`:

- `zebra-rpc/src/methods.rs:1718-1731`

The verbose transaction builder always emits Sapling value-balance fields and an
`orchard` object:

- `zebra-rpc/src/methods/types/transaction.rs:862-904`

The Orchard object is present even when `tx.orchard_shielded_data()` is `None`.
Several inner Orchard fields are then omitted through `Option`, but the top-level
`orchard` object still appears with zero value balance and an empty action list:

- `zebra-chain/src/transaction.rs:1071-1089`
- `zebra-chain/src/transaction.rs:1125-1128`

For transactions that do have Orchard data, the builder first iterates
`tx.orchard_actions()`, then searches `shielded_data.actions` by `Action`
equality to recover `spend_auth_sig`:

- `zebra-rpc/src/methods/types/transaction.rs:864-902`

The protocol data already stores each `Action` with its matching
`spend_auth_sig`, so RPC can preserve exact positional pairing by iterating
`AuthorizedAction` directly:

- `zebra-chain/src/orchard/shielded_data.rs:23-40`
- `zebra-chain/src/orchard/shielded_data.rs:185-204`

Verbose mempool response construction reads one entry from the dependents map and
uses that direct set for descendant count, size, and fees:

- `zebra-rpc/src/methods/types/get_raw_mempool.rs:70-105`

`TransactionDependencies` explicitly describes and exposes direct dependencies
and direct dependents:

- `zebra-node-services/src/mempool/transaction_dependencies.rs:16-22`
- `zebra-node-services/src/mempool/transaction_dependencies.rs:121-128`

The same type has separate traversal logic for removing all direct or indirect
dependents, which confirms that the stored map itself is direct-edge metadata:

- `zebra-node-services/src/mempool/transaction_dependencies.rs:82-104`

## Local Proof

Added
`zebra-rpc/src/methods/types/get_raw_mempool.rs::mempool_object_counts_only_direct_dependents_today`.
The test builds a synthetic parent -> child -> grandchild dependency chain from
three valid mempool test-vector transactions, constructs the verbose mempool
object for the parent, and confirms the reported descendant count, size, and fee
include only the direct child plus the parent.

Focused command:

```sh
cargo test -p zebra-rpc mempool_object_counts_only_direct_dependents_today --lib
```

Result on 2026-05-09: passed.

## Impact

The likely impact is misleading RPC output, not node compromise:

- a mempool transaction can be reported with `in_active_chain: false`, which
  looks like a chain-membership answer despite there being no queried block;
- non-Orchard transactions can carry a present `orchard` object, which can
  confuse clients that distinguish absent shielded data from empty shielded
  data;
- if duplicate Orchard action bodies were ever consensus-valid, equality-based
  lookup could attach the first equal action's signature rather than the
  positionally corresponding signature;
- chained mempool transactions can underreport descendant metadata because
  grandchildren and deeper descendants are excluded.

This matters most for indexers, explorers, exchanges, or mining infrastructure
that use verbose Zebra RPC as a compatibility layer with zcashd. It does not
change consensus verification, mempool admission, state commitment, or block
template selection.

## Suggested Fix

- In the mempool `getrawtransaction` fast path, pass `in_active_chain: None`
  when no `blockhash` was supplied.
- Emit `orchard: None` when `tx.orchard_shielded_data().is_none()`.
- Build Orchard verbose actions by iterating
  `tx.orchard_shielded_data().actions` directly, using each
  `AuthorizedAction`'s paired `spend_auth_sig`.
- For `getrawmempool(true)`, build a per-response analysis object that computes
  transitive descendant closures once and reuses them for each verbose
  `MempoolObject`.
- Validate Sapling `valueBalance` / `valueBalanceZat` field-presence behavior
  against zcashd before changing it; Zebra may intentionally mimic zcashd by
  emitting zero balances for non-Sapling transactions.

## Disclosure Posture

Treat as public hardening.

The paths require RPC access and produce misleading JSON, but the reviewed
shapes do not let an attacker crash Zebra, bypass validation, mint funds, or
change mined templates. They are good compatibility fixes, especially for
downstream infrastructure, but they should not consume private disclosure
bandwidth unless paired with a concrete downstream exploit.

## Confidence

Confidence: medium-high on the code shapes; low-to-medium on security severity.

The source-to-sink paths are direct. The main uncertainty is compatibility:
before fixing field presence, compare against zcashd behavior so Zebra does not
break callers that already rely on zcashd-compatible zero-valued shielded fields.
