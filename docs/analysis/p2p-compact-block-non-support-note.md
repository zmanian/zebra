# P2P Compact Block Non-Support Note

Date: 2026-05-02

Last updated: 2026-05-09

Disposition: local-only eliminated lead. Do not post publicly without explicit
re-authorization.

Scope: follow-up on attacker-controlled compact-block relay messages:
`sendcmpct`, `cmpctblock`, `getblocktxn`, and `blocktxn`.

## Result

Eliminated as an implemented compact-block parsing or reconstruction
vulnerability in the current Zebra checkout.

Zebra does not implement Bitcoin-style compact block relay in its P2P protocol
surface. The external `Message` enum has no compact-block variants, the wire
codec has no compact-block command arms, and the internal request/response layer
has no compact-block reconstruction, short-ID, or block-transaction-index
requests. A repo-wide search found compact-block names only in lightwalletd test
protobuf files, not in Zebra's P2P block relay implementation.

## Evidence

- `zebra-network/src/protocol/external/message.rs` lists supported P2P messages:
  `version`, `verack`, `ping`, `pong`, `reject`, `getaddr`, `addr`,
  `getblocks`, `inv`, `getheaders`, `headers`, `getdata`, `block`, `tx`,
  `notfound`, `mempool`, and BIP37 filter messages. It has no `sendcmpct`,
  `cmpctblock`, `getblocktxn`, or `blocktxn` variants.
- `zebra-network/src/protocol/external/codec.rs:441-461` dispatches known wire
  commands and has no compact-block command arms.
- `zebra-network/src/protocol/external/codec.rs:462-473` handles unknown
  commands by logging and returning `Ok(None)` after the frame has passed magic,
  body-length, and checksum checks.
- `zebra-network/src/protocol/internal/request.rs` and
  `zebra-network/src/protocol/internal/response.rs` expose inventory, headers,
  address, block, transaction, and mempool request/response types, but no
  compact-block reconstruction request or response.
- Repo-wide symbol search for `sendcmpct`, `cmpctblock`, `getblocktxn`,
  `blocktxn`, `CompactBlock`, and `BlockTransactions` only found the
  lightwalletd test protobuf `CompactBlock` definitions under
  `zebrad/tests/common/lightwalletd/lightwallet-protocol/`.

## Residual Hardening

Unsupported compact-block frames are still bounded peer work: Zebra reads the
body, verifies the checksum, logs the unknown command, and discards the frame.
That is not compact-block-specific state growth, but regression tests would make
the non-support posture explicit.

Suggested tests:

- valid wire frames for `sendcmpct`, `cmpctblock`, `getblocktxn`, and
  `blocktxn` return `Ok(None)` and leave no partial frame buffered,
- unsupported compact-block frames received during a pending peer request can
  only delay that request until `REQUEST_TIMEOUT`, not complete it incorrectly or
  corrupt connection state.

## Confidence

Confidence: high for absence of implemented compact-block parsing in Zebra's P2P
relay code. Medium for adjacent repeated-work hardening, because unsupported
unknown-command frames still cost bounded body buffering and checksum work.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra compact block sendcmpct cmpctblock getblocktxn blocktxn Zebra'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "sendcmpct" OR "cmpctblock" OR "getblocktxn" OR "blocktxn"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "compact block" "P2P"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "compact blocks" "network"'
```

No direct issue hits were returned. Broad `"compact blocks" "network"` search
only returned unrelated epics/refactors. Unsupported-command buffering remains
covered separately by the public unknown-command issue #10553.
