# Quench runtime performance experiments

This is a research backlog of 100 unproven experiments for the v2 runtime goal:
at least 20% faster and 20% lower peak RSS than both Node and Bun, measured
separately with JIT disabled and enabled. These are hypotheses, not results.
Run one package and one execution mode at a time. Keep throughput, peak RSS,
output parity, binary hashes, and warm/cold conditions together in each report.

The current gate is React SSR without JIT. Its latest baseline is about 5x slower
than Node/Bun while using less RSS. The latest retained dispatch fusion was only
about 1.1% faster and 1.1% lower RSS. `tests/npm-perf` currently has a timed React
workload; the other packages need representative timed workloads before their
runtime performance can be judged.

## Interpreter, calls, and activation windows

1. Search handler source/layout order against real transition profiles for
   instruction-cache and indirect-branch changes.
2. Replicate only the hottest handlers for common predecessor opcodes, then
   measure branch prediction and code-size effects.
3. Prototype context-threaded handling for frequent branch sequences and
   compare it with the current per-record function-pointer dispatch.
4. Decode a short run of lane records together to amortize dispatch while
   preserving per-opcode side exits.
5. Compare statically selected fusions with dynamically installed fusions under
   the same PGO and code-size budget.
6. Keep hot lane state such as the register base, frame, or current IP in host
   registers across adjacent handlers; inspect spills before keeping a variant.
7. Test pinning one or two selected VM context fields to host registers rather
   than pinning the full interpreter state.
8. Cache the common value tag in the dispatch state and measure tag-check cost
   against register pressure.
9. Add narrowly guarded operand-kind handler variants at hot sites, with a cap
   on variants per function.
10. Feed quickening observations directly into later JIT decisions to avoid
    duplicate profiling and repeated target discovery.
11. Place hot lane records and their cache entries together, then compare cache
    misses and RSS with the current record/cache layout.
12. Arm an adjacent own-field-read pair only after both field caches are warm;
    keep the second handler valid for direct branch entry.
13. Extend primary-own nested field reads from two steps to three or four with
    a fallback at every step.
14. Fuse a field read into a following binary operation when the field value is
    the left operand and the miss path can resume at the field record.
15. Measure the same field/binary fusion when the field value is the right
    operand, using a separate handler only if that relation is common.
16. Redirect a field result into a following Move's final register when the
    intermediate register is dead.
17. Combine a returning Call and following GetField for call classes that keep
    the result in the lane state and can preserve the original call PC.
18. Combine Call and Binary for stable call-entry classes while retaining the
    interpreter result and exception behavior on a side exit.
19. Propagate a LoadCapture directly into its final Move destination when
    liveness, replay, and GC metadata permit it.
20. Fuse a common typed branch predicate with its producer so it need not
    materialize a generic Value before the branch.
21. Select fixed-arity Call handler variants at view construction for arities
    zero through four.
22. Sweep argument-copy widths and branch ladders by observed arity frequency,
    including 0/1/2/3/4/8-value cases.
23. Skip the activation fill when copied arguments initialize every callee
    register; first profile how often `registers == argc` holds.
24. Cache activation metadata such as register count, argument count, and fill
    width when the target is stable.
25. Sweep activation-room rounding separately from fill-store count and track
    vector growth and peak RSS.
26. Generate per-function read-before-write masks and initialize only entry
    registers whose initial `undefined` value is observable.
27. Refine live-in masks by value kind so non-reference slots can avoid GC-root
    initialization when the collector can prove them safe.
28. Borrow the caller's outgoing argument window for same-program calls when
    liveness proves the caller will not mutate the borrowed values.
29. Reuse a dead caller register window as a callee prefix for call sites with
    non-overlapping live ranges.
30. Reuse the current activation for eligible synchronous tail calls, checking
    returns, exceptions, constructors, and cross-program boundaries.
31. Add a monomorphic target cache for stable call sites with explicit
    invalidation and a measured cache-hit threshold.
32. Try a bounded two- or four-target call cache for mildly polymorphic sites;
    fall back before lookup cost exceeds the generic path.
33. Memoize the resolved call-entry class at a lane site and measure whether it
    removes a repeated branch without enlarging the hot handler.
34. Specialize stable receiver-binding cases (primitive, object, lexical-this,
    or strict) after a receiver-kind census.
