# TOD-160: server retention on keep-alive connections

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`, glibc malloc (the
default build).

- Base `c158`: v2-gc `ed9427dd`.
- Candidate `c160`: this change.
- Hashes are in `binaries.sha256`.

## Harness

- `serve.cjs SCENARIO`: starts a `tests/frameworks` scenario under quench-node.
- `load.cjs BIN SCENARIO TOTAL STEP CONCURRENCY`: drives the server from a separate Node
  process.
  - Request mix: the `driver.cjs` mix (`GET /`, `POST /echo` 2 KB JSON, `GET /stream`,
    `GET /asset.txt`, `GET /missing`).
  - Keep-alive agent.
  - Every STEP requests it samples the server's VmHWM, VmRSS, RssAnon, RssFile and CPU time.
- `load-t.cjs`: adds a 3 s per-request timeout that names a hung request.
- `load-g.cjs`: adds glibc `malloc_stats()` per sample, called through gdb.
- `serve-n.cjs`: stops the server after N requests so profilers write their output.
- `msown.py`: groups massif growth by the first Quench frames on each stack.

## Diagnosis (base)

1. **Hono grows linearly** with no plateau: VmRSS 28.6 → 52.2 MB over 20k requests
   (`soak-base-hono-express-20k.txt`).
2. **glibc in-use bytes grow** 21.1 → 27.9 MB between 2.5k and 10k requests. That is live
   memory, not fragmentation.
3. **The runtime's own census is flat.** Under `profile-memory`, `accounted_runtime_bytes`
   stays at 11.1 MB from 2k to 12k requests; the only accounted growth is shapes, which are
   bounded by compaction. So the managed heap is not the owner.
4. **massif names the owners** (`massif-base-growth-by-owner.txt`, growth over 2.5k
   requests):
   - HTTP response body chunks (`response_write` → `encode_units`): +1.2 MB.
   - Response header pairs: +0.57 MB.
   - A hash map rehash: +0.42 MB.
   - Async contexts made per `nextTick` (`create_context`): +0.27 MB.
5. **Causes:**
   - Server responses were removed from the host table only when their socket closed. A
     keep-alive connection kept every response it served, with headers and written body.
   - Every finished async context kept its `context_triggers` entry forever.

## Change

- **At finish:** a finished response's body leaves the host table with its frame. An ended
  response never writes again.
- **On collection:** each response object is registered with a host `FinalizationRegistry`.
  When the object is collected and the response is terminal, the host entry is removed. An
  unfinished response still lives until its socket closes, as before. `getHeader` after
  `end()` keeps working while the object is reachable.
- **Async contexts:** `clear_context`, the single "context finished" operation, also drops
  the context's trigger entry.

## Result

Peak RSS (VmHWM) at the end of each soak:

| Server | Requests | Base | Candidate |
| -- | --: | --: | --: |
| hono | 20k | 52.2 MB, still growing | 32.3 MB (30.1 at 5k → 32.3 at 20k) |
| koa | 10k | 46.4 MB | 37.6 MB |
| fastify | 10k | 40.9 MB | 30.5 MB (flat after 2.5k) |
| h3 | 10k | 37.1 MB | 32.0 MB |

- Server CPU for hono's 20k requests: 40.7 → 35.9 s. Socket close no longer scans an
  ever-growing response table.
- The residual hono growth is about 0.15 KB per request, still being investigated.
- Upstream Node `test-http*` (773 fixtures, `node-http-tests-diff.txt`):
  - Pass count unchanged at 99.
  - The one changed case, `test-http-server.js`, goes from timeout to fail (both non-pass).
  - Run directly, the base prints `shared HTTP response is no longer active` and the
    candidate does not.

## Not fixed here (filed on TOD-133, web lane)

- Keep-alive latency of about 10 ms per request comes from the 1 ms sleep-poll loop, not
  from CPU.
- Express stops answering `GET /missing` after about 200 keep-alive requests, on base and
  candidate alike, so express has no soak result here.
