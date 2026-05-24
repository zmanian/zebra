# P2P Header Command Trace Unwrap Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

## Summary

`zebra-network/src/protocol/external/codec.rs` has a panic-looking unwrap while
tracing inbound P2P message headers:

```rust
command = %String::from_utf8(
    command.iter()
        .cloned()
        .flat_map(std::ascii::escape_default)
        .collect()
).unwrap(),
```

The `command` bytes are peer-controlled header bytes, but this is not a remote
panic path. Every input byte is passed through `std::ascii::escape_default`
before `String::from_utf8()`. That iterator emits ASCII escape bytes, and ASCII
is always valid UTF-8.

Classification: eliminated P2P panic lead, not a private disclosure candidate.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:370-387` reads the 12-byte
  message command from the peer header, escapes every byte with
  `std::ascii::escape_default`, and unwraps `String::from_utf8()`.
- `std::ascii::escape_default` emits printable ASCII bytes or ASCII escape
  sequences for arbitrary input bytes. The resulting byte vector is therefore
  valid UTF-8 even when the original command bytes are not.
- `zebra-network/src/protocol/external/codec.rs:393-397` rejects invalid network
  magic and oversized bodies before moving to body parsing.
- `zebra-network/src/protocol/external/codec.rs:446-465` later matches the raw
  12-byte command against known command strings and returns a parse error for
  unknown commands.
- The nearby version timestamp parser is not an unwrap path:
  `zebra-network/src/protocol/external/codec.rs:509-514` converts
  `timestamp_opt(...).single()` into `Error::Parse`.
- `zebra-network/src/protocol/external/codec/tests/vectors.rs:70-90` checks that
  out-of-range version timestamps are rejected with parse errors.

## Triage

No source-grounded vulnerability found.

Residual hardening: replacing the unwrap with
`String::from_utf8_lossy(...).into_owned()` or a short helper such as
`escaped_ascii_command(command)` would make the invariant more obvious, but this
would be readability hardening rather than a security fix.

## Duplicate Check

Read-only duplicate searches on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra P2P command String::from_utf8 escape_default unwrap'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "String::from_utf8" "escape_default" "command"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "command_bytes" "from_utf8"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown command" "from_utf8_lossy"'
```

No issue hits were returned.

## Local Verification

Focused tests rerun on 2026-05-09:

```sh
cargo test -p zebra-network version_timestamp_out_of_range --lib
cargo test -p zebra-network unknown_command_before_valid_frame_strands_buffered_frame_today --lib
```

Both commands passed.