35. Inline only tiny, monomorphic callees in the no-JIT lane under a strict
    bytecode expansion limit.
36. Color promoted locals into reusable registers when their live ranges do not
    overlap, pinning parameters and ABI windows.
37. Keep promoted locals in frame storage when a direct local access costs less
    than a larger register window and its activation stores.
38. Scalar-replace a non-escaping arguments object when only fixed indexed
    arguments are observed.
39. Pre-size activation metadata from measured call-depth distributions instead
    of reserving for worst-case depth.

## JIT, compilation, and code layout

40. Add a small baseline native tier for hot unsupported functions before the
    current optimizing compiler, prioritizing React's object-heavy functions.
41. Prototype copy-and-patch baseline code generation for common bytecodes to
    reduce first-tier compile cost.
42. Trigger OSR at a hot loop header rather than waiting for another function
    entry, starting with one loop shape and bounded compile work.
43. Compile bounded hot traces with side exits for uncommon values and effects.
44. Add side traces only for exits that repeat enough to repay code size and
    compilation cost.
45. Compress deoptimization snapshots to live or changed values and measure
    reconstruction cost at bailout.
46. Record OSR exits by site and reason; change tiering policy when one guard
    repeatedly exits instead of recompiling the same failure.
47. Choose compilation using expected future time saved, code size, and compile
    cost rather than invocation count alone.
48. Split cold guards, exceptions, and rare calls out of generated hot blocks.
49. Outline interpreter slow paths so common handler bodies stay short and
    instruction-cache friendly.
50. Share structured feedback between the lane and JIT for field, call, and
    numeric checks.
51. Reuse proven live-range analysis to allocate native temporaries and reduce
    register spills in compiled loops.
52. Cache frame reconstruction maps for common bailout sites to reduce
    deoptimization bookkeeping.
53. Add scalar replacement for small non-escaping object literals, beginning
    with repeated two-property shapes.
54. Tune generated block ordering from measured hot/cold edge frequencies and
    compare executable size as well as throughput.

## Loading, allocation, and memory

55. Sample allocation lifetimes by hot call site before changing the allocator;
    separate short-lived objects, strings, closures, and host wrappers.
56. Evaluate a thread-local nursery for hot allocation kinds, checking collection
    frequency and peak RSS as well as allocation speed.
57. Use allocation-site slabs for a measured fixed-size cell kind and compare
    fragmentation with the current general allocator.
58. Compare per-kind free lists with eager sweeping for objects, arrays, and
    strings as separate cohorts.
59. Specialize write barriers for measured young-to-young stores only when
    remembered-set and generational invariants prove the shortcut safe.
60. Deduplicate lane register roots using existing liveness/root maps and
    measure collector work items per render.
61. Tune the object dictionary-conversion threshold using property-count,
    deletion, and prototype-use distributions from real workloads.
62. Store compact enumerable-key metadata in object shapes if repeated
    key iteration is hot and mutation invalidation stays cheap.
63. Make dense-array append/iteration retain a fast representation until
    observed holes or descriptors require sparse conversion.
64. Tune array capacity growth from short-array histograms, measuring both copy
    work and unused retained capacity.
65. Release oversized parser/compile arenas after high-water events only above a
    threshold, and compare steady RSS against allocator churn.
66. Cache negative module-resolution results with generation-based invalidation
    so missing optional paths do not trigger repeated filesystem probes.
67. Cache package `exports`/`imports` condition selection by manifest identity
    and active conditions.
68. Cache file metadata for module resolution with invalidation after writes;
    count avoided syscalls and stale-result risk.
69. Intern canonical absolute paths once and use compact IDs in compile-cache
    keys and module graph edges.
70. Reuse parsed ASTs for identical generated/eval source using a bounded
    content-digest cache and account for retained bytes.
71. Select parser arena size classes from source-length histograms and measure
    allocator calls plus retained memory for many short files.
72. Cache validated bytecode metadata in the compile cache while checking the
    source, compiler-version, and feature guards.
73. Expand startup snapshots with measured high-fanout builtins and compare
    cold-start time, image size, and peak RSS.

## Package-specific workload and fast-path experiments

74. React/react-dom: benchmark `renderToPipeableStream` with a nested tree and
    compare chunking, promise callbacks, buffering, and peak RSS.
