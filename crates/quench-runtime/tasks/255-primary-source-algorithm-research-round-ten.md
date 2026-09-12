# 255 — Primary-source algorithm research, round ten

Status: complete

Research additional algorithms that fit the standing execution contract: every function
runs a stencil image on first execution; rustc/LLVM cooks a finite, general catalog AOT;
runtime generation is copy, patch, and bounded semantic-case repair; no interpreter,
hotness threshold, benchmark identity, or exact source spelling selects code.

## Result

One new near-term transformation survived deduplication: guest-level loop-idiom
recognition (Task 256). One later memory-layout experiment is distinct enough to track
(Task 257). The other useful findings refine existing owners instead of creating parallel
subsystems.

### 1. Recognize whole guest loops as semantic idioms (Task 256)

LLVM's `LoopIdiomRecognize` replaces proved loop recurrences with `memset`, `memcpy`,
`memmove`, `strlen`, popcount/find-first-set, polynomial-hash, and CRC operations. The
important pattern is not the specific intrinsic list: a loop is quoted data, analysis
proves a known denotation, and one rewrite replaces the whole trace before machine-code
emission.

That is directly applicable one level above LLVM. JavaScript dense-array and string
loops do not reach LLVM as one loop today, so LLVM cannot see their recurrence through
individually cooked stencil leaves. Task 256 adds one canonical `LoopIdiom` sum derived
from Task 171's region graph. Each recognized idiom lowers to either an immutable typed
kernel or a copy-patched stencil instance with the same input/output context.

The V8v7 corpus contains real general examples, including dense zero-fill loops and the
classic `x &= x - 1` popcount recurrence in Crypto. Those examples motivate coverage;
the selector matches data/effect/alias facts, never filenames or source positions.

### 2. Distinguish loop vectorization from basic-block SLP (Task 39 refinement)

LLVM maintains two separate vectorizers. The Loop Vectorizer widens consecutive loop
iterations; SLP finds isomorphic independent scalar trees within or across basic blocks.
Task 39 now explicitly owns both derived stencil forms. This matters for small fixed
numeric objects and unrolled arithmetic where there is no profitable counted loop to
widen.

### 3. Hoist or clone loop-invariant semantic branches (Task 196 confirmation)

LLVM's loop-unswitch pass hoists invariant conditions and may clone a sufficiently small
loop. Task 196 already owns the correct stencil adaptation: version the traced loop on a
shape, representation, protector, or callee invariant; keep the proof in the preheader;
emit a checked general loop beside a guard-free steady-state loop. No duplicate task is
needed.

### 4. Use matcher quick checks only after profiling (Task 87 refinement)

V8's RegExp compiler derives bounded lookahead and Boyer–Moore-style skip instructions,
and preloads characters for quick rejection before the full alternative. Task 87 now
records this as the first native-matcher experiment if post-Task-142 profiling shows that
matching itself dominates. The current host `regex` engine already performs automata and
literal acceleration, so adding a second prefilter without such a profile would likely
duplicate work.

### 5. Compress only representation-proved heap references (Task 257)

V8 stores compressed tagged values as 32-bit offsets inside a 4-GiB cage, pins the cage
base in an already-reserved root register, and eliminates redundant compression/
decompression operations. It reports large heap-size reductions and some CPU/GC gains.

This VM's NaN-boxed `Value` stores doubles directly, so blindly replacing every 64-bit
slot with V8's 32-bit format would either lose direct doubles or re-box them. Task 257 is
therefore narrower: compress pointer-only fields and shape-proved reference slots after
the stable VM heap and typed field representations exist. Values remain compressed
through load/store chains and expand only at dereference or canonical ABI boundaries.

### 6. Coalesce write barriers as an effect rewrite (Task 162 refinement)

JavaScriptCore uses a header-resident cell-state bit for a one-load barrier fast path and
coalesces barriers for repeated writes to the same object. Task 162 now requires the
smallest single-threaded form: omit barriers for new destinations and non-heap values;
coalesce repeated stores to one base until an effect boundary; emit at most one
remembered-set action. Task 173's MemorySSA supplies the proof, so this is not a second
GC subsystem.

### 7. Cluster immutable roots only where it removes a real test (Task 198 refinement)

V8's static roots place immutable objects at deterministic cage-relative addresses and
cluster related maps so type predicates become address-range tests. Task 198 now includes
optional root clustering, but only if disassembly shows it replaces multiple shape/root
comparisons. This VM already has distinct NaN-box tags for strings, objects, functions,
and regexps, so clustering must not duplicate an existing single tag comparison.

## Negative findings

- Do not retry a uniform `InlineSite` shrink as a performance project. Task 116's exact
  A/B was neutral, and JavaScriptCore reports that an out-of-line metadata table can add
  a costly load chain. Typed side tables are justified by measured hot metadata or
  memory footprint, not by compactness alone.
- Do not add a second RegExp prefilter in front of the existing host engine without a
  matcher-only profile. V8's quick checks are useful inside its one compiler pipeline;
  stacking unrelated engines is not the same optimization.
- Do not treat pointer compression as an immediate score fix. It depends on Tasks 148,
  162, 165, 171, 173, and 203, and it must preserve the current direct-double advantage.
- Do not reassociate JavaScript floating-point reductions under the SLP/scan work. JS
  observes IEEE-754 rounding; only exact integer/bitwise or otherwise proven associative
  operators may use parallel scan or reassociation.

## Priority

This research does not displace the measured critical path:

`185 -> 146/181/203 -> 149/144/171 -> 256 -> 39 -> 148/162 -> 257`

Task 185 covers the highest recorded single residual shape. Direct guest calls and
register-resident typed regions remove broader helper/frame boundaries. Loop idioms then
replace complete proven traces rather than adding more five-op fragments. Heap/reference
layout work follows only after the execution boundary is substantially narrower.

## Primary sources

- LLVM loop-idiom recognizer:
  <https://llvm.org/doxygen/LoopIdiomRecognize_8cpp.html>
- LLVM Loop Vectorizer and SLP Vectorizer:
  <https://llvm.org/docs/Vectorizers.html>
- LLVM loop unswitch implementation:
  <https://llvm.org/docs/doxygen/SimpleLoopUnswitch_8cpp.html>
- V8 pointer compression and decompression elimination:
  <https://v8.dev/blog/pointer-compression>
- V8 static roots and range-based map tests:
  <https://v8.dev/blog/static-roots>
- V8 RegExp quick checks and Boyer–Moore lookahead:
  <https://chromium.googlesource.com/v8/v8/+/c9b71fac463dacde93fcc5ad77f56ba6ad7eeae6/src/regexp/regexp-compiler.cc>
- JavaScriptCore cell-state barrier and barrier coalescing:
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>
- JavaScriptCore's compact bytecode/typed metadata tradeoff:
  <https://webkit.org/blog/9329/a-new-bytecode-format-for-javascriptcore/>

Local corpus evidence:
`v8-v7/crypto.js:423,437,899,1034,1049` in the configured benchmark checkout.
