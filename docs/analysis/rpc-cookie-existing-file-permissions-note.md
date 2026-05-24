# RPC Cookie Existing-File Permissions Note

Date: 2026-05-02

Scope: follow-up on Zebra's JSON-RPC cookie-auth file hardening after the
v4.4.0 security-fix pass.

Private report: GHSA-g6vw-jj63-r8c4, combined with the RPC cookie lifecycle
cleanup finding.

## Finding

`zebra-rpc` creates new RPC cookie files with `0600` permissions on Unix and
rejects an already-present symlink at the cookie path. However, if `.cookie`
already exists as a regular file with broader permissions, Zebra opens it with
`create(true).truncate(true)` and rewrites the fresh cookie secret without
tightening the existing file mode.

This can preserve an insecure stale cookie file across startup. A plausible
operator path is:

1. An older or interrupted Zebra run leaves `.cookie` behind with loose
   permissions.
2. A newer Zebra run starts with cookie auth enabled.
3. `write_to_disk()` truncates and rewrites the same inode.
4. The fresh RPC credential remains readable to any local user who could read
   the old file.

This does not bypass cookie authentication by itself. It is a local credential
exposure issue for hosts where another local account, container, mounted volume,
or service can read the cookie file path.

## Evidence

- `zebra-rpc/src/server.rs:127-131` generates a fresh cookie on RPC startup and
  calls `cookie::write_to_disk(&cookie, &conf.cookie_dir)` when
  `enable_cookie_auth` is true.
- `zebra-rpc/src/server/cookie.rs:48-56` rejects a symlink that already exists
  at the cookie path.
- `zebra-rpc/src/server/cookie.rs:71-78` opens the cookie path with
  `write(true).create(true).truncate(true)` and `mode(0o600)`.
- On Unix, `OpenOptionsExt::mode(0o600)` only controls the permissions used for
  a newly created file. It does not chmod an existing regular file.
- `zebra-rpc/src/server/tests/cookie.rs:8-30` covers the newly created file
  case, and `zebra-rpc/src/server/tests/cookie.rs:33-47` covers the already
  present symlink case.
- `zebra-rpc/src/server/tests/cookie.rs:51` now locks in today's
  existing-file behavior with
  `cookie_write_preserves_existing_regular_file_permissions_today`.

Durable current-behavior proof:

```sh
cargo test -p zebra-rpc cookie_write_preserves_existing_regular_file_permissions_today --lib
```

Result: passed. The test pre-creates `.cookie` as `0644`, calls
`cookie::write_to_disk()`, and confirms the rewritten file remains `0644`
today.

## Impact

Severity: low-to-medium, local credential exposure.

The impact is bounded by several mitigations:

- JSON-RPC is disabled by default.
- Cookie auth is enabled by default when JSON-RPC is enabled.
- The default cookie directory is normally inside the node user's cache
  directory.
- A remote network attacker cannot directly read the cookie file.

The risk becomes more relevant for multi-user hosts, containers with shared
volumes, custom `rpc.cookie_dir` paths, stale cookie files left by older
versions or crashed processes, and operational setups that copied or mounted the
cookie path with permissive metadata.

Because this affects an authentication secret and partially undermines the
promise of the cookie-file hardening advisory for pre-existing files, treat it
as a private maintainer heads-up unless the Zebra team prefers local
configuration-sensitive credential issues to be filed publicly.

## Suggested Fix

Make the cookie-file write atomic and permission-tight for both new and existing
files:

- create a fresh temporary file in the same directory with `0600`,
- write the cookie and flush/sync it,
- atomically rename it over `.cookie`, replacing any existing regular file or
  symlink entry rather than following it,
- ensure the parent directory is not group/world writable unless explicitly
  accepted by configuration, and
- add a Unix regression test for rewriting an existing `0644` regular file.

Alternatively, open with no-symlink semantics where available, call
`set_permissions(0o600)` on the opened file before writing, and fail closed if
the permissions cannot be tightened. The atomic-tempfile-and-rename approach is
usually easier to reason about because it does not follow a raced symlink target.

## Confidence

Confidence: high that existing-file permissions are not tightened; medium on
real-world exposure because it depends on local filesystem state and deployment
layout.
