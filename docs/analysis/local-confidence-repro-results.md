# Local Confidence Repro Results

Date: 2026-05-02

Scope: local-only repro tests added to this checkout to decide which audit findings are worth
maintainer time. These tests document current behavior; they are not prepared as upstream PR tests.

Disclosure status: the initial private email to ZF has been sent for the V5
`SIGHASH_SINGLE` corresponding-output finding. Keep exact exploit details and
repro artifacts private until maintainers confirm disclosure handling.

## Confirmed Locally

### 1. V5 `SIGHASH_SINGLE` Missing Corresponding Output

Local test:

- `zebra-script/src/tests.rs::sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today`
- `zebra-script/src/tests.rs::sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today`
- `zebra-consensus/src/transaction/tests.rs::v5_sighash_single_missing_corresponding_output_is_accepted_by_transaction_verifier_today`
- `zebra-consensus/src/transaction/tests.rs::v5_sighash_single_anyonecanpay_missing_corresponding_output_is_accepted_by_transaction_verifier_today`

Behavior reproduced:

- Builds a V5 transaction with two transparent inputs and one transparent output.
- Signs and verifies input index `1` with canonical `SIGHASH_SINGLE` (`0x03`) and
  `SIGHASH_SINGLE|ANYONECANPAY` (`0x83`).
- `all_previous_outputs.len() == transaction.inputs().len()`, so the PR #10510 alignment fix is satisfied.
- `input_index >= transaction.outputs().len()`, so there is no corresponding transparent output.
- Zebra's script verifier accepts the spend.
- Zebra's full transaction verifier also accepts a block transaction with this shape when supplied
  with known UTXOs, a valid `SIGHASH_ALL` signature for input `0`, the missing-output
  `SIGHASH_SINGLE` or `SIGHASH_SINGLE|ANYONECANPAY` signature for input `1`, and a positive
  miner fee.
- Live zcashd source inspection shows zcashd's ZIP 244 path explicitly rejects both
  `SIGHASH_SINGLE` and `SIGHASH_SINGLE|ANYONECANPAY` when `nIn >= txTo.vout.size()`.
- Upstream `librustzcash`'s low-level V5 sighash implementation computes an empty transparent
  outputs digest for this case, so callers need to enforce the zcashd wrapper-level validation.

Command run:

```text
cargo test -p zebra-script sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
cargo test -p zebra-script sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today --lib
cargo test -p zebra-consensus v5_sighash_single_missing_corresponding_output_is_accepted_by_transaction_verifier_today --lib
cargo test -p zebra-consensus v5_sighash_single_anyonecanpay_missing_corresponding_output_is_accepted_by_transaction_verifier_today --lib
```

Result:

```text
test tests::sighash_single_v5_p2pkh_missing_corresponding_output_is_accepted_today ... ok
test tests::sighash_single_anyonecanpay_v5_p2pkh_missing_corresponding_output_is_accepted_today ... ok
test transaction::tests::v5_sighash_single_missing_corresponding_output_is_accepted_by_transaction_verifier_today ... ok
test transaction::tests::v5_sighash_single_anyonecanpay_missing_corresponding_output_is_accepted_by_transaction_verifier_today ... ok
```

Confidence change:

- Raised from medium-high to high that the corresponding-output issue is real in Zebra's current
  script verification path.
- Raised again after the full transaction verifier accepted the same transaction shape, which
  largely eliminates "another Zebra transaction-verification layer catches it" as an explanation.
- Raised again after live source comparison found zcashd's explicit ZIP 244 corresponding-output
  rejection and upstream `librustzcash`'s lower-level empty-output digest behavior.
- This remains private-disclosure material unless/until maintainers confirm it is already covered
  by a later block/state contextual layer or upstream fix.

Later-layer review:

- `zebra-consensus::transaction::Verifier` accepts both canonical missing-output variants.
- `zebra-consensus::block::SemanticBlockVerifier` consumes transaction verifier results and then
  checks block-level totals such as sigops and miner fees; it does not re-run transparent script
  verification or inspect sighash types.
- `zebra-state` contextual validation checks UTXO existence, chain-order spends, duplicate spends,
  coinbase spend restrictions, anchors, nullifiers, and remaining transaction value; it does not
  inspect transparent signature hash modes.

Adjacent behavior:

- Pre-V5/V4 missing-output `SIGHASH_SINGLE` appears intentionally different in zcashd's pre-ZIP 244
  branch: the explicit corresponding-output rejection is in zcashd's ZIP 244 path.
- Shielded V5 signatures use `SIGHASH_ALL` with no transparent input index, so this particular
  corresponding-output rule is not relevant to shielded bundle verification.
- A wider post-disclosure source comparison did not find another zcashd ZIP 244 wrapper restriction
  of the same shape. The current zcashd C++ wrapper rejects undefined V5 hash types and missing
  corresponding outputs for both `SIGHASH_SINGLE` encodings; its Rust FFI validates hash-type parsing
  and previous-output input bounds but relies on the C++ wrapper for the corresponding-output rule.

### 2. Inbound Misbehavior Scoring Downcast

Local test:

- `zebrad/src/components/inbound/tests.rs::score_bearing_router_error_does_not_downcast_to_verify_block_error`

Behavior reproduced:

