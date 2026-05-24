# RPC `z_listunifiedreceivers` Invalid Sapling Receiver Panic Finding

Date: 2026-05-03

Scope: follow-up audit of RPC address parsing paths after the
`getblocktemplate` `longpollid` panic finding.

## Finding

`z_listunifiedreceivers` accepts a Unified Address string, decodes it with
`zcash_address::unified::Encoding::decode()`, and then iterates over the decoded
receiver items.

For Sapling receivers, Zebra treats successful Unified Address decoding as if it
also proves the Sapling receiver bytes are semantically valid:

```rust
zcash_address::unified::Receiver::Sapling(data) => {
    let addr = zebra_chain::primitives::Address::try_from_sapling(network, data)
        .expect("using data already decoded as valid");
    sapling = Some(addr.payment_address().unwrap_or_default());
}
```

That assumption is false. The `zcash_address` Unified Address parser enforces
the receiver typecode and byte length, but a Sapling receiver is represented as
`[u8; 43]`; it does not prove those bytes form a valid Sapling payment address.
Zebra's later `try_from_sapling()` call performs that semantic check with
`sapling_crypto::PaymentAddress::from_bytes(&data)`, returning an error for
invalid receiver bytes. The RPC method unwraps that error with `expect()`.

Because Zebra configures `panic = "abort"` for both dev and release profiles,
this panic is process-fatal in the actual `zebrad` binary.

## Evidence

Call chain:

- `zebra-rpc/src/methods.rs:689-691` exposes
  `z_listunifiedreceivers` as a JSON-RPC method.
- `zebra-rpc/src/methods.rs:2873-2877` decodes the caller-provided string as a
  `zcash_address::unified::Address`.
- `zebra-rpc/src/methods.rs:2884-2910` iterates decoded receiver items.
- `zebra-rpc/src/methods.rs:2891-2894` calls
  `Address::try_from_sapling()` and unwraps the result with `expect()`.
- `zebra-chain/src/primitives/address.rs:67-75` validates Sapling receiver
  bytes with `sapling_crypto::PaymentAddress::from_bytes(&data)`, returning an
  error when the bytes are not a valid Sapling payment address.
- `zebra-chain/src/primitives/address.rs:77-131` already has a whole-Unified
  Address conversion path that rejects invalid Sapling and Orchard receivers
  instead of treating structural UA decoding as enough validation.
- `zcash_address 0.11.0` represents `Receiver::Sapling` as `[u8; 43]`, and its
  receiver parser maps the Sapling typecode by `addr.try_into()`, enforcing
  length but not Sapling payment-address validity.
- `Cargo.toml:183-185` and `Cargo.toml:304-305` set `panic = "abort"` for the
  dev and release profiles.

Existing Zebra coverage checks a fully invalid address string and two valid
Unified Address vectors in
`zebra-rpc/src/methods/tests/vectors.rs:3159-3216`. This audit added a durable
current-behavior proof for a syntactically valid Unified Address with a
semantically invalid Sapling receiver.

## Local Reproducer

Added a durable current-behavior unit test:
`zebra-rpc/src/methods/tests/vectors.rs:3220`.

The test constructs this panic-only proof shape:

```rust
#[tokio::test(flavor = "multi_thread")]
#[should_panic(expected = "using data already decoded as valid")]
async fn rpc_z_listunifiedreceivers_panics_on_invalid_sapling_receiver_today() {
    use zcash_address::unified::{Encoding, Receiver};

    let invalid_sapling_ua =
        zcash_address::unified::Address::try_from_items(vec![Receiver::Sapling([0; 43])])
            .expect("single receiver UA shape is valid")
            .encode(&NetworkType::Main);

    let _ = rpc.z_list_unified_receivers(invalid_sapling_ua).await;
}
```

Verification command:

```sh
cargo test -p zebra-rpc rpc_z_listunifiedreceivers_panics_on_invalid_sapling_receiver_today --lib
```

Result:

```text
running 1 test
test methods::tests::vectors::rpc_z_listunifiedreceivers_panics_on_invalid_sapling_receiver_today - should panic ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.89s
```

## Preconditions

- Zebra's JSON-RPC server is enabled with `rpc.listen_addr`.
- The attacker can send an RPC request:
  - they have the cookie,
  - cookie authentication is disabled,
  - or RPC is otherwise exposed through a trusted-but-shared service.

