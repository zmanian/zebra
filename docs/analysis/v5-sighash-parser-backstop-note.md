# V5 SIGHASH_SINGLE Parser Backstop Note

Date: 2026-05-03

Status: strengthens existing private `SIGHASH_SINGLE` finding; not a separate
finding.

## Question

Could another layer catch the V5 `SIGHASH_SINGLE` missing-corresponding-output
case before Zebra's script callback accepts it?

## Short Answer

I did not find a structural parser backstop for this rule.

Zebra does re-parse every V5 transaction through
`zcash_primitives::Transaction::read()` during Zebra deserialization, but that
layer validates transaction structure: version fields, branch ID, transparent
bundle shape, Sapling and Orchard bundle encoding, value-balance ranges, and
canonical component encodings. It does not validate transparent script
signature semantics.

The missing-output rule is reached later, when the script verifier asks Zebra's
sighash callback to compute a digest for a particular transparent input and hash
type. At that point, the lower dependency accepts an in-bounds input index and
then computes an empty-output digest for `SIGHASH_SINGLE` when the corresponding
output is absent. That is useful as a digest primitive, but it is not the
ZIP-244 validation failure Zebra needs.

## Parser Boundary Trace

In Zebra V5 deserialization, the transaction is first parsed into Zebra's own
`Transaction::V5` value. Zebra then immediately calls `tx.to_librustzcash()` as
a compatibility/structural check before returning the transaction:

- `zebra-chain/src/transaction/serialize.rs:1040`
- `zebra-chain/src/transaction/serialize.rs:1050`

`to_librustzcash()` serializes the Zebra transaction back to bytes and calls
`zcash_primitives::transaction::Transaction::read()`:

- `zebra-chain/src/transaction.rs:1505`
- `zebra-chain/src/transaction.rs:1521`

In `zcash_primitives 0.27.0`, `Transaction::read()` dispatches V5 to
`read_v5()`:

- `zcash_primitives-0.27.0/src/transaction/mod.rs:816`
- `zcash_primitives-0.27.0/src/transaction/mod.rs:824`

`read_v5()` reads the header fragment, transparent bundle, Sapling bundle, and
Orchard bundle, then computes the txid from `TransactionData`:

- `zcash_primitives-0.27.0/src/transaction/mod.rs:937`
- `zcash_primitives-0.27.0/src/transaction/mod.rs:947`
- `zcash_primitives-0.27.0/src/transaction/mod.rs:969`

The unwraps I reviewed inside this boundary were guarded or infallible:

- Sapling `anchor.unwrap()` is only reached while mapping `sd_v5s`; if that
  iterator is non-empty, `n_spends > 0`, so `anchor` was set.
- Orchard `NonEmpty::from_vec(...).expect(...)` is behind an
  `actions_without_auth.is_empty()` early return.
- txid `write_*().unwrap()` calls write into in-memory hash state, and the final
  `<[u8; 32]>::try_from(txid_digest.as_bytes()).unwrap()` converts a fixed
  32-byte Blake2b output.

This parser layer can reject malformed transaction encodings, but it has no
input/output-index-specific script hash context and does not inspect the
signature hash byte from a transparent script.

## Sighash Boundary Trace

Script validation reaches `zebra-script`'s sighash callback. Zebra already
rejects undefined V5+ hash type bytes here:

- `zebra-script/src/lib.rs:186`
- `zebra-script/src/lib.rs:187`
- `zebra-script/src/lib.rs:188`

But for valid `SIGHASH_SINGLE` bytes (`0x03` or `0x83`), the callback maps
`SignedOutputs::Single` to Zebra's `HashType::SINGLE` and computes a typed
sighash:

- `zebra-script/src/lib.rs:207`
- `zebra-script/src/lib.rs:209`
- `zebra-script/src/lib.rs:216`

The lower Zebra wrapper constructs a
`zcash_transparent::sighash::SignableInput`:

- `zebra-chain/src/primitives/zcash_primitives.rs:480`

`zcash_transparent 0.7.0` only checks that the input index is in range for the
transparent input vector:

- `zcash_transparent-0.7.0/src/sighash.rs:102`
- `zcash_transparent-0.7.0/src/sighash.rs:110`

It does not check that `SIGHASH_SINGLE` has a corresponding output.

Then `zcash_primitives 0.27.0` computes the V5 transparent signature digest.
For `SIGHASH_SINGLE`, it hashes the corresponding output if present, otherwise
it hashes an empty output list:

- `zcash_primitives-0.27.0/src/transaction/sighash_v5.rs:100`
- `zcash_primitives-0.27.0/src/transaction/sighash_v5.rs:102`
- `zcash_primitives-0.27.0/src/transaction/sighash_v5.rs:105`

That is the missing backstop. The dependency gives Zebra a digest for the
missing-output shape instead of rejecting the script-validation request.

## Confidence

High that there is no structural parser catch for the V5 `SIGHASH_SINGLE`
missing-corresponding-output case.

High that the relevant enforcement point is the script-verification sighash
callback, before Zebra calls the typed V5 sighasher for `0x03` or `0x83`.

The existing private finding and repro tests remain the right vehicle for this
issue. This note just explains why `to_librustzcash()` and the upstream
transaction parser do not reduce the severity.
