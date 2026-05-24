# Open GitHub Issue Overlap For Post-4.4.0 Audit

Date: 2026-05-07

Scope: comparison between the local post-4.4.0 security-audit findings and
public issues in `ZcashFoundation/zebra`.

Posting update: after issue #10560, the user asked to stop posting public GitHub
issues for low-severity findings and keep a local record instead. See
`docs/analysis/low-severity-local-record-2026-05-07.md`.

Source command:

```sh
gh api --paginate '/repos/ZcashFoundation/zebra/issues?state=open&per_page=100'
```

This note is for disclosure routing. It does not replace the individual finding
notes or repro tests.

## Exact Or Substantial Public Coverage

| Local audit item | Public issue coverage | Notes |
| --- | --- | --- |
| ZIP-235 miner-fee share intermediate overflow | [#10519](https://github.com/ZcashFoundation/zebra/issues/10519), [#10520](https://github.com/ZcashFoundation/zebra/issues/10520) | Covered publicly. Do not re-report as fresh. |
| P2P `getblocks` / `getheaders` locator length cap | [#10549](https://github.com/ZcashFoundation/zebra/issues/10549) | Covered publicly. |
| Generic `Vec::with_capacity` preallocation from untrusted counts | [#10545](https://github.com/ZcashFoundation/zebra/issues/10545), [#10548](https://github.com/ZcashFoundation/zebra/issues/10548) | Covered publicly. |
| P2P `addr` / `addrv2` late count-cap / preallocation shape | GHSA-xr93-pcq3-pxf8, [#10545](https://github.com/ZcashFoundation/zebra/issues/10545), [#10563](https://github.com/ZcashFoundation/zebra/pull/10563), [#10570](https://github.com/ZcashFoundation/zebra/pull/10570) | Current tree rejects counts above `MAX_ADDRS_IN_MESSAGE` before `Vec::with_capacity()` via `AddrV1` / `AddrV2` `TrustedPreallocate::max_allocation()`. Treat as duplicate/regression-verification unless a new bypass is found. |
| Per-field deserialization bounds for `Halo2Proof` / transparent scripts | [#10554](https://github.com/ZcashFoundation/zebra/issues/10554) | Covered publicly. |
| P2P unknown-command frame consumes bytes then returns `Ok(None)` | [#10553](https://github.com/ZcashFoundation/zebra/issues/10553) | Covered publicly. Local follow-up confirmed already-buffered valid frames can be stranded until another read or EOF. |
| V6 transaction hash/auth-digest panic when librustzcash conversion fails | [#10534](https://github.com/ZcashFoundation/zebra/issues/10534) | Covered publicly. Local follow-up confirmed the same V6 conversion boundary also reaches an `auth_digest()` panic. |
| GBT template byte-budget mismatch | [#10552](https://github.com/ZcashFoundation/zebra/issues/10552) | Covered publicly. |
| Multi-query RPC snapshot consistency | [#10550](https://github.com/ZcashFoundation/zebra/issues/10550) | Covers the general snapshot-consistency family, including `getblock` mixing by height. |
| Peer Prometheus high-cardinality labels | [#10551](https://github.com/ZcashFoundation/zebra/issues/10551) | Covers peer labels. See partial overlap for mempool/RPC labels. |
| RPC cookie constant-time comparison | [#10546](https://github.com/ZcashFoundation/zebra/issues/10546) | Adjacent cookie-auth hardening, but not the same as file permissions/lifecycle. |
| Non-finalized transparent `received` overflow | [#10556](https://github.com/ZcashFoundation/zebra/issues/10556) | Public low-severity address-index RPC correctness hardening. |
| Regtest funding-stream validation bypass panic | [#10557](https://github.com/ZcashFoundation/zebra/issues/10557) | Public low-severity custom-network availability hardening. |
| Custom lockbox disbursement config panic | [#10558](https://github.com/ZcashFoundation/zebra/issues/10558) | Public low-severity custom-network availability hardening for NU6.1 lockbox configuration. |
| Mempool infrastructure failures cached as exact-tip rejections | [#10559](https://github.com/ZcashFoundation/zebra/issues/10559) | Public low-severity mempool availability hardening. |
| P2P `notfound` request-correlation gap | [#10560](https://github.com/ZcashFoundation/zebra/issues/10560) | Public bounded P2P availability hardening; related to but not duplicated by closed #2156/#2726/#3271/#3235. |

## Partial Overlap

| Local audit item | Public issue overlap | Gap that remains |
| --- | --- | --- |
| RPC cookie existing-file permissions and lifecycle cleanup | [#10546](https://github.com/ZcashFoundation/zebra/issues/10546), [#10404](https://github.com/ZcashFoundation/zebra/issues/10404), [#9190](https://github.com/ZcashFoundation/zebra/issues/9190) | Public issues cover comparison, alternate auth, and unauthenticated-error behavior. They do not cover preserving loose existing `.cookie` modes or leaving cookies behind on auth-enabled startup failure / task abort. |
| Mining RPC verifier timeout and error taxonomy | [#9301](https://github.com/ZcashFoundation/zebra/issues/9301), [#9727](https://github.com/ZcashFoundation/zebra/issues/9727) | Public issues cover GBT DoS and long-poll responsiveness themes, not the exact `submitblock` / proposal verifier wait and rejection-classification gap. |
| Metrics cardinality | [#10551](https://github.com/ZcashFoundation/zebra/issues/10551), [#10540](https://github.com/ZcashFoundation/zebra/issues/10540), [#10160](https://github.com/ZcashFoundation/zebra/issues/10160) | Peer labels are covered. Mempool failure `reason` labels and raw RPC `method` labels are not clearly covered. |
| Value-pool observability/accounting | [#8820](https://github.com/ZcashFoundation/zebra/issues/8820) | Public issue is metrics for deferred chain value pool, not `Block::chain_value_pool_change()` suppressing transaction-level value-balance errors. |
| Trusted indexer / `TrustedChainSync` hardening | [#8821](https://github.com/ZcashFoundation/zebra/issues/8821) | Public issue is a refactor toward `chain_tip_change()`, not the full validation-boundary and stream-supervision hardening from the audit. |
| Elasticsearch feature transport and panic hardening | Closed [#8329](https://github.com/ZcashFoundation/zebra/issues/8329), closed [#7270](https://github.com/ZcashFoundation/zebra/issues/7270) | Closed issues cover ES-unavailable panics and bulk-size panic risk. Local follow-up adds a feature-gated proof that an endpoint-controlled HTTP 200 bulk response with `"errors": true` still panics, plus the disabled TLS certificate validation concern. This is not a clean fresh private report by itself. |

## No Public Issue Match Found

These were not matched by the open-issue title/body search and should remain in
the private-heads-up or follow-up queue according to their individual severity
notes:

- RPC `longpollid` Unicode parser panic.
- RPC `z_listunifiedreceivers` invalid Sapling receiver panic.
- RPC `invalidateblock` / `reconsiderblock` non-finalized-state panics.
- Address-book misbehavior-ban panic with `max_connections_per_ip > 1`.
- Block value-pool error suppression in `Block::chain_value_pool_change()`.
- P2P transaction `getdata` pre-mempool count cap.
- P2P `mempool` request full-enumeration work.
- P2P transaction `inv` queue amplification.
- P2P BIP37 ignored-message size and request-like empty-message parsing
  hardening.
- P2P counted-header / trailing-junk parse strictness.
- P2P header-only frame body reservation pressure.
- P2P `getaddr` empty-cache repeated rescan.
- Address-book ban cleanup assuming same-IP entries are contiguous.
- Direct pushed transaction source-attribution loss.
- Lossy misbehavior report transport.
- RPC batch request count cap.
- RPC HTTP compatibility parse/rewrite amplification for strict JSON-RPC 2.0
  request and response bodies.
- RPC `text/plain` compatibility / browser-origin hardening.
- RPC pre-guard HTTP connection retention.
- RPC address-index query bounds.
- RPC solution-rate window bounds.
- RPC subtree limit overflow distinction.
- Indexer gRPC exposure and stream limits.
- Indexer `MempoolChange` privacy.
- Health endpoint connection retention.
- Tracing filter-reload body limit and auth.

## Disclosure Routing

Recommended private bundle after removing publicly covered items:

1. RPC process-fatal panics:
   `longpollid`, `z_listunifiedreceivers`, `invalidateblock`, and
   `reconsiderblock`.
2. Address-book misbehavior-ban panic for supported non-default
   `max_connections_per_ip > 1` deployments.
3. Block value-pool error suppression.
The mempool downloader timeout / stale cancel-handle retention item was
separately submitted as GHSA-89mr-m7gq-cxjm after the 2026-05-07
service-level proof.

Recommended public-hardening follow-up bucket:

- Mempool/RPC metric label cardinality not covered by peer-label issue #10551.
- Bounded P2P parser, queue, request, and routing hardening items.
- RPC exposure and resource-limit hardening items that require configured RPC
  access and do not crash the process.

## Public Comments Posted

Public-safe audit context was posted on these open issues:

- [#10551](https://github.com/ZcashFoundation/zebra/issues/10551#issuecomment-4393547108):
  sibling high-cardinality mempool `reason` and RPC `method` metric labels.
- [#10553](https://github.com/ZcashFoundation/zebra/issues/10553#issuecomment-4393573219):
  local proof-test evidence that an unknown-command frame can strand an
  already-buffered valid frame until another socket read or EOF.
- [#10534](https://github.com/ZcashFoundation/zebra/issues/10534#issuecomment-4393606461):
  sibling V6 `auth_digest()` panic evidence on the same librustzcash conversion
  boundary as the reported txid/hash panic.
- [#10549](https://github.com/ZcashFoundation/zebra/issues/10549#issuecomment-4393548171):
  local proof-test evidence for locator decode acceptance and pre-cap state
  scans.
- [#10550](https://github.com/ZcashFoundation/zebra/issues/10550#issuecomment-4393549199):
  public `getblock` snapshot-consistency regression shape.
- [#9301](https://github.com/ZcashFoundation/zebra/issues/9301#issuecomment-4393550468):
  mining RPC timeout and error-taxonomy hardening context.
- [#10552](https://github.com/ZcashFoundation/zebra/issues/10552#issuecomment-4393552792):
  final serialized block-size regression guidance for template byte-budgeting.
- [#10545](https://github.com/ZcashFoundation/zebra/issues/10545#issuecomment-4393785968):
  sibling P2P codec header-only body reservation proof and bounded hardening
  guidance.
- [#10558](https://github.com/ZcashFoundation/zebra/issues/10558):
  local proof-test evidence for configured lockbox-disbursement invalid-address
  and invalid-total panic paths.
- [#10559](https://github.com/ZcashFoundation/zebra/issues/10559):
  local proof-test evidence that verifier/state infrastructure failures can
  enter exact-tip failed-verification rejection state.
- [#10560](https://github.com/ZcashFoundation/zebra/issues/10560):
  local proof-test evidence that unsolicited or unrelated `notfound` can update
  missing-inventory routing state or complete active requests before correlation.
