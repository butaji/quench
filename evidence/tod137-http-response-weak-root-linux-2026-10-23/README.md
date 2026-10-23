# Keep-alive HTTP servers leaked one weak root per response — fixed

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. All binaries are static `quench-node`
production builds, unbolted.

| Side | Source |
| -- | -- |
| Base | v2-gc `644f6a2353` (`qf1.static`) |
| Fix | v2-gc `644f6a2353` plus this change, built with `quench-runtime/gc-tuning` for `QUENCH_GC_TRACE` |

## Cause

Two lanes fixed keep-alive response retention independently. Trunk merged both:

- TOD-160 (`435710adf2`) registers each response object in a `FinalizationRegistry`.
  Its callback, `response_release`, removes the ended record.
- `4edde0f0b4` gives each record an owner weak root (`host_weak_root`, a rooted
  `WeakRef` cell). It retires the record once a collection proves the owner dead
  (`poll::prune_collected_responses`), and releases the weak root as it does.

The finalization callback runs first and removes the record. Its weak root is therefore never
released, and neither is its request's async context. Every response served on a keep-alive
connection left behind one `WeakRef` cell and one root-table entry.

## Finding it

- With `QUENCH_GC_TRACE`, live weight after each full collection grows with requests
  served: 24.2K → 71.2K over 100K requests on 16 connections (`honoload-c.js`).
- The same happens with a server that only answers `res.end('Hi')`, driven by Node 22 from
  another process (`srv-only.js`, `cli-only.js`). So the leak is in the server, not
  the in-process client.
- The `profile-memory` census at exit, 20K → 60K requests, shows only `weak_ref` cells
  growing: 19,595 → 58,754. Objects grow 2,303 → 2,342. The HTTP response table stays
  at 0–2 records.
- A backtrace at every 1000th `host_weak_root` names
  `http_shared_vm::operations::response_initialize` as the creator.

## Change

`response_release` now retires the record the way `poll::retire_response` does: it
releases the owner's weak root and retires the request's async context.

## Results

Live weight after full collections (`QUENCH_GC_TRACE`, fix build):

| Load | Base | Fix |
| -- | -- | -- |
| Bare server, 1 connection, 20K / 60K requests | 36.3K / 75.1K | 19.9K / 19.9K |
| `honoload-c.js` 100K requests, 16 connections | 24.2K → 71.2K | 22.3K–23.0K, flat |

Hono-style server, 300K requests on 64 connections (`honoload-c.js 300000 64`), 3 runs each,
alternating:

| | Base | Fix |
| -- | -- | -- |
| Peak RSS (KiB) | 34,680 / 30,568 / 34,776 | 19,428 / 19,832 / 19,468 |
| Wall (s) | 50.7 / 49.9 / 50.1 | 48.8 / 47.9 / 48.4 |

Callgrind, `honoload.js` (one connection). Collection instructions per request between
6K and 20K requests: base 19.2K, fix 18.2K. The saving grows with run length, because the
base's live set grows with every request served.

## Pacing frequency is not the cause

The web lane reported +14.5K `collect_with_object_roots` instructions per hono request after
the trunk merge. Trunk's Throughput table does collect young twice as often as the web
table at this live size (2^16 vs 2^17 allocations). But in this model, halving the
frequency (`QUENCH_GC_MIN_YOUNG=131072`, with full minimums 4×) saves only 0.8–1.5K
instructions per request. Most collection cost is sweeping dead young cells, which scales
with garbage, not with collection count. Pre-merge v2-gc (`3d7dc0656a`) spent 19.6K per
request against 19.8K post-merge in this one-connection model.

## Node behavior

The 400 upstream `tests/node/test/parallel/test-http-*.js` cases ran on base and fix. Each
was copied to `.cjs` next to symlinks to `common/` and `fixtures/`, with a 20 s deadline.
Exit statuses are identical case for case: 92 pass, 268 fail, 39 time out, 1 exits 7, on
both binaries. The ad-hoc harness is not the Stage A runner, so the failures are not
regressions; the point is that the change moves no case.
