# Network Peerset Config Overflow Note

Date: 2026-05-09

Status: local-only. Do not post publicly without explicit user direction.

## Summary

`peerset_initial_target_size` is accepted from Zebra network configuration as a
`usize` and is only checked for zero. Large but TOML-representable values can
overflow the derived peer connection limit calculations.

The concrete proof uses:

```toml
peerset_initial_target_size = 9223372036854775807
```

On 64-bit targets this parses as `usize`, then
`Config::peerset_total_connection_limit()` panics in debug/test builds while
computing the derived inbound/outbound limits. The workspace dev profile uses
`panic = "abort"`, so this class is process-fatal there. In ordinary release
builds, unchecked integer arithmetic is more likely to wrap and produce
unexpectedly small or inconsistent connection limits.

## Evidence

The config field is exposed as `usize`:

- `zebra-network/src/config.rs:165`
- `zebra-network/src/config.rs:622`

Deserialization rejects only zero values, replacing them with defaults:

- `zebra-network/src/config.rs:931-944`

The derived limits multiply the configured value by fixed factors and then add
the results:

- `zebra-network/src/config.rs:222-240`
- `zebra-network/src/constants.rs:56-69`

Those derived values feed channel capacities, nonce retention bounds, peer-set
connection counters, and address-book worker capacity:

- `zebra-network/src/peer_set/initialize.rs:174`
- `zebra-network/src/peer_set/initialize.rs:207`
- `zebra-network/src/peer_set/initialize.rs:216`
- `zebra-network/src/address_book_updater.rs:59-67`
- `zebra-network/src/peer/handshake.rs:622`

Local proof test added:

- `zebra-network/src/config/tests/vectors.rs`
- `oversized_peerset_initial_target_size_overflows_connection_limits_today`

Verification:

```sh
cargo test -p zebra-network oversized_peerset_initial_target_size_overflows_connection_limits_today --lib
```

Result on 2026-05-09: passed.

## Impact

Likely severity: low.

This requires local/deployment configuration control. It is not a remote peer
or RPC trigger by itself.

The practical effects are:

- debug/test/profile builds can abort from oversized network config;
- release builds can wrap derived limits and behave contrary to the operator's
  configured target;
- large accepted values can feed oversized channel capacities and bookkeeping
  structures during network initialization.

## Recommended Fix Direction

- Add an upper bound for `peerset_initial_target_size`.
- Use checked arithmetic for derived connection-limit helpers.
- Return a configuration error for oversized values rather than panicking or
  wrapping.
- Consider applying the same pattern to other local numeric config knobs that
  derive resource limits through multiplication or addition.

## Duplicate Check

Local search on 2026-05-09 found tests and notes around peer limits and peer
cache sizing, but no dedicated note for `peerset_initial_target_size` overflow:

```sh
rg -n "peerset_initial_target_size|peerset_total_connection_limit|OUTBOUND_PEER_LIMIT_MULTIPLIER|INBOUND_PEER_LIMIT_MULTIPLIER|peer.*limit.*overflow|connection limit.*overflow" docs/analysis zebra-network/src/config/tests zebra-network/src/peer_set/initialize/tests -g '*.md' -g '*.rs'
```

## Confidence

High for the debug/test panic: it is backed by a direct local test.

Medium for the release-build behavior: it follows Rust's default overflow
semantics, but was not separately exercised in a release test.
