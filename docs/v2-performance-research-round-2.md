# Additional performance research for TOD-173

This is a curated follow-up to the original 100-hypothesis backlog in
[`v2-performance-experiments.md`](v2-performance-experiments.md). The proposals
below were collected from package-specific online research, repo inspection,
and Linear TOD-173 review. They are hypotheses, not measured wins. I excluded
the obvious duplicates of the first backlog and the React experiments Linear
already records as rejected. Each experiment still needs matched output,
throughput, and max-RSS evidence in both JIT modes before it can count toward
the goal.

## Zod

1. Compile a schema graph into a compact validation plan at schema construction; compare repeated parse cost after separating one-time setup.
2. Interpret compiled schemas with an explicit reusable stack instead of allocating per-field closures and result wrappers; measure nested objects and arrays.
3. Allocate an issue list only on the first validation failure; compare successful object, array, and union parses.
4. Reuse internal parse frames between sequential children when results cannot escape; measure allocation volume and recursion fallback frequency.
5. Mark synchronous schema subgraphs and bypass Promise checks there; compare `parse`, `safeParse`, and scalar arrays.
6. Share immutable default parse context and allocate a per-call context only when caller options require one.
7. Cache stable-shape field access plans with accessor, proxy, and shape-change fallbacks; compare flat and nested objects.
8. Fuse field read, child validation, and output assignment in one schema-plan operation; profile successful and failing branches separately.
9. Cache a presence-check plus value-read pair for stable object fields while preserving inherited keys and getters.
10. Precompute optional, defaulted, and input/output field policy during object-schema construction.
11. Specialize homogeneous dense arrays while retaining holes, mutation, and element-error semantics.
12. Run array child validation in a tight loop with the child schema and bounds hoisted; sweep short and long arrays.
13. Precompute primitive enum/literal membership tables and preserve SameValueZero behavior.
14. Use safe primitive, literal, or enum metadata to skip impossible union branches; test early, late, and no-match cases.
15. Compile discriminated unions to direct branch selection with fallback for inherited, missing, or accessor-backed discriminator fields.
16. Group numeric checks such as bounds, integer, and multiple-of into a single schema-plan pass.
17. Group string length/format checks and reuse compiled regular expressions when schema semantics permit.
18. Keep issue paths parent-linked during validation and materialize path arrays only when an error escapes.
19. Prefix nested issues lazily instead of allocating temporary arrays in each `prefixIssues` call.
20. Give first-error validation a separate traversal that stops before later children and issue construction.
21. Defer detailed union branch errors until the caller reads the failure's error property.
22. Compare Zod v4's shared lazy `safeParse` error accessor representation with the current Quench object layout for call speed and retained bytes.
23. Resolve error-map and global issue configuration once per parse, not once per issue.
24. Optimize stable output-object construction with correct `__proto__`, symbol, and inherited-field behavior.
25. Detect identity-only schema graphs and test whether output cloning can be avoided without violating mutation and clone guarantees.

## Fastify, Undici, ws, and Pino

