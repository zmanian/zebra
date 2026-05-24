# Vulnerability Finding: V5+ `SIGHASH_SINGLE` Missing Corresponding-Output Failure

Date: 2026-05-02

Status: Local security finding; needs Zebra maintainer confirmation before public disclosure.

Scope: Zebra `main` at `589d64b9b` / release `v4.4.0`.

Related references:

- Zebra 4.4.0 security note: <https://forum.zcashcommunity.com/t/zebra-4-4-0-critical-security-fixes/55576>
- ZIP-244: <https://zips.z.cash/zip-0244>
- PR mentioned by the advisory: <https://github.com/ZcashFoundation/zebra/pull/10510>

## Summary

Zebra still appears to be missing the ZIP-244 validation failure for V5+
transparent spends that use `SIGHASH_SINGLE` without a corresponding transparent
output.

ZIP-244 says validation must fail for:

- undefined V5+ `hash_type` values, and
- `SIGHASH_SINGLE` when the input being verified has no transparent output at
  the same index.

The current Zebra code implements the first restriction, but I did not find the
second restriction in either the script-verification callback or the lower
`zebra-chain` sighash wrapper.

## Impact

This is a consensus-divergence risk.

An attacker can construct a V5+ transaction with a transparent input signed using
`SIGHASH_SINGLE` or `SIGHASH_SINGLE | ANYONECANPAY` where the input index is
greater than or equal to the transaction's transparent output count. Under
ZIP-244 and `zcashd`, that signature validation path fails because there is no
"corresponding output".

In Zebra, the current path can compute a digest for the missing-output case
instead of forcing validation failure. If the signature is made for that digest,
Zebra can accept a transaction that `zcashd` rejects.

This can flow through:

- `sendrawtransaction`, which queues the transaction for mempool verification;
- the mempool, which stores `VerifiedUnminedTx` values;
- `getblocktemplate`, which trusts those verified mempool transactions when
  constructing templates; and
- `submitblock` / block verification, which uses the same script verifier.

That makes the issue more practically relevant than a mempool-only divergence:
a Zebra-backed miner could mine a block that Zebra accepts but `zcashd` rejects.

## Evidence

### ZIP-244 Requirement

ZIP-244 defines the V5+ transparent signature digest and says the following
restrictions cause validation failure:

- using an undefined `hash_type`, and
- using `SIGHASH_SINGLE` without a corresponding output.

It also defines the missing-output digest value for the digest algorithm, but
the validation rule is separate: the missing-output `SIGHASH_SINGLE` case must
not be accepted as a valid transparent spend.

### Script Verifier Path

`zebra-script/src/lib.rs` checks only input-side alignment before script
verification:

- `CachedFfiTransaction::is_valid()` checks that `all_previous_outputs` has an
  entry for `input_index`.
- It also checks that `all_previous_outputs.len() == transaction.inputs().len()`.
- It then indexes `transaction.inputs()[input_index]`.

Relevant location:

- `zebra-script/src/lib.rs:147`

The V5+ hash-type callback rejects undefined bytes:

- valid values are checked against `{0x01, 0x02, 0x03, 0x81, 0x82, 0x83}`;
- invalid values return no computed sighash and are replaced with random bytes
  so signature verification fails.

Relevant location:

- `zebra-script/src/lib.rs:186`

But when the hash type is `SIGHASH_SINGLE`, the callback maps it directly to
Zebra's typed sighash value and computes the digest:

- `SignedOutputs::Single => HashType::SINGLE`
- optional `ANYONECANPAY` flag is applied
- `self.sighasher().sighash(our_hash_type, Some((input_index, script_code_vec)))`
  is called

Relevant location:

- `zebra-script/src/lib.rs:207`

I did not find a check equivalent to:

```rust
if transaction.version() >= 5
    && matches!(hash_type.signed_outputs(), SignedOutputs::Single)
    && input_index >= transaction.outputs().len()
{
    fail_signature_validation();
}
```

### `zebra-chain` Sighash Path

The internal sighash error type has input-side error cases, but no output-side
error for `SIGHASH_SINGLE` without a corresponding output:

- `InputIndexOutOfBounds`
- `NoTransparentBundle`
- `BundleInputCountMismatch`
- `InvalidPreviousOutputAmount`

Relevant location:

- `zebra-chain/src/primitives/zcash_primitives.rs:307`

The lower sighash path builds a `zcash_transparent::sighash::SignableInput`
using the input index and maps construction failure to an input-count mismatch:

- `SignableInput::from_parts(...)`
- error becomes `BundleInputCountMismatch`

Relevant location:

- `zebra-chain/src/primitives/zcash_primitives.rs:480`

