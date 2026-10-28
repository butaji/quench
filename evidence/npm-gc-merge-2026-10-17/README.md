# npm top 300 on the generational collector (2026-10-17)

v2-merged at be19d2b9b failed every top-300 npm scenario. This checkpoint
merges it into v2-npm and fixes the causes. Measured on 41baa502c, v2-npm with
v2-merged 4c401bda6 merged in (release `quench-node`, sha256 `0c97d99cedcfacf4…`) against Node v26.12.0 and Bun 1.4.2.
Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup limits).

## What broke and the fixes

1. **Shared property storage (09607b664).** `Vm::copy_module_namespace` copied a
   namespace's `ValueVec` handle into another object, so two live objects owned
   one property-arena range. The sticky-mark minor collections of 3ec6b65dc sweep
   the young source while the old target lives. The sweep released the range,
   the arena reused it, and the target read other objects' values. For example,
   `import 'node:child_process'; import 'node:fs'` ran `_forkChild` for
   `SafeMap.prototype.get`.
   - Found by bisecting v2-merged's first-parent history to 49f368e0f, then
     quarantining slot, record and arena reuse in a scratch build. Only arena
     quarantine fixed it. A release check showed dead young record 227 freeing
     range 5656, which live old record 224 still used.
   - Fix: `ValueArena::duplicate` and `Heap::copy_property_storage` give the
     target its own copy of the storage.
2. **Duplicate host operation names (458f81d1d).** `workerRef` and
   `workerTerminate` were registered by both Worker implementations.
   `operation()` took the first match, so `worker.ref()`/`unref()`/`terminate()`
   threw "Invalid Worker receiver" (esbuild's sync API). Fix: renamed the
   operations; `operation_names_are_unique` guards against this.
3. **`serialization: 'advanced'` IPC (458f81d1d)** used Node's json codec, so a
   Map, BigInt or cycle failed (vitest's fork pool). Fix:
   `internalBinding('ipc_serdes')`, Node's unchanged serialization.js, and the
   channel's real mode passed to `setupChannel`/`_forkChild`.
4. **Also fixed:**
   - `require.resolve(request, { paths })`, `require.resolve.paths()` and
     `Module.globalPaths`/`NODE_PATH` now behave as Node.
   - `process.memoryUsage()` now reports real `heapTotal`, `heapUsed`,
     `external` and `arrayBuffers` (606778ee4); Node's
     `test-memory-usage.js` passes.

## Result (`npm-scenarios.json.gz`, `rss-report.txt`)

All 300 pass: 286 behavior, 13 declarations, 1 oracle-failed (the musl rollup
addon, which glibc Node cannot load either).

| median peak RSS | Quench | Node | Bun |
| --- | --- | --- | --- |
| this checkpoint | 23.3 MiB | 72.0 MiB | 36.4 MiB |
| 54aeec881 (`../npm-rss-2026-10-17`) | 27.5 MiB | 71.9 MiB | 36.3 MiB |

Quench has the lowest peak on 264 of 300 packages, up from 252, and is not lowest on 36.

How to read the 36:

- **Bun ran with `BUN_OPTIONS=--smol`**, which this environment's shell exports.
  That is Bun's low-memory mode, a stricter target than default Bun
  (`default-bun.txt`).
- **Eight are decided by the same native process for every engine.** TypeScript 7's
  `bin/tsc` is a JS shim that `execve`s the Go compiler, and `wait4` reports the
  larger of the shim's and the compiler's peaks. The declaration checks
  (`@types/*`, `type-fest`, `undici-types`, `@eslint/core`) and `typescript`
  therefore sit within 0.1–1 MiB of Bun on all engines: run-to-run noise in that
  Go process, not engine memory.
- **The rest are code-heavy:**
  - typescript-eslint and @typescript-eslint/* at 1.1–2.3× Bun.
  - playwright 230 vs 101 MiB; jsdom 201 vs 98; @testing-library/dom 223 vs 120.
  - babel at about 1.2× Bun; lucide-react, jiti, undici, rxjs.
  - In these, compiled-program memory dominates, not the JS heap. After
    `require('typescript')` the heap holds 19 MiB of a 99 MiB resident set.
    Quench compiles every function body eagerly, and each compiled function's
    plan is about 1.2 KB before its code.
