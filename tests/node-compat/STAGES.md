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
