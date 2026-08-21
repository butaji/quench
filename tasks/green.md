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

Measured improvement (2026-08-21): the active dispatch URL constructor now
provides non-throwing `URL.canParse` and `URL.parse` statics. Valid relative
URLs honor a base; invalid inputs return `false`/`null`, including malformed
colon-prefixed input. The focused Node API fixture is
`crates/quench-node-test/node-tests/test-url-static.js`; its definition of done
is a passing focused runner, a passing Node CLI oracle, and clean
`run-compat`/`run-parallel` results. The legacy `url.parse` path remains
separate and is not claimed as equivalent to the WHATWG static parser.
`url.fileURLToPath` and `url.pathToFileURL` are present.
Measured improvement (2026-08-21): legacy `url.parse()` now separates the
scheme, authority, pathname, query, and combined `path` for ordinary HTTP(S)
URLs. Focused `test-url.js` asserts those Node fields; the Node CLI oracle and
the full focused/upstream gates pass. Complex legacy parser edge cases remain
outside this slice.
The separate default `quench-node` runtime remains an explicit gap: its
host-value URL constructor does not yet preserve the same static properties
through the VM host-value boundary, so its direct CLI smoke must not be counted
as verified by this dispatch-path result.

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
semantics). Three architectural blockers were confirmed during implementation
attempts: (1) `AbortController#abort()` itself throws `value is not callable`
in the runtime (a broken stub in `js_runtime_adapters.rs:94` and an absent
runtime registration in `host::install_with_argv`), so `events.addAbortListener`
cannot be verified end-to-end until a real AbortController/AbortSignal pair
is wired through the run harness; (2) `events.once` would need a per-call
Rust closure that resolves a specific `PromiseData` on the next emit, but
the host dispatch layer has no per-instance state for static capability
functions (all `capability_function(Custom(name))` values share the same
dispatch target); (3) JS-side polyfills via `quench_runtime::reduce::
reduce_global_script_source` + `vm::call_value` cannot survive across the
`host_value` boundary — the returned constructor becomes a native function
(`function () { [native code] }`) that drops its captured closure state, so
inline methods, prototype assignments, and IIFE-captured state are all
lost when the host value is invoked from a user script. True `Symbol` values
are also not constructible. Closing these gaps needs a Promise-capable
host export with per-call state, a JS-callable host function API (so closures
survive), host Symbol support, and fixing the AbortController stub at the
same time; this is multi-day host-engine work, tracked here rather than
claimed complete.