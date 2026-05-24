# P2P Log Injection B6 Recheck Note

Date: 2026-05-04

Last updated: 2026-05-09

Disposition: local-only hygiene hardening. Do not post publicly without
explicit re-authorization.

## Summary

The B6 log-injection sweep did not find a high-impact peer-controlled log
injection path. Most peer-controlled protocol values are either logged with
`Debug` formatting, summarized as counts, or bounded by protocol limits.

One small hygiene issue remains: the external-message codec logs unknown 12-byte
command names as a lossy UTF-8 string using `Display` formatting. A peer can
choose those command bytes, including control characters. This is debug-level,
length-bounded, and separate from the higher-signal Prometheus cardinality
finding, but it is worth tightening by logging the raw command bytes or a hex
encoding instead of a lossy string.

## Evidence

- `zebra-network/src/protocol/external/codec.rs:462-473` converts an unknown
  12-byte command to `String::from_utf8_lossy(&command)` and logs both
  `?command` and `%command_string`.
- `zebra-network/src/protocol/external/message.rs:459-527` implements
  `Display` for known messages using summaries such as inventory counts, header
  counts, and block hash/height rather than dumping full attacker-controlled
  message bodies.
- `zebra-network/src/protocol/external/message.rs:468-473` formats the version
  message user agent with `{:?}`, so embedded newlines or other control
  characters are escaped in that display implementation.
- `zebra-network/src/protocol/external/codec.rs:518-531` limits the version
  `user_agent` to 256 bytes before it is stored in peer state or logged.
- `zebra-network/src/peer/handshake.rs:718-723`, `750-756`, and `789-795` log
  the remote user agent with `?remote.user_agent`, which uses quoted debug
  formatting rather than raw display formatting.
- `zebra-network/src/peer/connection.rs:1187-1193` logs inbound messages through
  `msg.command()` for span fields and `%msg`/`?msg` in debug logs; known message
  `Display` is mostly summarized by command/count.

## Impact

This does not appear to be a node availability or consensus issue. The
attacker-controlled raw-ish log field is:

- limited to the 12-byte protocol command field;
- emitted at debug level;
- paired with `?command`, which preserves an escaped byte view;
- reached only when a peer sends an unknown protocol command.

The operational risk is log hygiene: a malicious peer could make debug logs
contain confusing control characters in `command_string`. That is much weaker
than the existing Prometheus high-cardinality labels, because it does not create
persistent metric time series and is not enabled in normal info-level logs.

## Suggested Fix Direction

- Replace `%command_string` with `?command_string`, or remove the lossy string
  field and rely on `?command`.
- If human readability is important, log a fixed hex encoding of the 12 command
  bytes.
- Keep using summarized `Display` implementations for full peer messages rather
  than logging raw message bodies.

## Triage

Local hygiene hardening. No private disclosure.

Confidence: medium-high that this is the only obvious raw-control-character log
path in the reviewed P2P codec/handshake slice; low severity because it is
length-bounded and debug-level.

## Duplicate Check

Read-only duplicate searches on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra P2P unknown command lossy UTF-8 log control character'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "unknown message command from peer"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "String::from_utf8_lossy" "unknown" "command"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "log injection" "P2P"'
```

The direct log-hygiene searches returned no issue hits. The exact log-message
search returned old closed PR #3120 ("Stop closing connections on unexpected
messages"), which is historical context for ignoring unknown commands rather
than a duplicate of the lossy-display logging question.

## Local Verification

Focused test rerun on 2026-05-09:

```sh
cargo test -p zebra-network unknown_command_before_valid_frame_strands_buffered_frame_today --lib
```

The command passed. This covers the current unknown-command decode behavior; the
remaining log-format point is source-evidence-backed.
