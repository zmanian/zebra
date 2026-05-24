# RepoPrompt Consensus And Serialization Slice

Date: 2026-05-09

Status: local audit note only. Do not post publicly without explicit user
direction.

Scope: a RepoPrompt-assisted follow-up over `zebra-chain` serialization,
transaction/block commitment helpers, and `zebra-consensus` transaction/block
checks. This pass was intentionally filtered against the existing local audit
ledger and open public issues.

## Summary

RepoPrompt returned four plausible candidates:

1. trailing bytes after top-level `block` / `tx` payloads;
2. large transparent-script byte allocation before truncated-payload EOF;
3. V5/V6 coinbase transactions with Orchard actions and `ENABLE_SPENDS`
   rejected only after Orchard bundle parsing;
4. panic-prone block helper preconditions if invalid parsed blocks are routed
   into Merkle / network-upgrade helpers in the wrong order.

After cross-checking, none should be promoted as a new private disclosure from
this slice:

- items 1 and 2 are already covered by public issues;
- item 3 is a bounded late-parse hardening idea, not a disclosure-grade remote
  DoS under the current verifier order;
- item 4 is a public-helper invariant footgun, but normal block and checkpoint
  verification reject missing coinbase heights before the panic-prone helper.

## Candidate Triage

| Candidate | Verdict | Reason |
| --- | --- | --- |
| Trailing bytes after `block` / `tx` payloads | Duplicate | Already covered by local note `p2p-block-header-parse-strictness-note.md` and public issue #10569, "Tighten P2P parser strictness for headers counts and trailing block or tx bytes". |
| Transparent script truncated-length allocation | Duplicate | Public issue #10554, "Add per-field size bounds to `Halo2Proof` and `transparent::Script` deserializers", covers `transparent::Script::zcash_deserialize` delegating to the global `Vec<u8>` allocation bound. |
| Orchard coinbase `ENABLE_SPENDS` late rejection | Eliminated as disclosure-grade | Distinct as a future parser-hardening idea, but the remaining attacker gain is bounded malformed transaction/block parse work. The transaction verifier rejects before state, script, or proof verification. |
| Missing-height / empty-block helper panic | Eliminated as production-reachable | The helpers still assume validated input, but the normal full-block and checkpoint paths preflight coinbase height before calling `merkle_root_validity()`. |

## Orchard Coinbase Recheck

RepoPrompt's most interesting non-obvious candidate was the Orchard analogue of
the older Sapling coinbase-spend allocation shape: a peer could send a coinbase
transaction with `nActionsOrchard > 0` and `ENABLE_SPENDS`, which is invalid for
coinbase transactions.

The parser does not know the transaction is coinbase at the Orchard bundle
boundary:

- `zebra-chain/src/transaction/serialize.rs:420` implements
  `ZcashDeserialize` for `Option<orchard::ShieldedData>`.
- `zebra-chain/src/transaction/serialize.rs:423` parses the Orchard actions.
- `zebra-chain/src/transaction/serialize.rs:428` returns early only when the
  action list is empty.
- `zebra-chain/src/transaction/serialize.rs:443` parses `flagsOrchard`.
- `zebra-chain/src/transaction/serialize.rs:455` parses `Halo2Proof`.
- `zebra-chain/src/transaction/serialize.rs:462` parses spend-auth signatures
  using the action count.

So there is still some late work before Zebra reaches the coinbase-specific
semantic rejection. However, that work stays inside the ordinary bounded
transaction/block deserialization budget:

- `zebra-chain/src/orchard/shielded_data.rs:190-206` bounds Orchard action
  preallocation using `MAX_BLOCK_BYTES` and `AUTHORIZED_ACTION_SIZE`.
- `zebra-chain/src/orchard/shielded_data.rs:237` defines `ENABLE_SPENDS`.
- `zebra-chain/src/orchard/shielded_data.rs:278` rejects reserved Orchard flag
  bits with `Flags::from_bits(...)`.
- public issue #10554 already covers the materially interesting per-field bound
  gap on `Halo2Proof`.

The transaction verifier then rejects this shape in quick checks:

- `zebra-consensus/src/transaction.rs:404-407` runs context-free checks before
  state loading or cached FFI transaction construction.
- `zebra-consensus/src/transaction.rs:415` calls
  `coinbase_tx_no_prevout_joinsplit_spend()` for coinbase transactions.