RPC is disabled by default and cookie authentication is enabled by default. This
is therefore not a default public internet surface, but copied Docker/mining or
shared-service deployments can make RPC reachable by untrusted users.

## Attack Shape

Construct a syntactically valid Unified Address containing a length-valid but
semantically invalid Sapling receiver, then call `z_listunifiedreceivers` with
that address string.

One simple local construction is a single Sapling receiver with all-zero 43-byte
receiver data encoded through `zcash_address::unified::Address::try_from_items`.
The Unified Address parser accepts the encoded address shape, but Zebra's
Sapling semantic conversion rejects the receiver bytes and hits the `expect()`
panic.

## Severity

Suggested severity: private heads-up / coordinated disclosure candidate for RPC
availability, but not a consensus issue.

The bug is a remotely triggerable process abort for deployments that expose
JSON-RPC to the attacker. It does not affect block validation or transaction
acceptance, and Zebra's default RPC/auth configuration substantially limits the
default attack surface.

## Suggested Fix

- Replace the Sapling `expect()` in `z_list_unified_receivers()` with normal
  JSON-RPC invalid-address error handling.
- Prefer validating the whole Unified Address through
  `zebra_chain::primitives::Address::try_from_unified(network, unified_address)`
  before listing receivers, so invalid Sapling and Orchard receiver bytes follow
  ZIP 316's "MUST reject" rule.
- Keep transparent receiver handling simple: `try_from_transparent_p2pkh()` and
  `try_from_transparent_p2sh()` are currently infallible for `[u8; 20]` payloads.
- Review the Orchard branch as well. It currently re-encodes a decoded Orchard
  item as an Orchard-only Unified Address without semantic validation. That does
  not trigger this panic, but it appears to share the same "structural decode is
  enough" assumption.
- Convert the temporary `should_panic` probe into an `expect_err` regression
  test once the fix exists.

## Adjacent Address RPC Check

The sibling `validateaddress` and `z_validateaddress` RPCs do not appear to
repeat this panic. They parse the caller string as a `zcash_address::ZcashAddress`
and then call `convert::<zebra_chain::primitives::Address>()`, which routes
Unified Addresses through `Address::try_from_unified()`.

A temporary proof test constructed Unified Addresses from
`Receiver::Sapling([0; 43])` and `Receiver::Orchard([0; 43])` and called both
validation RPCs. Both methods returned their normal invalid-address response
without panicking:

```sh
cargo test -p zebra-rpc rpc_validateaddress_rejects_semantically_invalid_unified_receivers --lib
```

Result: `1 passed; 71 filtered out; finished in 0.01s`.

## Follow-up Orchard Receiver Check

The Orchard branch in `z_listunifiedreceivers` does not reproduce the process
abort, because it does not call Orchard semantic validation and unwrap that
result. Instead, it re-wraps the decoded receiver item with
`zcash_address::unified::Address::try_from_items(vec![item])` and encodes the
result as an Orchard-only Unified Address.

A temporary proof test constructed a Unified Address from
`Receiver::Orchard([0; 43])`, verified that the whole-address Zebra conversion
rejects it through `Address::try_from_unified()`, then called
`z_listunifiedreceivers`. The RPC returned `Ok` with a populated `orchard` field
and no other receivers:

```sh
cargo test -p zebra-rpc rpc_z_listunifiedreceivers_echoes_invalid_orchard_receiver_today --lib
```

Result: `1 passed; 71 filtered out; finished in 0.00s`.

This is lower severity than the Sapling panic: it is a validation/compatibility
bug in an RPC helper method, not a process-fatal availability bug. It reinforces
the same fix direction: validate the whole Unified Address before splitting and
returning constituent receiver strings.

## Confidence

Confidence: high on the panic and the direct RPC method path. The temporary
unit test proves that a syntactically valid Unified Address with invalid Sapling
receiver bytes reaches the `expect()` panic.

Confidence: high that this is process-fatal for the `zebrad` binary because the
workspace uses `panic = "abort"`.

Confidence: medium-high on practical severity because default RPC/auth settings
reduce exposure, but the method is a configured-RPC availability sink whenever
RPC is reachable by an attacker.
