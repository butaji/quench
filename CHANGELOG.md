# Changelog

## 2026-08 — Node API host slice

- `quench-node` is a pure-Rust Node API compatibility host built
  on top of `quench-runtime`. It exposes 26 modules (assert,
  buffer, cluster, console, dgram, dns, events, fs, https,
  inspector, net, os, path, perf_hooks, process, querystring,
  repl, sea, stream, timers, tls, trace_events, tty, url, util,
  wasi, worker_threads, plus require)
  through a single Host trait implementation + a capability
  dispatch table. No self-hosted JS builtin layer; no JS bridge.

- `quench-node-test` owns the compat suite at
  `crates/quench-node-test/node-tests/` (a plain directory of
  17 Node compat API test scripts) and a `run-compat` runner
  that classifies each as Pass / Fail / Skip.

- Major slices:

  - Initial Node host + Hono example
  - Dispatch table lint trim (single-line trampolines, 500-line cap)
  - EventEmitter: `EmitterRegistry` + receiver-aware dispatch so
    `ee.on` / `ee.emit` actually fire JS callbacks
  - `os.cpus` returns Node-shaped array of CPU objects
  - `os.networkInterfaces` works on all Unix (Linux + macOS)
  - `os.uptime/loadavg/totalmem/freemem` via the `sysinfo` crate
    (replaced custom /proc + sysctl + getifaddrs code, -230 lines)
  - `querystring.stringify` emits one kv per array element
  - `path.relative` implemented; `path.posix` + `path.win32`
    exposed as nested namespaces
  - `util.format_template` appends trailing positional args
  - `net.isIP` distinguishes v4 and v6 correctly

- Compat suite: 17 / 17 passing locally on macOS.
