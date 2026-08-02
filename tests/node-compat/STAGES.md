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
