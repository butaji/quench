# Node.js compatibility stages

This file defines the v1 acceptance stages for `quench-node`. Each stage
is one or more `node:` modules, a slice of the `node_modules` resolver, or
a group of npm packages from the green-list, that must run end-to-end
through the canonical `quench-node-test` runner against the pinned
upstream `node-tests/` submodule before the next stage begins. There
are no skip policies and no checkpoints: each stage runs through the
exact upstream fixtures and harnesses, and observable behavior is
verified at execution time, never worked around.

This is a definition document only — it is not a progress ledger.
Verify stages with the relevant commands and Node test runs at
execution time. Pass counts, stage totals, completion percentages, and
failure inventories belong in the runner output and the commit
history, never in this repository. `AGENTS.md` rule 16 is the
authoritative source for that boundary.

## Execution order

Stages are grouped by domain and revisited when needed if a later
stage exposes a semantic gap. The grouping is intentionally coarse so
that each stage can hold multiple modules whose semantics are tightly
coupled (e.g. `path` + `querystring` + `url` are all string-parsing
primitives; `events` + `stream` + `string_decoder` are all event /
flow primitives).

1. **Foundations.** `assert`, `buffer`, `console`, `util`, `string_decoder`,
   `punycode`. Pure-JS / pure-Rust; no kernel I/O, no resolver.
2. **String and path primitives.** `path`, `path/win32`, `path/posix`,
   `querystring`, `url` (legacy + WHATWG), WHATWG `URL` global. The
   runtime's existing kernel table covers most of this.
3. **Process and timers.** `process` (partial), `timers`,
   `timers/promises`, `setImmediate` / `clearImmediate`,
   `setInterval` / `clearInterval`, `setTimeout` / `clearTimeout`,
   `queueMicrotask`, `performance` (subset), `perf_hooks` (subset).
   Drives the host event loop.
4. **Events and streams.** `events`, `stream`, `stream/web`,
   `stream/consumers`, `stream/promises`. This stage is the load-bearing
   stage for everything else; it ships the `Resource` and `Stream`
   state machines.
5. **Operating system and TTY.** `os`, `tty`, `tty` ReadStream /
   WriteStream. No I/O yet, just sysinfo and TTY detection.
6. **File system.** `fs`, `fs/promises`. The full `fs` surface from the
   Bun 🟢 list, on a `spawn_blocking` pool; `fs.watch` uses `mio`
   readiness. This is the first stage that exercises the host event
   loop end-to-end.
7. **Networking primitives.** `net`, `dns`, `dgram`. `mio`-driven TCP,
   UDP, and the resolver. Unix-domain sockets included.
8. **HTTP and HTTP/2.** `http`, `http2`, `https` (request/get/Agent only;
   `https.Server` is `http.Server` + TLS options per the Bun matrix).
   `net` is a prerequisite.
9. **Compression and tracing.** `zlib`, `trace_events`. `zlib` runs on
   the worker-thread pool; `trace_events` is a write-only event sink.
10. **Interactive I/O and SQLite.** `readline`, `sqlite`,
    `node:readline/promises`. `sqlite` is its own stage because the
    `rusqlite` kernel is a meaningful new dependency.
11. **QUIC.** `quic`. Last because it is the most experimental item on
    the 🟢 list and depends on the cleanest version of the I/O loop.
12. **Module resolver.** The `require()` / `import` machinery: the
    `node_modules` walk, the `exports` / `imports` map, conditional
    exports with the `import` / `require` / `node` / `default`
    conditions, CJS↔ESM interop, `createRequire`, `require.cache`,
    `require.resolve`. This stage is **not** gated on a single module;
    it is gated on the green-list of npm packages, see below.
13. **Green-list acceptance.** The hand-picked set of 20–50 real
    packages from the "servers + libraries" tier (Hono, Express,
    Fastify, Koa, `ws`, `graphql-yoga`, Prisma-style clients, `dayjs`,
    `lodash`, `zod`, …) plus a representative npm-only utility per
    stage above. Each must run from a published npm version under
    `quench-node ./script.js` with a passing exit code. The exact
    package set is the worker's first assignment per stage and is
    expected to grow.

## npm-package green-list (subset, illustrative)

The exact list is set by the first coordinator handoff after this ADR
is accepted. The illustrative minimum:

- **Servers:** `hono`, `express`, `fastify`, `koa`, `@koa/router`.
- **Realtime:** `ws`, `socket.io` (server side only).
- **GraphQL:** `graphql`, `graphql-yoga`.
- **Validation / types:** `zod`, `valibot`.
- **Data clients:** `pg` (Postgres), `mysql2`, `better-sqlite3` (if a
  compatible `node:sqlite` lands in v1), `ioredis`.
- **Utilities:** `lodash`, `dayjs`, `date-fns`, `nanoid`, `chalk`,
  `pino`, `debug`, `dotenv`.
- **Testing consumers (run under their own runner, not
  `quench-node-test`):** `vitest` subset, `uvu` subset, `node:test`
  consumer-only subset.

A package that depends on a 🟡 Bun item (`worker_threads`,
`https.Server`, `tls` server features, `cluster`, `vm.Script` options,
`inspector.Session` non-`Profiler` domains, `wasi` extensions,
`perf_hooks` Node-only entries) is **out of scope for v1** and is
tracked in the v2 backlog instead of the green-list.

## Excluded from v1 stable coverage

- Anything on Bun's 🟡 list that this ADR does not explicitly call
  out as in scope.
- `node:sea` (Bun 🔴).
- Next.js, Nuxt, SvelteKit dev mode, `next build`, production SSR.
  These need a framework CLI + a build step and are not part of the
  "servers + libraries" tier.
- Performance targets ("beat V8 on web apps," "lowest mem/RSS in the
  Quench family"). Captured in `docs/adr/0002-quench-node-scope.md`
  section 9 as a downstream aspiration; not part of v1 acceptance.

## Verification

Each stage is verified by the canonical runner:

```bash
cargo run -p quench-node-test --bin run-stages -- --from <first> --to <last> --continue
```

`triage` is a diagnostic tool for clustering or focusing failures; it
is not the conformance gate:

```bash
cargo run -p quench-node-test --bin triage -- <node-tests-subdir>
```

The final v1 report must show no failures, no skips, and no
unexpected results across stages 1–13. The v1 campaign is complete
only on `main` (or on this branch, if the coordinator's topology
decides to track the Node host on a separate branch — see
`GOAL.md`'s "Team topology"), after a fresh full-suite run succeeds.
