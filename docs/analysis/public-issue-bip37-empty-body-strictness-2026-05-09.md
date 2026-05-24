## Summary

Zebra currently accepts a few malformed or unsupported P2P message shapes that could be rejected earlier:

- Zebra does not advertise or implement BIP37 bloom filters, but `filterload`, `filteradd`, and `filterclear` still parse as normal `Message` variants and are later consumed by the peer connection.
- `filteradd` accepts bodies larger than the documented 520-byte element limit by parsing the first 520 bytes and treating the rest as tolerated extra data.
- `filterload` bounds total body size but does not enforce the documented `hash_functions_count <= 50` rule.
- Empty-body-style commands such as `mempool`, `getaddr`, `filterclear`, and `verack` accept and discard unexpected body bytes under the codec's generic extra-data policy.

This looks like low-severity public P2P hardening rather than a consensus issue. The global P2P message size still bounds each decoded body, but unsupported or malformed messages can consume bandwidth, checksum/parse work, and connection scheduling capacity.

Related background: #10315 tracks broad ZIP-204 conformance review, but I did not find an existing issue for these concrete unsupported-BIP37 and unexpected-body acceptance paths.

## Evidence

- `zebra-network/src/protocol/external/codec.rs` accepts message bodies up to the protocol message cap and reserves for the full declared body before command-specific decoding.
- The same codec allows extra bytes after decoding most message bodies and only logs them.
- `zebra-network/src/protocol/external/message.rs` documents BIP37 `filterload` / `filteradd` limits and says these messages are ignored because Zebra does not implement `NODE_BLOOM`.
- The codec parses `filterload` with an unrestricted `hash_functions_count` and parses `filteradd` by taking only the first 520 bytes.
- `zebra-network/src/peer/connection.rs` consumes BIP37 messages after logging that they arrived without `NODE_BLOOM`, so they do not go through the normal inbound request service overload path.

Local current-behavior tests from my audit reproduce the permissive behavior:

```sh
cargo test -p zebra-network accepted_today --lib
cargo test -p zebra-network bip37_filter_messages_are_consumed_without_inbound_request_today --lib
```

Those tests cover oversized `filteradd`, `filterload` with 51 hash functions, non-empty `filterclear` / `mempool` / `getaddr` / `verack`, and BIP37 messages being consumed without inbound service requests.

## Impact

Expected impact is bounded availability and conformance hardening:

- A peer can send large ignored `filteradd` bodies that Zebra still receives, checksums, partially copies, and logs as extra data.
- Ignored BIP37 messages are consumed at the connection layer instead of being counted through the usual inbound request handling path.
- `mempool` and `getaddr` with unexpected body bytes are still accepted by the codec before entering the inbound service path.

This does not appear to create invalid block acceptance, unbounded retained memory, or a high-confidence default-configuration crash.

## Suggested Fix

- Reject BIP37 messages at the codec/message boundary when Zebra has not negotiated `NODE_BLOOM`, or route them through an explicit misbehavior/disconnect path.
- Reject `filteradd` bodies larger than 520 bytes instead of truncating and accepting the suffix.
- Enforce `filterload.hash_functions_count <= 50` even if Zebra ignores BIP37 semantics.
- Require exact empty bodies for commands such as `mempool`, `getaddr`, `filterclear`, and `verack`, or otherwise make the intended leniency explicit.

Duplicate checks before filing found #10315 as broad related tracking, closed PR #520 as unrelated, and no exact issue for these paths.
