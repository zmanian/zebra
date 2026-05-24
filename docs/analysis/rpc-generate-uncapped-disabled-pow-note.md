# RPC generate uncapped disabled-PoW loop note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

The `generate` JSON-RPC method accepts a caller-supplied `u32` block count and
does not cap it before entering a synchronous RPC loop. Zebra rejects the method
on networks where proof of work is enabled, so this is not a Mainnet/Testnet
consensus issue. But Regtest enables `disable_pow` by default, and custom
Testnet configurations can also set `disable_pow = true`. On those networks, an
authenticated or exposed RPC caller can request a very large number of generated
blocks and tie up RPC, mining-template, verifier/state, and response-buffer
resources.

This is public RPC hardening, not private disclosure.

## Evidence

- The RPC trait exposes `generate(num_blocks: u32)` and documents only that the
  argument is the number of blocks to generate in
  `zebra-rpc/src/methods.rs:718-727`.
- The implementation rejects only `!network.disable_pow()` in
  `zebra-rpc/src/methods.rs:2946-2956`.
- The method then initializes `Vec::new()` and loops once per requested block in
  `zebra-rpc/src/methods.rs:2958-3013`.
- Each iteration mutates extra coinbase data, calls `get_block_template(None)`,
  builds a proposal block, serializes it, calls `submit_block(...)`, and pushes
  the generated hash into the in-memory response vector.
- Regtest parameters call `.with_disable_pow(true)` in
  `zebra-chain/src/parameters/network/testnet.rs:994-1000`.
- Custom Testnet documentation explicitly advertises `disable_pow = true` in
  `book/src/user/custom-testnets.md:14` and the example config in
  `book/src/user/custom-testnets.md:36-43`.
- `Network::disable_pow()` returns true for any Testnet parameter set with that
  flag, not only Regtest, in
  `zebra-chain/src/parameters/network/testnet.rs:1162-1168`.

## Impact

For reachable RPC deployments on disabled-PoW networks, a caller can request up
to `u32::MAX` generated blocks in one RPC call. The call is not just a cheap
counter loop: each block requires template construction, proposal conversion,
serialization, block submission, and response-vector growth. Depending on the
state and mempool configuration, this can occupy a JSON-RPC worker for a long
time, drive repeated state/verifier work, and allocate a large response.

This is low severity in normal Zebra deployments because:

- JSON-RPC is disabled by default;
- cookie authentication is enabled by default when RPC is enabled;
- Mainnet and default Testnet do not have PoW disabled, so `generate` returns an
  error before the loop;
- Regtest and disabled-PoW custom Testnets are normally local/private test
  infrastructure.

The risk rises for test infrastructure, CI services, shared custom-testnet
nodes, or copied configurations that expose RPC while running with
`disable_pow = true`.

## Documentation mismatch

The RPC documentation comment says `generate` "only works if the network of the
running zebrad process is `Regtest`" in `zebra-rpc/src/methods.rs:720-727`.
The code checks `network.disable_pow()` instead, so custom Testnets with
`disable_pow = true` can also reach the loop. That mismatch can make operators
underestimate the method's availability impact on custom Testnets.

## Suggested fix

- Reject excessive `num_blocks` values with a JSON-RPC invalid-parameter error.
- Use a small configurable maximum for a single `generate` call.
- Consider pre-allocating only after the cap, for example with
  `Vec::with_capacity(num_blocks as usize)` after validating the maximum.
- Update the RPC docs to say the method is enabled on disabled-PoW networks, or
  narrow the implementation to Regtest if that is the intended policy.
- For test tooling that genuinely needs long runs, prefer repeated small calls
  or an explicit long-running local-only workflow rather than one huge RPC
  response.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "generate" "disabled_pow" "num_blocks" "cap"'
```

No issue hits were returned.

## Local Proof Status

Current-behavior proof added and rerun on 2026-05-09:

```sh
cargo test -p zebra-rpc rpc_generate_u32_max_reaches_template_work_on_disabled_pow_today --lib
cargo test -p zebra-rpc rpc_generate --lib
```

Result: passed.

The new test lives in `zebra-rpc/src/methods/tests/vectors.rs`. It constructs a
Regtest RPC instance with mining configured, calls `generate(u32::MAX)`, and
observes the first `ReadRequest::ChainInfo` request from the template path. That
proves the maximum caller-supplied block count reaches generation work on a
disabled-PoW network instead of being rejected at the parameter boundary.

## Confidence

Confidence: high on the missing cap and disabled-PoW reachability. Confidence:
low-medium on practical severity because the method is blocked on normal
networks and RPC should be trusted/private in the affected deployments.
