# Green Node API coverage

Target: Bun green modules assert, buffer, console, dgram, dns, events, fs, http,
http2, net, os, path, punycode, querystring, readline, sqlite, stream,
string_decoder, timers, tty, url, zlib, trace_events, and quic. `tasks/advanced.md`
owns the advanced green modules; this file remains the canonical cross-reference.

Green means Bun documents the module as broadly implemented, not that Quench
has passed Node's complete suite. Each module requires implementation evidence,
a focused fixture, and applicable upstream Node API results.

Bun-specific caveats MUST be recorded: dgram requires bind before
addMembership; dns lacks resolveTlsa and has Resolver differences; fs lacks
Temporal.Instant Stats getters; http ignores selected listen options; path
matchesGlob follows Bun.Glob semantics; stream predicates only understand Node
streams; string_decoder rejects end(string) and subclassing; tty permits
non-TTY construction; zlib/http2/quic have documented upstream failure or
experimental-surface caveats.

Current measured evidence: the focused suite passes 68/68 and the upstream
parallel manifest passes 178/178. These results cover the repository's current
fixtures, not every Bun-documented Node v26 API. New or expanded green claims
still require related Node API tests and recorded results.

Measured additions (2026-08-21): `stream` now exports the Node predicate family
`isReadable`, `isWritable`, `isErrored`, and `isDisturbed`, with `Readable` and
`Writable` destroy/errored tracking so the predicates behave correctly against
real streams. Verified by
`crates/quench-node-test/node-tests/test-stream-predicates.js` (focused suite
68/68, upstream parallel 178/178).

Measured additions (2026-08-21): `stream.Readable.from` now converts
sync/async iterables and pull-style sources with `read()`, propagating
iteration errors and honoring object-mode defaults
(`test-stream-from.js`). `node:timers/promises` gained Node-compatible
options/ref/signal validation and AbortSignal abort reason semantics for
setTimeout, setImmediate, and setInterval, plus `ref:false` unref behavior
(`test-timers-promises.js`). `node:perf_hooks` gained real `createHistogram`
(stats/percentile/delta), `monitorEventLoopDelay` (unref'd timer-tick
enable/disable/start/stop/reset metrics), and error-aware `timerify`
(`test-perf-hooks2.js`).

Measured gap (2026-08-21): `URL.canParse` and `URL.parse`/`url.parse` are not
callable in the active dispatch path — `capability_function(Custom(UrlCanParse))`
has no handler in `dispatch::url_dispatch`, so invoking it throws
`value is not callable`. The legacy `js_runtime_dispatch_url` stub returns only
`true` for `UrlCanParse` and does not implement non-throwing parse semantics.
Closing this gap requires wiring a real non-throwing URL-parse handler into the
active dispatch layer (additive capability mapping) plus a focused fixture; it
is tracked here as an explicit green gap rather than claimed complete.
`url.fileURLToPath` and `url.pathToFileURL` are present.

Measured improvements (2026-08-21): `os` now returns real host data backed by
`sysinfo` and `getifaddrs` for `cpus` (model/speed/times), `totalmem`/`freemem`,
`release`/`type`, `loadavg`, `networkInterfaces` (real IPv4 NICs), `userInfo`
(uid/gid/env), `uptime`, and `hostname`; errno constants are expanded. Verified
by `test-os.js` (focused 68/68, upstream parallel 178/178). `homedir`/`tmpdir`
remain environment-backed.

Measured improvement (2026-08-21): `assert.match` now actually asserts (with
string-pattern support) and throws on mismatch; `assert.fail` honors its
message; `assert.doesNotThrow` wraps a caught error as an AssertionError-style
failure instead of rethrowing the raw error; deep-equality terminates on cyclic
inputs via visited-pair tracking. All covered by `test-assert.js`.

Measured (2026-08-21): `events` exposes the Bun-green namespace exports
`EventEmitter`, `getMaxListeners`, `setMaxListeners`, `getEventListeners`,
`listenerCount`, and `defaultMaxListeners` (verified for `getEventListeners`
counting, identity, and post-removal behavior). Measured gap: `events.once`,
`events.addAbortListener`, `usingAsyncResource`, and the `errorMonitor`/
`captureRejectionSymbol` Symbol constants (plus `EventEmitter.captureRejections`
semantics). True `Symbol` values are not constructible and the export object
does not accept `once` via the same path that accepted `getEventListeners`;
these need a Promise-capable host export and host Symbol support, tracked here
rather than claimed complete.