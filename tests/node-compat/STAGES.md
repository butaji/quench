# Node compatibility stages

Each stage is a closed, runnable gate. A stage may be marked complete only
when every JavaScript file in its directory passes; the harness never silently
skips a file. The Node submodule is the upstream compatibility corpus, while
these small fixtures isolate the currently implemented contract.

| Stage | Scope | Gate |
|---:|---|---|
| 0 | runtime globals and `Buffer` | `tools/run-node-tests.sh --stage 0` |
| 1 | initial CommonJS built-in shims | `tools/run-node-tests.sh --stage 1` |
| 2 | filesystem and path host bindings | `tools/run-node-tests.sh --stage 2` |
| 3 | synchronous file read/write/stat | `tools/run-node-tests.sh --stage 3` |
| 4 | timers and process event listeners | `tools/run-node-tests.sh --stage 4` |
| 5 | `events.EventEmitter` | `tools/run-node-tests.sh --stage 5` |
| 6 | `os` and `util` modules | `tools/run-node-tests.sh --stage 6` |
| 7 | `querystring` module | `tools/run-node-tests.sh --stage 7` |
| 8 | basic `URL` and `URLSearchParams` | `tools/run-node-tests.sh --stage 8` |
| 9 | URL authority and serialization | `tools/run-node-tests.sh --stage 9` |
| 10 | asynchronous `fs` callbacks and promises | `tools/run-node-tests.sh --stage 10` |
| 11 | minimal `stream` readable/writable flow | `tools/run-node-tests.sh --stage 11` |
| 12 | expanded `assert` contract | `tools/run-node-tests.sh --stage 12` |
| 13 | `timers` and `timers/promises` modules | `tools/run-node-tests.sh --stage 13` |
| 14 | process cwd and environment lookup | `tools/run-node-tests.sh --stage 14` |
| 15 | path normalization and decomposition | `tools/run-node-tests.sh --stage 15` |
| 16 | Node `url` module helpers | `tools/run-node-tests.sh --stage 16` |
| 17 | Rust-backed `crypto.createHash('sha256')` | `tools/run-node-tests.sh --stage 17` |
| 18 | Rust-backed `crypto.randomUUID()` | `tools/run-node-tests.sh --stage 18` |
| 19 | Buffer composition and base64 | `tools/run-node-tests.sh --stage 19` |
| 20 | host-backed console output | `tools/run-node-tests.sh --stage 20` |
| 21 | asynchronous assertion helpers | `tools/run-node-tests.sh --stage 21` |
| 22 | promise EventEmitter helpers | `tools/run-node-tests.sh --stage 22` |
| 23 | synchronous directory filesystem APIs | `tools/run-node-tests.sh --stage 23` |
| 24 | Rust-backed filesystem stat kinds | `tools/run-node-tests.sh --stage 24` |
| 25 | synchronous file mutation APIs | `tools/run-node-tests.sh --stage 25` |
| 26 | append and access filesystem APIs | `tools/run-node-tests.sh --stage 26` |
| 27 | filesystem realpath and rm aliases | `tools/run-node-tests.sh --stage 27` |
| 28 | promise directory filesystem APIs | `tools/run-node-tests.sh --stage 28` |
| 29 | POSIX `fs.chmodSync` | `tools/run-node-tests.sh --stage 29` |
| 30 | POSIX symbolic link APIs | `tools/run-node-tests.sh --stage 30` |
| 31 | filesystem and OS constants | `tools/run-node-tests.sh --stage 31` |
| 32 | harness no-empty-stage contract | `tools/run-node-tests.sh --stage 32` |
| 33 | upstream `common/tmpdir` helper | `tools/run-node-tests.sh --stage 33` |
| 34 | UTF-8 `TextEncoder` compatibility | `tools/run-node-tests.sh --stage 34` |
| 35 | upstream fs path coercion and async mkdtemp | `tools/run-node-tests.sh --stage 35` |
| 36 | Node-compatible six-character mkdtemp suffix | `tools/run-node-tests.sh --stage 36` |
| 37 | unique repeated `mkdtemp` prefixes | `tools/run-node-tests.sh --stage 37` |
| 38 | UTF-8 Buffer string round trips | `tools/run-node-tests.sh --stage 38` |
| 39 | host-backed `process.chdir` | `tools/run-node-tests.sh --stage 39` |
| 40 | POSIX `process.umask` | `tools/run-node-tests.sh --stage 40` |
| 41 | Rust-backed process high-resolution time | `tools/run-node-tests.sh --stage 41` |
| 42 | Rust-backed process identity | `tools/run-node-tests.sh --stage 42` |
| 43 | Rust-derived process platform and arch | `tools/run-node-tests.sh --stage 43` |
| 44 | Node process version metadata | `tools/run-node-tests.sh --stage 44` |
| 45 | process feature/config metadata | `tools/run-node-tests.sh --stage 45` |
| 46 | POSIX process user/group identity | `tools/run-node-tests.sh --stage 46` |
| 47 | Rust-derived OS host values | `tools/run-node-tests.sh --stage 47` |
| 48 | host-derived `os.cpus()` | `tools/run-node-tests.sh --stage 48` |
| 49 | harness `.js` and `.mjs` coverage | `tools/run-node-tests.sh --stage 49` |
| 50 | console timing APIs | `tools/run-node-tests.sh --stage 50` |
| 51 | console counters and clear | `tools/run-node-tests.sh --stage 51` |
| 52 | `assert.strict` namespace | `tools/run-node-tests.sh --stage 52` |
| 53 | upstream `common.mustCall` helpers | `tools/run-node-tests.sh --stage 53` |
| 54 | exact callback-count enforcement | `tools/run-node-tests.sh --stage 54` |
