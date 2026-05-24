# RPC `longpollid` Unicode Panic Finding

Date: 2026-05-03

Scope: follow-up audit of remotely reachable panic paths in Zebra RPC request
parsing.

## Finding

`getblocktemplate` accepts an optional `longpollid` parameter. Zebra parses that
field into `LongPollId`, which first checks the UTF-8 string's byte length and
then slices the string at fixed byte offsets:

```rust
if long_poll_id.len() != LONG_POLL_ID_LENGTH {
    return Err(...);
}

Ok(Self {
    tip_height: long_poll_id[0..10].parse()?,
    tip_hash_checksum: u32::from_str_radix(&long_poll_id[10..18], 16)?,
    max_timestamp: long_poll_id[18..28].parse()?,
    mempool_transaction_count: long_poll_id[28..38].parse()?,
    mempool_transaction_content_checksum: u32::from_str_radix(
        &long_poll_id[38..LONG_POLL_ID_LENGTH],
        16,
    )?,
})
```

Rust string slicing panics if the byte range boundary is not a UTF-8 character
boundary. An RPC caller can provide a 46-byte string containing a multibyte
Unicode character that straddles offset 10, 18, 28, or 38. The length check
passes, then the fixed slice panics.

Zebra configures `panic = "abort"` for both dev and release profiles in the
workspace `Cargo.toml`, so this class is process-fatal in the actual `zebrad`
binary.

## Evidence

Call chain:

- `zebra-rpc/src/methods.rs` declares `getblocktemplate` as accepting
  `Option<GetBlockTemplateParameters>`.
- `zebra-rpc/src/methods/types/get_block_template/parameters.rs` deserializes
  `longpollid` into `Option<LongPollId>`.
- `zebra-rpc/src/methods/types/long_poll.rs` implements
  `TryFrom<String> for LongPollId` by calling `s.parse()`.
- `LongPollId::from_str()` checks `long_poll_id.len()` and then slices the
  original UTF-8 `str` at fixed byte offsets.
- The workspace `Cargo.toml` sets `panic = "abort"` in both `[profile.dev]` and
  `[profile.release]`.

Local repro tests added in this checkout:

```rust
#[test]
#[should_panic(expected = "byte index 10 is not a char boundary")]
fn non_ascii_long_poll_id_with_valid_byte_length_panics_today() {
    let long_poll_id = format!("{}é{}", "0".repeat(9), "0".repeat(35));

    assert_eq!(long_poll_id.len(), LONG_POLL_ID_LENGTH);

    let _ = long_poll_id.parse::<LongPollId>();
}

#[test]
#[should_panic(expected = "byte index 10 is not a char boundary")]
fn getblocktemplate_parameters_non_ascii_long_poll_id_panics_today() {
    let long_poll_id = format!("{}é{}", "0".repeat(9), "0".repeat(35));
    assert_eq!(long_poll_id.len(), LONG_POLL_ID_LENGTH);

    let request = serde_json::json!({ "longpollid": long_poll_id });

    let _ = serde_json::from_value::<
        crate::methods::types::get_block_template::GetBlockTemplateParameters,
    >(request);
}
```

Verification command:

```sh
cargo test -p zebra-rpc non_ascii_long_poll_id --lib
```

Result: both repro tests passed as `should_panic`.

## Sibling Parser Sweep

Follow-up parser review did not find another RPC parameter parser with the same
fixed-offset UTF-8 slicing shape.

Eliminated siblings:

- `WtxId::from_str()` in `zebra-chain/src/transaction/hash.rs` handles its
  fixed-width string by first converting to bytes, splitting the byte slice, and
  then validating UTF-8 for each half. Existing arbitrary-string parse coverage
  passed with:

  ```sh
  cargo test -p zebra-chain transaction_wtx_id_string_parse_roundtrip --lib
  ```

- `BlockTemplateTimeSource::from_str()` in
  `zebra-rpc/src/methods/types/get_block_template/proposal.rs` uses string
  prefix checks and numeric parsing, returning parse errors for malformed
  values.
- `Zec::from_str()` / `TryFrom<f64>` in
  `zebra-rpc/src/methods/types/zec.rs` return errors for malformed,
  non-integral-zatoshi, or out-of-range values.
- `opthex::deserialize()` and `arrayhex::deserialize()` in
  `zebra-rpc/src/methods.rs` map invalid hex and wrong-length arrays into serde
  errors.
- Address-list RPC parameters deserialize into `Vec<String>` and then use
  `ValidateAddresses::valid_addresses()`, which maps invalid address parse
  errors into JSON-RPC invalid-address errors. These methods still have
  availability-bound concerns, but not parser panics.
- HTTP request/response compatibility middleware has `expect()` / `assert!()`
  sites, but those operate on already-deserialized request envelopes or
  framework-generated JSON-RPC responses, not direct fixed-format attacker
  strings.

## Preconditions

- Zebra's JSON-RPC server is enabled with `rpc.listen_addr`.
- The attacker can send an RPC request:
  - they have the cookie,
  - cookie authentication is disabled,
  - or RPC is otherwise exposed through a trusted-but-shared service.

RPC is disabled by default and cookie authentication is enabled by default, so
this is not a default public internet surface. However, several mining and
Docker/lightwalletd workflows intentionally enable RPC, and some examples
disable cookie auth for compatibility.

## Attack Shape

Send a `getblocktemplate` request whose params include a `longpollid` string
that is exactly `LONG_POLL_ID_LENGTH` bytes long but not ASCII. For example, a
string with 9 ASCII digits, then `é`, then 35 ASCII digits is 46 bytes long, and
byte offset 10 falls inside the two-byte `é`.

Deserialization reaches `LongPollId::from_str()`, passes the byte-length check,
and panics on `long_poll_id[0..10]`.

## Severity

Suggested severity: private heads-up / coordinated disclosure candidate for
availability, but not a consensus issue.

The bug is a remotely triggerable process abort for deployments that expose
JSON-RPC to the attacker. It does not affect block validation or transaction
acceptance, and the default RPC/auth configuration substantially limits the
default attack surface.

## Suggested Fix

- Reject non-ASCII `longpollid` values before slicing:

  ```rust
  if !long_poll_id.is_ascii() {
      return Err("long poll id must be ASCII".into());
  }
  ```

- Or parse from `long_poll_id.as_bytes()` and validate each fixed byte range as
  ASCII digits/hex before numeric conversion.
- Convert the repro tests from `should_panic` to `expect_err` regression tests.
- Consider normalizing all fixed-format RPC string parsers to byte-slice or
  ASCII-only parsing so malformed UTF-8 content cannot hit `str` slicing panics.

## Confidence

Confidence: high on the parser panic and RPC parameter deserialization path.
Confidence: high that this is the only confirmed panic in the reviewed RPC
parser/deserialization surface. Confidence: medium-high on practical process
impact because Zebra's binary profiles use `panic = "abort"`, but I did not run
a live `zebrad` RPC server and send the HTTP request end-to-end in this pass.
