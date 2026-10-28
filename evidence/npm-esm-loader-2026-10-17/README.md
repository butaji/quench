# ESM loader errors, CommonJS export recognizers and EventEmitter shape, 2026-10-17

Host: Linux 6.18.44 x86_64 (cloud VM), 4 vCPU Intel Xeon @ 2.10GHz, 15 GiB RAM.
Oracle: official Node v26.12.0. Source: f65e7b80d.

| run | behavior | declarations | Quench failures | oracle failures |
| --- | --- | --- | --- | --- |
| `scenarios.json.gz` | 286 | 13 | 0 | 1 |

| surface run | matching-unqualified | differences | oracle failures |
| --- | --- | --- | --- |
| `surface-before.json.gz` (012e00e96) | 240 | 51 | 9 |
| `surface.json.gz` (f65e7b80d) | 267 | 24 | 9 |

The surface gains come from four fixes:
- EventEmitter's public shape now matches Node. This removed differences in 20 packages
  that expose emitters.
- CommonJS named exports through `Object.defineProperty`, TypeScript `__exportStar` and
  Babel re-exports (typescript unstable API).
- Regex literal classes containing `(?-i)` (date-fns locales).
- Node's ESM loader error codes, constructors and messages, including import attribute
  checks.

These are surface comparisons and representative scenarios, not all-functions
qualification. The remaining differences are led by Buffer's hand-written prototype
(names, lengths, attributes).

`quench-node-test` on f65e7b80d: node_contracts 5/5 (now with esm/loader_contract.mjs),
worker_threads, napi_addons and shared_regressions all pass. child_entry has 458 passed
and 13 failed; all 13 are the failures already listed in
evidence/npm-serializer-merge-2026-10-17.
