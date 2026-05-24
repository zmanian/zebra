# RPC Cookie Lifecycle Cleanup Note

Date: 2026-05-02

Scope: follow-up on Zebra JSON-RPC cookie-auth startup and shutdown cleanup.

Private report: GHSA-g6vw-jj63-r8c4, combined with the RPC cookie existing-file
permissions finding.

## Finding

When RPC cookie authentication is enabled, `RpcServer::start()` writes a fresh
`.cookie` file before binding the JSON-RPC server. If server construction then
fails, for example because the configured RPC port is already in use, the fresh
cookie remains on disk.

The same cleanup gap appears on the normal live path used by `zebrad`: `start()`
returns only a spawned server task. The `RpcServer` struct has shutdown/drop
cleanup methods, but that struct is not returned or retained on the production
startup path. When `zebrad` exits, it aborts the returned task, which does not
call `cookie::remove_from_disk()`.

This does not let a remote attacker bypass RPC authentication by itself. It is a
local credential lifecycle hardening issue: Zebra writes an authentication
secret and can leave it behind after failed startup or shutdown.

Runtime authentication uses the in-memory `Cookie` stored in
`HttpRequestMiddleware`, not the on-disk file. So deleting the file does not
disable a live server, and a stale cookie is not useful after that server stops.
The file is the client-distribution artifact for the current secret.

## Evidence

- `zebra-rpc/src/server.rs:127-131` creates and writes the cookie before
  `Server::builder().build(listen_addr).await`.
- `zebra-rpc/src/server.rs:144-154` can fail after the cookie is already on
  disk, for example on a bind error.
- `zebra-rpc/src/server.rs:158-161` returns only the spawned server task.
- `zebra-rpc/src/server.rs:187-205` has a cleanup path that calls
  `cookie::remove_from_disk()`, but it is tied to `RpcServer::shutdown*()`.
- `zebra-rpc/src/server.rs:219-225` calls shutdown cleanup from `Drop for
  RpcServer`, but the live `RpcServer::start()` path does not construct or
  return a `RpcServer` value.
- `zebrad/src/commands/start.rs:267-270` stores the returned RPC task handle.
- `zebrad/src/commands/start.rs:538-542` stops the RPC task via
  `rpc_task_handle.abort()`, not via `RpcServer::shutdown()`.
- `zebra-rpc/src/server/cookie.rs:82-87` removes the on-disk cookie only when
  `remove_from_disk()` is explicitly called.

Durable current-behavior proof tests:

1. `zebra-rpc/src/server/tests/vectors.rs:213` occupies the configured port,
   starts RPC with cookie auth enabled, and asserts `.cookie` remains after the
   bind failure:

```sh
cargo test -p zebra-rpc rpc_server_start_failure_leaves_cookie_today --lib
```

Result: passed.

2. `zebra-rpc/src/server/tests/vectors.rs:271` starts RPC with cookie auth
   enabled, aborts the returned task, awaits the abort, and asserts `.cookie`
   remains:

```sh
cargo test -p zebra-rpc rpc_server_task_abort_leaves_cookie_today --lib
```

Result: passed.

## Impact

Severity: low-to-medium, local credential lifecycle exposure.

Mitigations:

- JSON-RPC is disabled by default.
- Cookie authentication is enabled by default when JSON-RPC is enabled.
- Newly created cookie files are intended to be owner-only on Unix.
- A stale cookie is not useful after the corresponding RPC server is no longer
  running.

The risk is more meaningful when combined with the existing-file permission
finding. If a stale `.cookie` file is left behind with loose permissions, later
Zebra startups rewrite the same file without tightening its mode, exposing fresh
RPC credentials to local readers. Shared container volumes, custom
`rpc.cookie_dir` paths, multi-user systems, and supervised restarts after bind
or configuration failures are the most plausible deployment shapes.

Because this involves authentication material and composes with the
existing-file permissions issue, treat it as a private maintainer heads-up unless
ZF prefers local credential-lifecycle issues to be filed publicly.

## Suggested Fix

Make the cookie lifecycle explicit on the actual `RpcServer::start()` path:

- bind/build the RPC server before writing the cookie when possible;
- write the cookie after successful bind but before the server begins serving,
  so failed bind/build attempts do not create fresh stale cookies and clients
  do not race a running server before the cookie exists;
- do not delete a pre-existing `.cookie` on failed bind/build unless this
  startup attempt created it, because another live process could be sharing the
  cookie directory by misconfiguration;
- if the cookie must be created earlier, wrap it in a guard that removes the
  file on every error path until ownership is handed to the running server;
- return a server handle/guard that owns both the `ServerHandle` and cookie
  path, instead of returning only `JoinHandle<Result<...>>`;
- ensure normal `zebrad` shutdown calls the guard's shutdown path rather than
  only aborting the task;
- make `cookie::remove_from_disk()` idempotent so duplicate explicit/task
  cleanup treats `NotFound` as success; and
- add regression tests for bind failure cleanup, pre-existing cookie
  preservation on bind failure, and normal shutdown cleanup.

The fix should be paired with the existing-file permission fix so a stale file
cannot preserve unsafe permissions across future starts.

## Confidence

Confidence: high that cleanup is not reached on startup failure or task abort.
Confidence is medium on real-world exposure because the cookie is local,
deployment-dependent, and stale cookies are invalid once the server stops.