- Constructs a `VerifyBlockError` with nonzero misbehavior score.
- Wraps it through the real `RouterError::from(VerifyBlockError)` path used by the block verifier.
- Boxes the `RouterError`, then attempts the same `downcast::<VerifyBlockError>()` shape used by
  inbound cleanup.
- The downcast fails, while the original `RouterError` still carries score `100`.

Command run:

```text
cargo test -p zebrad score_bearing_router_error_does_not_downcast_to_verify_block_error --lib
```

Result:

```text
test components::inbound::tests::score_bearing_router_error_does_not_downcast_to_verify_block_error ... ok
```

Confidence change:

- Raised from high code confidence to high repro confidence for the type mismatch.
- Impact remains medium: this looks like lost peer scoring rather than consensus failure.

### 3. Mempool State Lookup Error Becomes Missing Input

Local test:

- `zebra-consensus/src/transaction/tests.rs::mempool_request_with_state_lookup_error_is_currently_missing_input`

Behavior reproduced:

- Uses the transaction verifier mempool path.
- Makes `UnspentBestChainUtxo` return a synthetic state service error.
- The verifier returns `TransactionError::TransparentInputNotFound`.

Command run:

```text
cargo test -p zebra-consensus mempool_request_with_state_lookup_error_is_currently_missing_input --lib
```

Result:

```text
test transaction::tests::mempool_request_with_state_lookup_error_is_currently_missing_input ... ok
```

Rerun on 2026-05-07: passed.

Confidence change:

- Raised confidence that an infrastructure/state lookup failure can be collapsed into a
  missing-input transaction error in the mempool verifier path.

### 4. `TransparentInputNotFound` Is Cached As Exact-Tip Failed Verification

Local test:

- `zebrad/src/components/mempool/storage/tests/vectors.rs::transparent_input_not_found_is_exact_tip_rejected_today`

Behavior reproduced:

- Feeds `TransactionDownloadVerifyError::Invalid { error: TransparentInputNotFound, .. }` into
  `Storage::reject_if_needed`.
- Storage records `MempoolError::StorageExactTip(FailedVerification(TransparentInputNotFound))`.

Command run:

```text
cargo test -p zebrad transparent_input_not_found_is_exact_tip_rejected_today --lib
```

Result:

```text
test components::mempool::storage::tests::vectors::transparent_input_not_found_is_exact_tip_rejected_today ... ok
```

Rerun on 2026-05-07: passed.

Confidence change:

- Raised confidence that the mempool can cache this collapsed error as a failed verification at the
  exact tip.
- Impact remains medium unless a broader path lets transient infrastructure errors persist long
  enough to affect user-visible transaction acceptance.

### 4b. Internal Verifier Error Is Cached As Exact-Tip Failed Verification

Local test:

- `zebrad/src/components/mempool/tests/vector.rs::mempool_internal_verifier_error_is_exact_tip_rejected_today`

Behavior reproduced:

- Queues a direct pushed transaction through the normal `Mempool` service.
- The mock verifier returns `TransactionError::InternalDowncastError`.
- After polling, queueing the same txid returns
  `MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(TransactionError::InternalDowncastError(_)))`.

Command run:

```text
cargo test -p zebrad mempool_internal_verifier_error_is_exact_tip_rejected_today --lib
```

Result:

```text
test components::mempool::tests::vector::mempool_internal_verifier_error_is_exact_tip_rejected_today ... ok
```

Confidence change:

- Raised confidence that the infrastructure-error exact-tip caching behavior is
  visible at the actual mempool service boundary, not only in isolated
  consensus/storage tests.

### 5. Height-Based `getblock` Verbosity 2 Can Panic After Snapshot Drift

Local test:

- `zebra-rpc/src/methods/tests/vectors.rs::rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today`

Behavior reproduced:

- Calls `getblock <height> 2` through `RpcImpl`.
- Mocks `ReadRequest::BlockHeader(height)` to resolve block `A`.
- Mocks `ReadRequest::Depth(A)` to return `None`, making `get_block_header()`
  return `confirmations = -1`.
- Mocks `ReadRequest::BlockAndSize(height)` to return replacement block `B`,
  matching the current height-based follow-up request shape.
- The RPC method panics while converting `-1` confirmations to `u32` for verbose
  transaction objects.

Command run:

```text
cargo test -p zebra-rpc rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today --lib
```

Result:

```text
test methods::tests::vectors::rpc_getblock_height_verbosity_2_panics_if_header_depth_reorgs_before_block_lookup_today - should panic ... ok
```

Confidence change:

- Raised confidence from code-only to high that the panic path exists and is
  unit-testable with mocked read-state response ordering.
- Practical exploitability remains medium-low because it requires RPC access
  and a non-finalized reorg or best-chain switch affecting the queried height
  between subrequests.

## Formatting

Command run:

```text
cargo fmt --all -- --check
```

Result: passed after applying `cargo fmt --all`.

## Updated Triage

- Already privately disclosed: V5 `SIGHASH_SINGLE` missing corresponding output.
- Private maintainer heads-up or security issue: mempool state lookup error collapsing into cached
  exact-tip rejection.
- Private maintainer heads-up candidate: height-based `getblock <height> 2`
  snapshot drift can panic under the mocked reorg sequence; not consensus, but
  process-fatal for reachable RPC in aborting builds.
- Maintainer issue is probably sufficient: inbound misbehavior scoring downcast.