- `zebra-consensus/src/transaction.rs:483` creates `CachedFfiTransaction`
  later, after those quick checks.
- `zebra-consensus/src/transaction/check.rs:173-184` returns
  `TransactionError::CoinbaseHasEnableSpendsOrchard` if a coinbase transaction
  has Orchard `ENABLE_SPENDS`.

Final verdict: keep as eliminated / future hardening only. A future hardening
patch could pass `is_coinbase` into the Orchard parser and reject
`ENABLE_SPENDS` after reading flags but before reading the proof and signatures,
but this is not worth a new private report by itself.

## Block Helper Panic Recheck

RepoPrompt also highlighted that `Block::check_transaction_network_upgrade_consistency()`
assumes a valid coinbase height:

- `zebra-chain/src/block.rs:129` calls
  `self.coinbase_height().expect("a valid height")`.
- `zebra-consensus/src/block/check.rs:426-438` calls that helper from
  `merkle_root_validity()`.

That helper can still panic if a test or privileged caller routes a missing-height
block into it directly. The production remote block paths checked in this pass
reject first:

- `zebra-consensus/src/block.rs:208-210` returns `BlockError::MissingHeight`
  before calling `merkle_root_validity()`.
- `zebra-consensus/src/checkpoint.rs:596-599` returns
  `VerifyCheckpointError::CoinbaseHeight` before checkpoint Merkle checks.
- `zebra-consensus/src/block/check.rs:39-48` also has a typed
  `BlockError::NoTransactions` / coinbase-position check when `coinbase_is_first()`
  is reached.

Final verdict: not a fresh remote panic. Keep this under internal API
hardening / invariant documentation unless a production caller is found that
invokes `merkle_root_validity()` before the coinbase-height preflight.

## V6 Branch-ID Side Check

During the local cross-check, I also revisited a future V6 branch-ID concern.
Under the experimental `tx_v6` path, V6 deserialization accepts any
`NetworkUpgrade >= Nu5` as the encoded branch ID:

- `zebra-chain/src/transaction/serialize.rs:1065-1071`.

On its own that looks too loose for "V6 means NU7", and
`verify_v6_transaction()` currently delegates to the V5 verifier:

- `zebra-consensus/src/transaction.rs:979-988`.

But current verification appears fail-closed:

- `zebra-consensus/src/transaction/check.rs:543-556` rejects any V5+ transaction
  whose encoded branch ID does not match `NetworkUpgrade::current(network,
  height)`.
- `zebra-consensus/src/transaction.rs:483` maps failed librustzcash conversion
  into `TransactionError::UnsupportedByNetworkUpgrade`.
- the existing V6 conversion / auth-digest panic family is already covered by
  public issue #10534 and the local #10534 follow-up note.

Final verdict: future-activation hardening only. If librustzcash starts accepting
more V6 shapes, re-check that V6 is accepted only at the intended activation
network upgrade.

## Verification

Commands used during this pass:

```sh
rp-cli -w 1 -e 'builder "Deep Zebra security audit slice: consensus and serialization boundary search for fresh, non-overlapping vulnerabilities..." --response-type plan'
rp-cli -w 1 -e 'chat "Re-evaluate candidate 3 only: Orchard coinbase transactions with nActionsOrchard > 0 and ENABLE_SPENDS..." --mode plan'
gh api repos/ZcashFoundation/zebra/issues/10554 --jq '{title: .title, state: .state, url: .html_url}'
gh api repos/ZcashFoundation/zebra/issues/10569 --jq '{title: .title, state: .state, url: .html_url}'
gh api repos/ZcashFoundation/zebra/issues/10534 --jq '{title: .title, state: .state, url: .html_url}'
rg -n "MAX_U8_ALLOCATION|Script::zcash_deserialize|transparent script|Vec<u8>" docs/analysis zebra-chain/src/transparent zebra-chain/src/serialization -S
rg -n "Orchard coinbase|coinbase Orchard|ENABLE_SPENDS|nActionsOrchard|coinbase_tx_no_prevout_joinsplit_spend" docs/analysis zebra-chain/src zebra-consensus/src -S
rg -n "merkle_root_validity|coinbase_is_first|a valid height|missing coinbase|coinbase height" docs/analysis zebra-consensus/src/block zebra-consensus/src/checkpoint.rs zebra-chain/src/block.rs zebra-chain/src/block/merkle.rs -S
```
