# Isolated Handshake Fingerprint Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only residual of
[#3300](https://github.com/ZcashFoundation/zebra/issues/3300) and the isolated
connection design history. Do not post publicly without explicit
re-authorization.

Scope: follow-up audit of Zebra's isolated outbound connection APIs and the
wire-visible `version` message fields a remote peer can observe.

## Finding

Zebra isolated outbound connections emit a stable `version` message profile that
a remote peer can distinguish from Zebra's normal peer-set handshakes.

This is not a consensus bug and does not affect transaction or block
validation. It is a privacy hardening issue for callers that use
`connect_isolated*()` to reduce linkage between user-generated network activity
and the node's normal peer-set state.

## Preconditions

- The caller uses `zebra_network::connect_isolated`,
  `connect_isolated_tcp_direct`, or the Tor wrapper built on top of
  `connect_isolated`.
- The attacker controls or monitors the remote peer that receives the isolated
  outbound connection.
- The attacker records the initial Zcash network `version` message.

## Code Path

The isolated APIs intentionally avoid normal peer-set metadata:

- `zebra-network/src/isolated.rs` builds a normal `Handshake`, but does not call
  `with_advertised_services()` or `want_transactions()`.
- The builder therefore defaults to empty advertised services and `relay=false`.
- The isolated path uses `ConnectedAddr::new_isolated()`, even for
  `connect_isolated_tcp_direct()` where the TCP dial target is known.

In `zebra-network/src/peer/handshake.rs`, `negotiate_version()` handles
`ConnectedAddr::Isolated` by substituting an unspecified IPv4 address with the
network default port:

```rust
Isolated => {
    let unspec_ipv4 = get_unspecified_ipv4_addr(config.network);
    (unspec_ipv4.into(), PeerServices::empty(), unspec_ipv4)
}
```

The resulting local `VersionMessage` then uses:

```rust
services: our_services,
address_recv: AddrInVersion::new(their_addr, PeerServices::NODE_NETWORK),
address_from: AddrInVersion::new(our_listen_addr, our_services),
user_agent: user_agent.clone(),
start_height: minimum_peer_version.chain_tip_height(),
relay,
```

## Observed Isolated Profile

Existing isolated wire tests assert the current profile over both real TCP and
in-memory transports. The in-memory test was re-run in this pass:

```sh
cargo test -p zebra-network connect_isolated_sends_anonymised_version_message_mem --lib
```

Result on 2026-05-09: passed.

The observable isolated tuple is:

| Field | Isolated value or pattern |
| --- | --- |
| `version` | Current Zebra network protocol version |
| `services` | Empty |
| `timestamp` | Truncated to a 5-minute boundary |
| `address_recv` | `0.0.0.0:<network default port>` with `NODE_NETWORK` |
| `address_from` | `0.0.0.0:<network default port>` with empty services |
| `nonce` | Random per handshake |
| `user_agent` | Caller supplied; examples and tests use `""` |
| `start_height` | `0` because isolated uses `NoChainTip` |
| `relay` | `false` |

The timestamp bucket alone does not distinguish isolated connections from
normal Zebra peer-set connections, because the same timestamp truncation is
applied to all handshakes. The stronger isolated fingerprint is the combination
of unspecified default-port address fields, empty top-level services,
`start_height=0`, `relay=false`, and often an empty user agent.

## Comparison With Normal Zebra Handshakes

Normal peer-set handshakes are built in
`zebra-network/src/peer_set/initialize.rs` with:

- `with_advertised_services(PeerServices::NODE_NETWORK)`
- `with_user_agent(user_agent)`
- `with_latest_chain_tip(latest_chain_tip.clone())`
- `want_transactions(true)`

For non-isolated handshakes, `negotiate_version()` uses the real transient peer
address for `address_recv`, and uses the configured external address or listen
address for `address_from`.

So a remote peer can distinguish an isolated Zebra connection from a normal
Zebra peer-set connection using the initial `version` message alone. It does not
need timing side channels, follow-up requests, or multiple connections.

## Impact

Expected impact is privacy and traffic classification:

- A destination peer can classify the connection as likely coming from Zebra's
  isolated API rather than Zebra's normal node peer set.
- If the isolated connection is used for transaction submission or custom
  crawlers, that classification can help link the activity to a particular
  client behavior.
- Over Tor, the sender IP is hidden, but the destination peer can still
  classify the application-level client profile.

This does not reveal the node's normal peer set directly, does not deanonymize
Tor by itself, and does not change consensus behavior.

## Severity

Severity: low to medium privacy hardening, depending on deployment.

It is more important for wallet-like or user-transaction submission workflows
that rely on isolated connections for unlinkability. It is less important for
testing, crawling, or already-identifying workflows.

This does not need private emergency disclosure unless a downstream product is
using `connect_isolated*()` as a primary anonymity boundary and assuming remote
peers cannot classify it.

## Suggested Fixes

Treat mitigation as a separate design change, because changing `version` fields
can affect interoperability and fingerprintability in either direction.

Potential hardening options:

- Add construction-boundary tests for the isolated `VersionMessage` profile so
  future changes are deliberate and comparison-oriented.
- Reword the existing timestamp TODO: zeroing the timestamp may itself become a
  stronger fingerprint, and timestamp bucketing is not isolated-only today.
- Consider whether `connect_isolated_tcp_direct()` should preserve the known
  destination address for `address_recv`, while still avoiding address-book or
  peer-set state updates.
- Consider whether the isolated top-level `services`, `address_recv` services,
  and `relay` values should mimic a common client profile rather than a unique
  "empty services but receiving node service" combination.
- Compare candidate profiles against `zcashd`, Zebra normal handshakes, and any
  intended downstream wallet/crawler clients before changing behavior.

## Duplicate Check

Read-only duplicate search refreshed on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra isolated handshake fingerprint version message'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "isolated" "handshake" "fingerprint"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "connect_isolated" "version message"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "address_from" "relay=false" "isolated"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "isolated" "user_agent" "start_height"'
gh api repos/ZcashFoundation/zebra/issues/3300
gh api repos/ZcashFoundation/zebra/pulls/1014
gh api repos/ZcashFoundation/zebra/pulls/4870
```

Results:

- #3300 is directly related design/security history. It asks Zebra to fully
  anonymize isolated `version` timestamps for transaction broadcast and also
  mentions remote peer services in isolated network connections.
- PR #1014 introduced isolated, minimally distinguishable connections, and PR
  #4870 later returned isolated peer metadata. These are design/API history, not
  fresh vulnerability reports.
- Searches for the exact full tuple of `relay=false`, `start_height=0`, empty
  user agent, and unspecified default-port addresses returned no fresher
  dedicated issue.

## Confidence

Confidence: high that current isolated Zebra handshakes emit the documented
field tuple.

Confidence: high that this tuple distinguishes isolated Zebra connections from
normal Zebra peer-set handshakes.

Confidence: medium on cross-client uniqueness. A broader external-client
baseline is still needed to say how reliably a remote peer can distinguish
isolated Zebra from every other Zcash client, rather than from normal Zebra.
