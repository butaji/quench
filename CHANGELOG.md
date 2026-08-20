# Changelog

## 2026-08 — Node API host slice

- `quench-node` is a pure-Rust Node API compatibility host built
  on top of `quench-runtime`. Its supported surface is defined by the
  accepted v1 scope in `docs/adr/0002-quench-node-scope.md`.

- `quench-node-test` owns the compat suite at
  `crates/quench-node-test/node-tests/` and a runner that classifies
  each script as Pass / Fail / Skip.

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

- The compat suite has expanded since this entry was written; current
  script counts and outcomes are intentionally not tracked in the changelog.
