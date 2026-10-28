# RegExp `--jit`: where Quench's time goes (Linux, 2026-10-24)

Build: PGO `profiling` build of `df7e05eea9` on `v2-cloud-c`. This was one
callgrind run of `mat-regexp.js` with `--jit`, 7.88 G instructions in total. The
host is the same 4-vCPU Xeon VM as the V8-v7 boards.

In the 3-pair JIT screen of `evidence/jit-navier-stokes-codegen-linux-2026-10-24/`,
Node is 1.59× and Bun 4.14× ahead of Quench `--jit`. A 1.2× lead over Node
therefore needs Quench to run about 1.9× faster than it does now.

## Exclusive instruction share by component

| Component | Share | Main functions |
|---|---:|---|
| Matcher | ~36% | `backtrack::Program::run` 16.9%, `Regex::find_in_subject` 5.2%, `RangeSearcher::find` 4.7%, `memchr` 4.5%, `scan_start_bytes` 2.4%, `Prefix::find_folded` 1.2%, assertions 1.0% |
| Per-call glue | ~35% | `lane_regexp_call` 12.3% (234 instructions per call over 4.16 M calls; `steady_regexp_call` and its checks are inlined), `regexp_symbol_replace_fast` 3.9%, `regexp_match_from` 3.9%, `lane_native_call` and `lane_native_value` 5.7%, legacy match recording 2.2%, receiver, layout and symbol-method proofs 4.3%, `lastIndex` stores 0.7% |
| Interpreted JavaScript, allocation and GC | ~25% | field reads, captures, indexing, `alloc_cell`, sweep |

## What a 1.2× JIT-mode win would take

`--jit` does not change any of these components, because RegExp's hot code is
regex natives and short interpreted glue, not numeric loops.
- Removing all of the glue gives at most about 1.5×.
- Compiling regexes to native code cuts the matcher's share, but V8 already does
  that (Irregexp). Matching Node's matcher therefore only closes part of the gap.
- The interpreted remainder needs the JS function tier that TOD-110 assigns to
  the JIT lanes.

Reaching 1.9× needs all three, so the RegExp JIT-mode win is not reachable
through the glue alone.

## Follow-up: per-call cost and what the components can deliver

- **`exec` microbenchmark** (`rx-each.js` shape): Quench `--jit` is 3–6× slower
  than Node on every pattern, including the literal `/search/` (240 ns against
  75 ns per call).
  - An `exec` that builds its result costs about 2,300 instructions. Building the
    result is ~800, glue plus matching ~890, the GC its allocations trigger ~130,
    and the interpreter ~400.
  - The result layout lookup hashes the realm's globals on every call.
  - Building the result elements in place instead of through a temporary `Vec`
    saved nothing measurable (2,966 → 2,963 instructions per call), so it was
    not kept.
- **Fixture call mix:**
  - About 2.2 M of the 4.16 M regex calls end in the prefilter. Before it, the
    steady path runs about 130 instructions of receiver, `lastIndex` and
    method-cache checks.
  - Few exec results are built; most calls are `test`, `exec` whose result is
    unused, or `replace`.
- **Matcher:** only `backtrack::Program::run` (17%) would gain from native
  compilation. The prefix, range and `memchr` searches (about 14%) are already
  SIMD code.

Upper bounds:

| Change | Share it removes | Overall gain |
|---|---:|---:|
| `Program::run` compiled 3× faster | ~11% | ~1.13× |
| Glue halved | ~17% | ~1.2× |
| Both together | | ~1.36× |

Leading Node by 1.2× in `--jit` needs about 1.9×. The rest would have to come
from the interpreted JavaScript, which only a JS function tier can speed up.