This call does not check `bundle.vout.len()` for `SIGHASH_SINGLE`.

### Dependency Behavior

The locked dependency versions are:

- `zcash_primitives 0.27.0`
- `zcash_transparent 0.7.0`

Relevant location:

- `Cargo.lock:7510`
- `Cargo.lock:7603`

In the local cargo registry, `zcash_transparent::sighash::SignableInput::from_parts`
only rejects `index >= bundle.vin.len()`. It does not inspect `bundle.vout`.

In `zcash_primitives` 0.27.0, the V5 sighash code handles `SIGHASH_SINGLE` by
hashing the transparent output at the input index if it exists, otherwise hashing
an empty output list. That behavior is correct as a digest primitive, but Zebra
still needs a validation pre-check to match ZIP-244's script failure rule.

## Exploit Sketch

The minimal shape is:

1. Create a V5+ transaction with more transparent inputs than transparent
   outputs.
2. Choose an input index with no corresponding output.
3. Use `SIGHASH_SINGLE` or `SIGHASH_SINGLE | ANYONECANPAY` for that input.
4. Sign the digest Zebra computes for the missing-output case.
5. Submit the transaction to Zebra.

Expected behavior:

- `zcashd`: rejects during transparent script validation.
- Zebra: can accept if the signature matches Zebra's computed digest.

## Recommended Fix

Add an explicit V5+ pre-check before computing the sighash for transparent script
validation:

- If the raw hash type is one of the valid V5+ `SIGHASH_SINGLE` encodings
  (`0x03` or `0x83`), require `input_index < transaction.outputs().len()`.
- On failure, use the same failure mechanism as invalid V5+ hash types in the FFI
  callback: return the random dummy sighash so signature verification fails.

The check can live in `zebra-script/src/lib.rs` near the existing V5+ hash-type
validation. That keeps the failure aligned with script verification and avoids
turning a malformed transaction into a panic in the lower `zebra-chain` sighash
wrapper.

Add regression coverage for both:

- `SIGHASH_SINGLE` with `input_index >= outputs.len()`, and
- `SIGHASH_SINGLE | ANYONECANPAY` with `input_index >= outputs.len()`.

The test should assert that Zebra rejects the spend in the script verifier, not
merely that the lower sighash function computes a digest.

## Related Mining/RPC Hardening Observation

While reviewing this in light of the Litecoin MWEB incident, I also found a
separate hardening gap: Zebra's mining RPC paths call the raw block verifier
without an RPC-level timeout.

The block verifier router documentation says block and transaction verification
requests should be wrapped in a timeout because out-of-order and invalid
requests can hang indefinitely.

Relevant locations:

- `zebra-consensus/src/router.rs:8`
- `zebra-consensus/src/router.rs:242`

The sync and inbound paths add verifier timeouts, but these RPC paths await the
raw verifier directly:

- `submitblock`: `zebra-rpc/src/methods.rs:2573`
- `getblocktemplate` proposal validation:
  `zebra-rpc/src/methods/types/get_block_template.rs:657`

I do not currently have a concrete malformed block that triggers a hang in this
code, so this should be treated as defense-in-depth rather than a confirmed
vulnerability. It is still worth hardening because Litecoin's April incident
showed how a rejected mutated block can still break mining RPC liveness.

## Negative Findings

I did not find another allocation-amplification issue comparable to the Zebra
4.4.0 advisory in the reviewed parser areas:

- `headers` message count is checked against `MAX_HEADERS_PER_MESSAGE` before
  counted-header allocation.
- `addr` and `addrv2` have protocol count caps.
- `addrv2` address byte length is capped.
- inbound message body size is capped before reserving the body buffer.
- generic vectors and byte vectors use `TrustedPreallocate` /
  `MAX_U8_ALLOCATION`.
- coinbase Sapling spend vectors are rejected before allocation in the patched
  transaction deserialization paths.

One low-severity parser conformance note remains: `CountedHeader` deserialization
reads but ignores the transaction-count field, even though header-only messages
should encode zero transactions. This does not appear to create an allocation
amplification issue because `headers` enforces the 160-header cap before
deserializing counted headers.

Relevant location:

- `zebra-chain/src/block/serialize.rs:124`

## Verification Performed

The following targeted tests passed:

```console
cargo test -p zebra-script sighash_divergence --lib
cargo test -p zebra-network preallocate --lib
cargo test -p zebra-chain preallocate --lib
```

These tests confirm the existing stale-buffer and allocation regression coverage
still passes. They do not prove the `SIGHASH_SINGLE` missing-output behavior is
fixed; that regression test appears to be absent.
