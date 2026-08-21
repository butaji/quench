# Yellow Node API coverage

Target Bun-documented partial behavior for diagnostics_channel, https, async_hooks, child_process, cluster, crypto, domain, module, perf_hooks, process, util, tls, v8, vm, wasi, worker_threads, inspector, repl, node:test.

Document every supported API and every intentional Bun-matching gap. Implement reachable behavior with real errors/backends; no shape-only claims. Enforce file <=500 lines, function <=40 lines, complexity <=10. Add focused fixtures and run applicable upstream tests.
## Status (2026-08-21)

Implemented with focused fixtures and verified: `child_process` (spawnSync real, exec/exec/execFileSync real `sh -c`/direct execution, execFile; fork absent shape-only), `cluster` (single-process primary with fork/disconnect/send/online events), `domain` (create/enter/exit/run/addEmitter/remove), `async_hooks` (createHook/execution ids/AsyncLocalStorage run/getStore), `diagnostics_channel` (channel subscribe/publish/unsubscribe routed), `crypto` (real SHA-1/SHA-256/MD5/SHA-224/SHA-384/SHA-512 pure-Rust digests, getHashes, crypto.subtle.digest/importKey, getRandomValues), `util` (format/inspect + promisify/callbackify/types/deprecate/debuglog + isDeepStrictEqual/styleText/formatWithOptions/stripVT/inherits/getCallSites), `vm` (Script/compileFunction/createContext/isContext/runInThisContext), `v8` (Serializer/Deserializer, positive heap stats), `dns` (callback lookup/resolve4 + promises routed), `node:test` (async/subtest/todo/only/run/hooks/summary), `perf_hooks` (createHistogram/monitorEventLoopDelay/timerify/constants), `net` (getConnections/ref/unref/byte counters), `dgram` (TTL/broadcast/multicast/membership/queue/ref).

Known Bun-matching gaps (no shape-only claims): `tls` empty namespace, `https` request/get only, `wasm` skeletal, `inspector`/`repl` minimal Session/start, `fork` IPC object shape only, sha3-256 currently aliases sha256 backend. Verified: run-compat 57/57, run-parallel 178/178, workspace 76.