26. Compile static Fastify route segments into a compact radix walk, avoiding per-request path splitting; sweep route-table size.
27. Add a one-parameter-segment route path with wildcard and constraint fallback; track allocations and fallback rate.
28. Cache method/handler lookup by route-table generation after a route match; measure invalidation cost.
29. Cache Fastify response serializer plans by object shape and schema identity; compare bytes and allocations per response.
30. Compile common required-field and primitive validation checks for reused Fastify request schemas; retain fallback for unsupported keywords.
31. Bypass Fastify Promise machinery for routes whose complete hook chain is synchronous; measure promises/request and tail latency.
32. Intern common case-insensitive Undici header names and reuse canonical forms across parse and lookup.
33. Store small header sets in an ordered compact vector and promote only when header count crosses a measured threshold.
34. Serialize Undici headers into one pre-sized byte buffer, writing canonical lowercase names directly.
35. Add a caller-buffer response-body path where the public API permits it; quantify avoided copies and RSS.
36. Preserve streamed body chunks as segments until a consumer requests contiguous storage; compare copies with retained segment memory.
37. Use an allocation-light Undici pool waiter queue with a no-waiter fast path; measure both queue churn and connection reuse.
38. Specialize incremental parsing for common `content-length` and `connection` headers, leaving uncommon syntax on the general parser.
39. Reduce Promise/microtask work in a stable local-server Undici request cycle and count tasks per completed request.
40. Unmask ws payloads word-at-a-time with an architecture-safe scalar tail; sweep frame size and alignment.
41. Combine ws unmasking and UTF-8 validation to avoid a second traversal for text frames.
42. Parse short ws control frames directly from the input slice without a separate payload buffer.
43. Collect fragmented ws message slices and copy once at completion instead of repeated concatenation.
44. Allocate outbound ws header and payload in one exact-sized buffer rather than joining temporary buffers.
45. Reuse bounded ws parse scratch storage with a strict large-frame fallback; compare RSS under mixed sizes.
46. Reuse compression state for ws configurations that permit context takeover; sweep concurrency and retained memory.
47. Cache Pino serializer plans for repeated plain-object shapes with guarded fallback for shape changes.
48. Compile Pino redaction paths into a prefix trie so shared path prefixes are traversed once.
49. Snapshot stable Pino bindings and mixin data; invalidate only when logging configuration changes.
50. Encode Pino records into reusable destination chunks where record ownership and flush semantics permit it.
51. Batch Pino destination writes while preserving order and flush guarantees; report buffered bytes and syscalls.
52. Add a Pino monomorphic primitive-serializer path with generic serializer fallback and a measured hit rate.

## sharp and Node-API boundary

53. Adopt libvips output buffers through N-API external Buffer ownership instead of copying bytes into guest memory; verify exactly-once finalization.
54. Connect `napi_adjust_external_memory` deltas to GC pacing and reclaim them on finalization; test large output fanout.
55. Transfer output allocation ownership to guest Buffers to avoid simultaneous native and guest copies during wrapping.
56. Audit the pinned-input path separately from output wrapping; count copies while preserving detached, shared, and resizable buffer checks.
57. Cache validated N-API Buffer storage resolution within a handle scope without caching raw guest handles across GC.
58. Batch N-API handle/root operations for one native callback; count roots, calls, and scope allocations.
59. Resolve typed-array metadata and backing storage once per `napi_get_typedarray_info` call.
60. Batch external-memory accounting over one image pipeline while preserving exact create/finalize deltas.
61. Drain multiple ready sharp async-work completions in one event-loop turn under a bounded batch size.
62. Tune sharp/libvips worker concurrency jointly and report throughput, tail latency, RSS, and oversubscription.
63. Bound scheduled image work by live native-plus-guest output bytes rather than task count.
64. Partition native completion queues by owning environment to avoid scanning unrelated N-API environments.
65. Replace contended shared completion queue locking with per-worker rings and bounded completion batching.
66. Recycle async-work task boxes and roots across cancellation/completion after measuring queue churn.
67. Sweep sharp cache disabled/tiny/automatic settings and include libvips cached bytes in memory accounting.
68. Compare libvips sequential and random access only for operation graphs whose access pattern is proven.

## Vite and esbuild

69. Split Vite transform benchmarks into warmed same-URL cache hits and unique-URL misses; record transform-cache reuse.
70. Compare Vite plugin hook filter widths while keeping transform callback work identical; count callbacks avoided.
71. Measure Vite `pre`/normal/`post` hook ordering and sequential versus parallel hook scheduling overhead.
72. Benchmark `import.meta.glob` expansion across eager/lazy and named-import shapes; measure generated bytes and path allocations.
73. Transform repeated CSS Modules files and measure export-map construction independently of CSS parsing.
74. Benchmark nested CSS `@import` and `url()` rewriting with and without source maps; track string copies.
75. Isolate repeated `import.meta.env` and `define` replacement scans across many modules.
76. Sweep assets just below/above Vite's inline threshold and report base64 allocations, output size, and RSS.
77. Measure many HTML entry points with multiple HTML hooks, holding total input bytes constant.
78. Compare Vite SSR dependency externalization and bundling by export-condition matrix; count module resolver calls.
79. Compare esbuild `buildSync` and async `build` on identical no-plugin inputs, splitting wrapper and native service time.
80. Compare esbuild `write: false` buffers with filesystem writes and attribute host I/O separately.
81. Sweep esbuild plugin filter selectivity while holding callback work fixed; measure callback dispatch cost.
82. Exercise esbuild plugin namespace fanout and resolve/load combinations; count namespace and callback lookups.
83. Compare primitive and nested structured data passed from esbuild `onResolve` to `onLoad`; measure bridge copies.
84. Measure esbuild `onEnd` sync and async plugin hooks separately from Go build time.
85. Split esbuild metafile generation from JS-side JSON parsing and count retained metadata objects.
86. Compare one-bundle and code-split esbuild outputs at equal source size; track path and output-object costs.
87. Sweep tree-shaking `sideEffects`, pure annotation, and re-export depth while reporting native/JS time split.
88. Sweep esbuild transform loader, target, and JSX options independently on repeated warm inputs.

