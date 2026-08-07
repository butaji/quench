# Current compatibility push — stream/iter and auditable baselines

## Verified progress

- Focused contracts: **1,945/1,945 passing**.
- Live inventory: 58 canonical modules, 57 statically registered, one
  platform-limited runtime omission (`node:sea`), and 186 observed Node globals.
- Latest completed differential: 4,682 fixtures with 922 exact matches, 2,461
  Quench-only failures, 537 output mismatches, 87 timeouts, and 190 fixtures
  explicitly classified as environment-limited.
- `stream/iter` now covers broadcast cancellation/abort propagation,
  `fromWritable()`, and preservation of typed-array chunks in `array()` and
  `arraySync()`.

## Current verification

- The fresh full differential completed against canonical `main`: 4,682
  fixtures, 922 exact matches, 1,399 both-failed, 2,461 Quench-only failures,
  537 output mismatches, 180 Node-only failures, 87 timeouts, and 190 explicitly
  environment-limited fixtures. No worker failed.
- Deno formatting, `cargo build -p quench-node`, and `git diff --check` pass.

## Next queue

- Refresh the decision report from the completed differential.
- Continue the owned `streams-events-async` queue, using isolated upstream
  fixtures before changing shared stream semantics.
- The current top owned queue is HTTP (56 callback failures); raw TCP framing
  cases are classified individually when their missing host transport is the
  actual cause.
- Preserve explicit platform classifications for native TLS, HTTPS, HTTP/2,
  inspector, QUIC, and other host-integrated APIs.

## Latest slice

- Stage 2034 locks the currently implemented HTTP response header surface:
  `getHeaders()`, `getHeaderNames()`, `flushHeaders()`, and
  `writeEarlyHints()`, plus request `flushHeaders()`.
- The stage passes locally; it is a focused regression guard while the broader
  HTTP callback and agent queue remains open.
- Stage 2035 adds the `http.Server.listen({ port, host }, callback)` contract,
  which was previously accepted but ignored its options object.
- Stage 2036 fixes automatic client `content-length` headers for empty POST and
  PUT requests by normalizing headers before constructing the server request.
  The focused contract now passes for GET, HEAD, DELETE, OPTIONS, POST, PUT,
  and TRACE.
- Stage 2037 adds the public `http.ClientRequest` constructor defaults for
  empty `method` and `path` options.
- Stage 2038 adds `ClientRequest` header introspection via `getHeader`,
  `getHeaders`, `getHeaderNames`, and `hasHeader`.
- Stage 2039 adds chainable `ClientRequest` socket-control methods:
  `setNoDelay`, `setSocketKeepAlive`, and `setSocketTimeout`.
- Stage 2040 adds chainable `ClientRequest.cork()` and `uncork()` buffering
  controls with balanced nesting behavior.

Stages 2034–2040 all pass together. The upstream listener-leak fixture remains
host-transport-specific: it requires native socket creation and keep-alive
reuse, while this runtime intentionally uses the in-memory HTTP transport.

The authoritative serial focused gate now passes **1,953/1,953** stages with
zero failures. The gate generated repository-root fixture artifacts during its
run; those artifacts were removed after verification, leaving the worktree
clean.

The next package-loading slice is stage 2041: the ESM resolver now searches
ancestor `node_modules` directories and reads package `exports`, `module`, and
`main` entries. This is the missing resolution layer exposed by the Hono
example.
The package loader now also honors the nearest package `type: "module"` for
`.js` files, allowing ESM package graphs such as Hono’s to load correctly.
The Hono module itself now loads under `quench-node`; its asynchronous
`app.fetch()` result still requires the runtime’s pending-Promise/microtask
drain support before a standalone script can print the response.
