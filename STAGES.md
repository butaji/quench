# Compatibility stages

A stage is a semantic planning boundary, not a progress claim or fixture
workaround. Use local Node tests and CLI as oracle; declare reusable API facts
once, generate mechanics from them, and handwrite only observable algorithms
and host adapters. `Proven`, `Guarded`, and `Unknown` are distinct: unknown
behavior uses complete semantics or fails visibly.

Claim completion only from a clean, reproducible verification with no
unexplained failure or timeout. Compare values, descriptors, identity,
ordering, errors, exit status, and host effects. The VM remains the only JS
semantics; the host never recognizes benchmark or fixture identity.

## Families

1. Measurement and runner truth
2. Runtime and value semantics
3. Core data APIs
4. Events, scheduling, and async context
5. Filesystem, VFS, and modules
6. Streams and backpressure
7. Network and protocols
8. Process, workers, and clusters
9. Crypto, security, and policy
10. Observability and performance APIs
11. Web and special modules
12. Full-suite closure and reduction

## Execution stages

### 0. Measurement and runner truth
### 1. Runtime and value semantics
### 2. Core data APIs
### 3. Events, scheduling, and async context
### 4. Filesystem, VFS, and modules
### 5. Streams and backpressure
### 6. Network and protocols
### 7. Process, workers, and clusters
### 8. Crypto, security, and policy
### 9. Observability and performance APIs
### 10. Web and special modules
### 11. Full-suite closure and reduction

## Evidence gates

`run-parallel` has two deliberately different gates:

- The checked-in manifest (`cargo run -p quench-node-test --bin run-parallel`)
  is the reproducible regression gate. It currently contains 695 fixtures.
- The recursive inventory (`... run-parallel -- --all`) is the coverage gate.
  It discovers every `test-*.js`, `test-*.mjs`, and `test-*.cjs` under
  `tests/node/test/parallel` (currently 4,235 files), isolates each fixture in
  a process group, and records pass/skip/fail/timeout/crash/unclassified plus
  an inventory hash. A filtered run is diagnostic; it never changes the
  manifest or implies full-suite completion.

Every family advancement must therefore record both the focused inventory
result and the manifest result. A timeout remains an unresolved runtime or
fixture-capability fact; it is not converted into a pass by omitting the
fixture. The `tests/node` submodule is the upstream oracle and is intentionally
not staged by compatibility commits.

## Current evidence

After the Rust TLS validation surface, self-reexec argv boundary, Blob URL
registry, shared TTY helper, receiver-sensitive `process.binding('util')`,
internal encoding/CIDR/SAB capabilities, and fixture `execArgv` boundary, the
latest manifest gate reports **591 pass, 98 fail, 3 timeout** of 692 fixtures.
The count is a point-in-time measurement because unsupported and timing-
sensitive fixtures remain variable. TLS/HTTPS failures are encrypted
transport effects, not validation gaps; they remain explicit until a bounded
Rust transport capability exists.

The focused util inventory after these changes is **40 pass, 13 fail, 1
timeout** of 54 fixtures. Filtered results guide family selection but do not
replace the manifest gate above.

The focused timers inventory after deriving the AbortSignal timeout callback
ID from the registry is **67 pass, 0 fail, 1 timeout** of 68 fixtures. The
scheduler promise family now passes; the remaining timeout is the worker
termination fixture. The scheduler fix keeps the harness call-check list as
plain indexed data, avoiding an observable `Array.prototype.push` call across
an already-rejected async continuation.