## React and Next

89. Scan React text for escapable characters and copy unchanged spans in bulk; measure escape density and bytes copied.
90. Use a small ASCII escape table with a Unicode and special-character slow path; record cycles per code unit.
91. Cache escaped React strings within a render under a byte bound; record cache hit rate and retained bytes.
92. Reuse render-local scratch arrays/buffers across sequential `renderToString` calls while supporting reentrancy.
93. Measure per-call React render-state allocations and compact only the fixed-layout short-lived state that dominates.
94. Fuse React legacy-server chunk append with result accumulation to avoid intermediate chunk conversions/copies.
95. Add a guarded text-only host subtree path for strings, numbers, nulls, and arrays; count eligible subtree coverage.
96. Classify common intrinsic React tags with a compact fast path while preserving namespace/property fallbacks.
97. Cache repeated React style property hyphenation and unitless-property decisions; measure style-shape hit rates.
98. Profile React streaming chunk count, callback count, bytes/chunk, and buffering before specializing its writer.
99. Cache normalized Next route lookup keys by route-manifest generation; measure invalidation and repeated segment reuse.
100. Keep Next dynamic route parameters in a compact request representation and materialize public objects only when observed.

## Source anchors

- React server rendering: [legacy server implementation](https://github.com/facebook/react/blob/main/packages/react-dom/src/server/ReactDOMLegacyServerImpl.js) and [DOM/Fizz output](https://github.com/facebook/react/blob/main/packages/react-dom-bindings/src/server/ReactFizzConfigDOM.js).
- Next response caching: [response-cache source](https://github.com/vercel/next.js/blob/canary/packages/next/src/server/response-cache/index.ts).
- Vite: [plugin API](https://vite.dev/guide/api-plugin.html), [features](https://vite.dev/guide/features.html), [CSS](https://vite.dev/guide/features.html#css), and [SSR](https://vite.dev/guide/ssr.html).
- esbuild: [API](https://esbuild.github.io/api/) and its [plugin API](https://esbuild.github.io/plugins/).
- Fastify: [validation and serialization](https://fastify.dev/docs/latest/Reference/Validation-and-Serialization/); Undici: [Pool API](https://undici.nodejs.org/#/docs/api/Pool); ws: [documentation](https://github.com/websockets/ws/blob/master/doc/ws.md); Pino: [redaction](https://getpino.io/#/docs/redaction).
- sharp: [N-API external buffers](https://nodejs.org/api/n-api.html#napi_create_external_buffer), [async work](https://nodejs.org/api/n-api.html#asynchronous-operations), [concurrency and cache controls](https://sharp.pixelplumbing.com/api-utility/), and [libvips demand-driven execution](https://www.libvips.org/API/current/how-it-works.html).
- Zod: [v4 parsing](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/parse.ts), [schema implementation](https://github.com/colinhacks/zod/blob/main/packages/zod/src/v4/core/schemas.ts), and [v4 design notes](https://zod.dev/v4).
- Quench benchmark and prior results: [`tests/npm-perf/README.md`](../tests/npm-perf/README.md), [`dispatch_fast.rs`](../crates/quench-runtime/src/vm/dispatch_fast.rs), and Linear issue TOD-173.
