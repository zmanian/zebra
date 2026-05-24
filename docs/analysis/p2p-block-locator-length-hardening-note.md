# P2P block locator length hardening note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: publicly covered by
[#10549](https://github.com/ZcashFoundation/zebra/issues/10549). Do not
re-report as fresh; keep this note as local supporting evidence.

## Summary

Peer `getblocks` and `getheaders` messages carry a block locator as a vector of
known block hashes. Zebra caps the response size (`500` block hashes or `160`
headers), but it does not cap the request locator length before searching for a
chain intersection.

The wire deserializer bounds the vector by the overall protocol message size and
the `block::Hash` trusted-preallocation cap, so this is not unbounded memory
growth. But a protocol-valid peer can still send a very large locator, forcing
Zebra to scan it and perform repeated best-chain/finalized-state membership
checks before producing a capped response.

## Evidence

- `zebra-network/src/protocol/external/message.rs` documents `GetBlocks` and
  `GetHeaders` as carrying `known_blocks: Vec<block::Hash>`.
- `zebra-network/src/protocol/external/codec.rs` deserializes both
  `known_blocks` vectors using `Vec::zcash_deserialize(&mut reader)` and does
  not apply a Bitcoin-style locator-count cap at the message layer.
- `zebra-chain/src/block.rs` allows a vector of block hashes up to the
  `(MAX_PROTOCOL_MESSAGE_LEN - 1) / 32` trusted-preallocation cap. The exact
  `getblocks` / `getheaders` locator maximum is slightly lower because the
  message body also contains the protocol version, vector length prefix, and
  stop hash.
- `zebrad/src/components/inbound.rs` forwards `known_blocks` to
  `zebra_state::Request::FindBlockHashes` / `FindBlockHeaders` unchanged.
- `zebra-state/src/service/read/find.rs` searches for the first chain
  intersection by iterating `known_blocks.iter().find(...)`, where each item can
  call into non-finalized chain and finalized DB membership checks.
- Response sizes are capped by `MAX_FIND_BLOCK_HASHES_RESULTS = 500` and
  `MAX_FIND_BLOCK_HEADERS_RESULTS = 160`, but those caps are applied after the
  request locator scan.
- `zebra-network/src/protocol/external/codec/tests/vectors.rs` includes
  `getblocks_locator_longer_than_response_cap_is_accepted_today` and
  `getheaders_locator_longer_than_response_cap_is_accepted_today`, proving that
  locators longer than the downstream response caps decode and round-trip at the
  P2P codec layer.
- `zebra-state/src/service/tests.rs` includes
  `find_blocks_scans_large_locator_before_response_cap_today`, which places the
  local tip hash after more unknown locator entries than either response cap and
  confirms `FindBlockHashes` / `FindBlockHeaders` scan far enough to find that
  late intersection.

## Public Coverage / Duplicate Check

Live issue check on 2026-05-09:

- #10549, open, `Cap getblocks/getheaders locator vector length at
  deserialization time`, covers the same core issue: the P2P codec deserializes
  locator hash vectors up to the generic `block::Hash` preallocation bound,
  while honest locators are much smaller.

Read-only GitHub searches on 2026-05-07:

```sh
gh issue list --repo ZcashFoundation/zebra --search 'block locator length getblocks getheaders in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'FindBlockHashes large locator scan in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'getblocks locator response cap in:title,body' --state all --limit 100
gh issue list --repo ZcashFoundation/zebra --search 'getheaders locator maximum in:title,body' --state all --limit 100
```

Earlier closest hit:

- #8907, closed, covers P2P `headers` response semantics and zcashd
  compatibility. It does not cover oversized request locator scan cost.

The other searches returned no hits.

Focused current-behavior tests rerun on 2026-05-07:

```sh
cargo test -p zebra-network locator_longer_than_response_cap --lib
cargo test -p zebra-state find_blocks_scans_large_locator_before_response_cap_today --lib
```

Result: both commands passed.

## Impact

This is a public P2P request-cost amplification hardening lead. A malicious peer
can send a large locator full of unknown hashes to make Zebra perform many
membership checks and then return a small capped response from genesis, after a
late match, or `Nil`.

Existing mitigations:

- overall protocol message size bounds the request;
- inbound requests are wrapped in a 5-second timeout in `zebrad`;
- inbound service buffers use load shedding;
- peer-set stall tracking can penalize peers that return bad `FindBlocks` /
  `FindHeaders` responses, but this path is about requests sent to Zebra.

## Suggested fix direction

- Add an explicit maximum locator length for inbound `getblocks` and
  `getheaders`, matching protocol expectations rather than the raw message-size
  cap.
- Truncate or reject oversized locators before state lookup.
- Add regression tests for `getblocks` / `getheaders` with locator lengths above
  the cap, verifying that state receives only the capped locator or that the peer
  request is rejected.

Disclosure triage: public hardening.

Confidence: high on missing request-locator count cap and scan-before-response-
cap behavior from source review, RepoPrompt cross-check, and local proof tests;
medium on impact because existing protocol and inbound timeout/load-shed limits
bound the damage.