75. React/react-dom: use a text-heavy tree with escaping, Unicode, and repeated
    long strings to isolate escaping and string allocation.
76. React/react-dom: vary wide fragments and mixed null, boolean, text, element,
    and array children to stress traversal and collection dispatch.
77. Next: add a warm App Router request workload with dynamic params, headers,
    cookies, and JSON output to expose server-side JS overhead.
78. Next: split cold module compilation from repeated server-component route
    requests with the route cache warm.
79. Next: benchmark image metadata and optimization separately, accounting for
    sharp/libvips native time and memory.
80. Vite: repeatedly merge nested aliases, plugin lists, and server config to
    isolate object/array merge overhead.
81. Vite: run `resolveId`/`load`/`transform` callbacks over many synthetic
    modules, including sourcemap strings and plugin crossings.
82. Vite: invalidate one leaf in a large module graph and measure Map/Set
    traversal and allocation.
83. esbuild: compare repeated small TypeScript transforms with syntax options,
    minification, and source maps; isolate wrapper cost from native Go work.
84. esbuild: exercise JavaScript plugin callbacks over many modules to measure
    host/runtime crossings apart from the native parser.
85. esbuild: run incremental context rebuilds with one-file edits and track
    promise overhead, cache reuse, and retained RSS.
86. sharp: reuse one input Buffer through repeated resize/format pipelines to
    separate JS builder churn from libvips processing.
87. sharp: parse metadata from many encoded inputs without decoding pixels to
    isolate wrapper, Buffer, and Promise costs.
88. sharp: bound concurrent resize pipelines and compare queue overhead with
    peak native memory at several image sizes.
89. Fastify: repeatedly inject a request into a precompiled route with a stable
    JSON schema to measure route and serializer overhead.
90. Fastify: compare static and parameterized route lookup after registering a
    large route set.
91. Fastify: measure synchronous and asynchronous pre-validation, pre-handler,
    and on-send hook chains separately.
92. Undici: exercise Headers append/get/delete with duplicate and differently
    cased names over a stable and a freshly created header set.
93. Undici: use a local keep-alive Pool with small and large response bodies to
    isolate request setup, header work, and body draining.
94. Undici: compare reused URL/options with newly created Request objects under
    identical local fetch responses.
95. ws: parse one-byte-fragmented masked UTF-8 frames and larger fragmented
    payloads to measure parser state and Buffer-copy costs.
96. ws: sweep frame lengths around 125/126 and 65535/65536 bytes for masking,
    framing, and copy thresholds.
97. Pino: compare disabled debug calls with enabled logging and deep object
    arguments to measure guard laziness and formatting work.
98. Pino: vary serializers, mixins, bindings, and redaction in JSON formatting;
    measure output construction and retained memory.
99. Zod: benchmark flat successful object parsing under strict, strip, and
    passthrough policies with stable input shapes.
100. Zod: compare invalid nested `safeParse` issue construction against
     discriminated unions with early and late matches.

## Sources and current evidence

The ideas were cross-checked against the current React performance notes and
Linear issue TOD-173. Relevant local anchors include
[`tests/npm-perf/README.md`](../tests/npm-perf/README.md),
[`dispatch_fast.rs`](../crates/quench-runtime/src/vm/dispatch_fast.rs),
[`frame_registers.rs`](../crates/quench-runtime/src/compile/frame_registers.rs),
and [`lane_exits.rs`](../crates/quench-runtime/src/vm/lane_exits.rs).
Online references used for candidate mechanisms include [Ertl and Gregg on
interpreter dispatch](https://doi.org/10.1145/1286821.1286828), [PEP 659
quickening](https://peps.python.org/pep-0659/), [Firefox Warp and CacheIR](https://hacks.mozilla.org/2020/11/warp-improved-js-performance-in-firefox-83/),
[V8 Sparkplug](https://v8.dev/blog/sparkplug), [copy-and-patch](https://compilers.stanford.edu/software/copy-and-patch/),
[JavaScriptCore speculation](https://webkit.org/blog/10308/speculation-in-javascriptcore/),
[LuaJIT traces](https://luajit.org/ext_jit.html), and [V8 startup snapshots](https://v8.dev/blog/custom-startup-snapshots).
